#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bolt;
mod config;
mod gfx;
mod hotkey;
mod overlay;
mod platform;
mod quick;
mod settings_ui;
mod theme;
mod update;
mod winkey;
mod worker;

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;

use lightning_core::{Hit, Kind};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
};
use windows::Win32::UI::Controls::{MARGINS, WM_MOUSELEAVE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    PAINTSTRUCT,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIN_BALLOONUSERCLICK, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::config::Config;
use crate::gfx::{Gfx, Target};
use crate::hotkey::Hotkey;
use crate::overlay::Overlay;
use crate::settings_ui::Settings;
use crate::theme::Palette;
use crate::worker::{Req, Resp};

pub const WM_APP_RESULTS: u32 = WM_APP + 1;
const WM_APP_TRAY: u32 = WM_APP + 2;
const WM_APP_SHOW: u32 = WM_APP + 3;
/// A lone Windows-key tap (see `winkey`).
const WM_APP_WINKEY: u32 = WM_APP + 4;
/// Quick settings states, packed in WPARAM (see `quick`).
pub const WM_APP_QUICK: u32 = WM_APP + 5;
/// Icons that followed a results message (see `worker`).
pub const WM_APP_ICONS: u32 = WM_APP + 6;
/// An update check finished (see `update`).
const WM_APP_UPDATE: u32 = WM_APP + 7;
const CARET_TIMER: usize = 1;
const STATS_TIMER: usize = 2;
const UPDATE_TIMER: usize = 3;
/// First automatic update check after start, then one a day.
const UPDATE_FIRST_MS: u32 = 2 * 60 * 1000;
const UPDATE_EVERY_MS: u32 = 24 * 60 * 60 * 1000;
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// Side effects that may re-enter a window procedure (showing windows,
/// modal menus...). Handlers queue them; they run once state is released.
pub enum Fx {
    Search,
    ShowOverlay,
    HideOverlay,
    ToggleOverlay,
    ShowSettings,
    HideSettings,
    ToggleSettings,
    Launch { hit: Hit, reveal: bool },
    /// From the right-click menu: the app's executable and shortcut
    /// arguments, or `None` for a packaged app.
    RunAsAdmin { hit: Hit, exe: Option<(String, Option<String>)> },
    OpenLocation { hit: Hit, path: String },
    /// Quick settings tile `i` switched to `on`.
    Quick(usize, bool),
    /// Resize the visible overlay to its content.
    FitOverlay,
    TrayMenu,
    Quit,
    ApplyTheme,
    /// Glass slider moved: only the panel tint changes.
    ApplyGlass,
    Sources,
    SuspendHotkey,
    ApplyHotkey,
    SetHotkey(String),
    CheckUpdate { manual: bool },
    /// Ask before installing the update found by a manual check.
    OfferUpdate,
    InstallUpdate,
    Message(String),
}

/// What an update check posts back with `WM_APP_UPDATE`.
type UpdateCheck = (bool, Result<Option<update::Update>, String>);

struct App {
    gfx: Gfx,
    cfg: Config,
    pal: Palette,
    worker: Sender<Req>,
    quick: Sender<quick::Req>,
    overlay: Overlay,
    settings: Settings,
    tray_icon: HICON,
    /// A newer release found by the last check.
    update: Option<update::Update>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs `f` on the app state unless it is already borrowed (re-entrant
/// message), in which case the message falls back to default handling.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

impl App {
    fn search(&mut self) {
        self.overlay.generation += 1;
        let _ = self.worker.send(Req::Query {
            generation: self.overlay.generation,
            query: self.overlay.query().to_owned(),
        });
    }

    fn register_hotkey(&mut self) -> bool {
        Hotkey::unregister(self.overlay.hwnd);
        if self.cfg.hotkey == hotkey::WIN_ONLY {
            winkey::enable(self.overlay.hwnd, WM_APP_WINKEY);
            return true;
        }
        // The keyboard hook stays only while it is needed.
        if !self.settings.recording {
            winkey::disable();
        }
        Hotkey::parse(&self.cfg.hotkey).is_some_and(|h| h.register(self.overlay.hwnd))
    }

    fn apply_theme(&mut self) {
        self.pal = theme::effective(self.cfg.theme, self.cfg.glass);
        for hwnd in [self.overlay.hwnd, self.settings.hwnd] {
            style_window(hwnd, &self.pal);
        }
        self.overlay.invalidate();
        self.settings.invalidate();
    }
}

fn main() -> windows::core::Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _mutex = CreateMutexW(None, true, w!("Local\\LightningLauncher.Instance"))?;
        if windows::Win32::Foundation::GetLastError() == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
            if let Ok(h) = FindWindowW(w!("LightningOverlay"), None) {
                // This process was just launched by the user, so Windows lets
                // it take the foreground; pass that right on, or the overlay
                // would appear without keyboard focus.
                let mut pid = 0u32;
                GetWindowThreadProcessId(h, Some(&mut pid));
                let _ = AllowSetForegroundWindow(pid);
                let _ = PostMessageW(Some(h), WM_APP_SHOW, WPARAM(0), LPARAM(0));
            }
            return Ok(());
        }
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);

        let mut cfg = Config::load();
        // The installer can create the Run entry too: the registry is the truth.
        cfg.launch_at_startup = platform::autostart_enabled();
        if cfg.launch_at_startup {
            platform::set_autostart(true); // keep the path current if the exe moved
        }
        let pal = theme::effective(cfg.theme, cfg.glass);
        let overlay_hwnd = create_window(w!("LightningOverlay"), Some(overlay_proc))?;
        let settings_hwnd = create_window(w!("LightningSettings"), Some(settings_proc))?;
        for h in [overlay_hwnd, settings_hwnd] {
            style_window(h, &pal);
        }

        let roots = lightning_core::files::default_roots();
        let worker = worker::spawn(overlay_hwnd, config::data_dir(), cfg.sources());
        let tray_icon = platform::bolt_icon().unwrap_or_default();
        add_tray(overlay_hwnd, tray_icon);
        if update::is_installed() {
            SetTimer(Some(overlay_hwnd), UPDATE_TIMER, UPDATE_FIRST_MS, None);
        }

        let app = App {
            gfx: Gfx::new()?,
            cfg,
            pal,
            worker,
            quick: quick::spawn(overlay_hwnd),
            overlay: Overlay::new(overlay_hwnd),
            settings: Settings::new(settings_hwnd, &roots),
            tray_icon,
            update: None,
        };
        APP.with(|a| *a.borrow_mut() = Some(app));

        let hotkey_ok = with_app(|a| {
            a.search(); // warm up: recents + counts ready before first show
            let ok = a.register_hotkey();
            if !ok {
                a.settings.hotkey_error = Some("Shortcut unavailable");
            }
            ok
        })
        .unwrap_or(false);
        if !hotkey_ok {
            run_fx(vec![Fx::ShowSettings]);
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(overlay_hwnd, HICON::default()));
    }
    Ok(())
}

unsafe fn create_window(class: PCWSTR, proc: WNDPROC) -> windows::core::Result<HWND> {
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: proc,
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class,
            w!("Lightning"),
            WS_POPUP,
            0,
            0,
            100,
            100,
            None,
            None,
            Some(hinst.into()),
            None,
        )
    }
}

/// Rounded corners, a hairline border and an acrylic blur behind the window.
/// The blur is composited by DWM, so it costs this process no CPU or memory;
/// the panel paints its own translucent tint over it. (The documented
/// DWMSBT_TRANSIENTWINDOW backdrop adds a heavy grey tint of its own that
/// hides the wallpaper's colour, so the accent-policy blur is used instead.)
fn style_window(hwnd: HWND, pal: &Palette) {
    let rgb = pal.border;
    let bgr = COLORREF(((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF));
    let dark: i32 = pal.dark.into();
    let set = |attr, v: *const core::ffi::c_void| unsafe {
        let _ = DwmSetWindowAttribute(hwnd, attr, v, 4);
    };
    set(DWMWA_WINDOW_CORNER_PREFERENCE, &DWMWCP_ROUND as *const _ as _);
    set(DWMWA_BORDER_COLOR, &bgr as *const _ as _);
    set(DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as _);
    platform::set_acrylic(hwnd);
    let m = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &m);
    }
}

fn tray_data(hwnd: HWND, icon: HICON) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_APP_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    for (i, c) in "Lightning".encode_utf16().enumerate() {
        nid.szTip[i] = c;
    }
    nid
}

fn add_tray(hwnd: HWND, icon: HICON) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &tray_data(hwnd, icon));
    }
}

/// A notification from the tray icon; clicking it sends NIN_BALLOONUSERCLICK.
fn notify(hwnd: HWND, icon: HICON, title: &str, text: &str) {
    let mut nid = tray_data(hwnd, icon);
    nid.uFlags |= NIF_INFO;
    nid.dwInfoFlags = NIIF_INFO;
    for (dst, c) in nid.szInfoTitle.iter_mut().zip(title.encode_utf16().take(63)) {
        *dst = c;
    }
    for (dst, c) in nid.szInfo.iter_mut().zip(text.encode_utf16().take(255)) {
        *dst = c;
    }
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

fn message_box(text: &str, style: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    use windows::core::HSTRING;
    unsafe { MessageBoxW(None, &HSTRING::from(text), w!("Lightning"), style | MB_SETFOREGROUND | MB_TOPMOST) }
}

/// Monitor under the cursor: (work area, DPI).
fn cursor_monitor() -> (RECT, f32) {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        (mi.rcWork, dx as f32)
    }
}

fn place(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_SHOWWINDOW);
    }
    focus(hwnd);
}

/// Gives `hwnd` keyboard focus. Windows refuses SetForegroundWindow from a
/// background process in some cases (e.g. opened from the tray or a second
/// instance), which left the overlay visible but deaf to Esc and typing;
/// briefly attaching to the foreground thread's input queue lifts that.
fn focus(hwnd: HWND) {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    unsafe {
        if SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd {
            return;
        }
        let fg = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let me = GetCurrentThreadId();
        let attached = fg != 0 && fg != me && AttachThreadInput(me, fg, true).as_bool();
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
        if attached {
            let _ = AttachThreadInput(me, fg, false);
        }
    }
}

fn run_fx(fx: Vec<Fx>) {
    for f in fx {
        run_one(f);
    }
}

fn run_one(f: Fx) {
    match f {
        Fx::Search => {
            with_app(|a| a.search());
        }
        Fx::ShowOverlay => {
            let (work, dpi) = cursor_monitor();
            let s = dpi / 96.0;
            let Some((hwnd, h)) = with_app(|a| {
                a.overlay.dpi = dpi;
                a.overlay.on_show();
                a.overlay.visible = true;
                a.search();
                let _ = a.worker.send(Req::RefreshApps);
                let _ = a.quick.send(quick::Req::Refresh);
                (a.overlay.hwnd, a.overlay.height())
            }) else {
                return;
            };
            let (w, h) = ((overlay::W * s).round() as i32, (h * s).round() as i32);
            let x = work.left + (work.right - work.left - w) / 2;
            let y = work.top + ((work.bottom - work.top) as f32 * 0.18) as i32;
            place(hwnd, x, y, w, h);
            unsafe {
                SetTimer(Some(hwnd), CARET_TIMER, 530, None);
            }
        }
        Fx::HideOverlay => {
            if let Some(hwnd) = with_app(|a| {
                a.overlay.visible = false;
                // Hidden most of the time: free the frame buffer and icon bitmaps.
                a.overlay.drop_target();
                a.overlay.hwnd
            }) {
                unsafe {
                    let _ = KillTimer(Some(hwnd), CARET_TIMER);
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            }
        }
        Fx::ToggleOverlay => {
            let Some((visible, hwnd)) = with_app(|a| (a.overlay.visible, a.overlay.hwnd)) else { return };
            if !visible {
                run_one(Fx::ShowOverlay);
            } else if unsafe { GetForegroundWindow() } == hwnd {
                run_one(Fx::HideOverlay);
            } else {
                // Still on top but another app has focus: take focus back.
                focus(hwnd);
            }
        }
        Fx::ShowSettings => {
            let (work, dpi) = cursor_monitor();
            let s = dpi / 96.0;
            let Some((hwnd, h)) = with_app(|a| {
                a.settings.dpi = dpi;
                a.settings.visible = true;
                a.search(); // refreshes the indexed count
                (a.settings.hwnd, a.settings.height())
            }) else {
                return;
            };
            let (w, h) = ((settings_ui::W * s).round() as i32, (h * s).round() as i32);
            let m = (12.0 * s) as i32;
            place(hwnd, work.right - w - m, work.bottom - h - m, w, h);
            // Hidden only now: while the overlay is still the foreground
            // window, Windows lets Settings take focus (and so close on Esc
            // or when clicking away).
            run_one(Fx::HideOverlay);
            unsafe {
                SetTimer(Some(hwnd), STATS_TIMER, 1000, None);
            }
        }
        Fx::HideSettings => {
            if let Some((hwnd, recording)) = with_app(|a| {
                a.settings.visible = false;
                a.settings.target = None;
                let r = a.settings.recording;
                a.settings.recording = false;
                (a.settings.hwnd, r)
            }) {
                if recording {
                    run_one(Fx::ApplyHotkey);
                }
                unsafe {
                    let _ = KillTimer(Some(hwnd), STATS_TIMER);
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            }
        }
        Fx::ToggleSettings => {
            let visible = with_app(|a| a.settings.visible).unwrap_or(false);
            run_one(if visible { Fx::HideSettings } else { Fx::ShowSettings });
        }
        Fx::Launch { hit, reveal } => {
            run_one(Fx::HideOverlay);
            let ok = if reveal && hit.kind == Kind::File {
                platform::reveal(&hit.target);
                true
            } else {
                platform::open(&hit.target)
            };
            if ok {
                with_app(|a| a.worker.send(Req::Launched(hit)));
            }
        }
        Fx::FitOverlay => {
            let Some((hwnd, h, dpi)) =
                with_app(|a| a.overlay.visible.then(|| (a.overlay.hwnd, a.overlay.height(), a.overlay.dpi))).flatten()
            else {
                return;
            };
            let h = (h * dpi / 96.0).round() as i32;
            let mut rc = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut rc);
                if rc.bottom - rc.top != h {
                    // Grows downward from a fixed top edge.
                    let flags = SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE;
                    let _ = SetWindowPos(hwnd, None, 0, 0, rc.right - rc.left, h, flags);
                }
            }
        }
        Fx::Quick(i, on) => {
            with_app(|a| {
                let _ = a.quick.send(quick::Req::Set(i, on));
                // Auto already follows Windows; a fixed theme switches with the tile.
                if i == quick::THEME && a.cfg.theme != config::ThemeMode::Auto {
                    a.cfg.theme = if on { config::ThemeMode::Dark } else { config::ThemeMode::Light };
                    a.cfg.save();
                    a.apply_theme();
                }
            });
        }
        Fx::RunAsAdmin { hit, exe } => {
            let Some(hwnd) = with_app(|a| a.overlay.hwnd) else { return };
            run_one(Fx::HideOverlay);
            let ok = match exe {
                Some((path, args)) => platform::run_as_admin(&path, args.as_deref()),
                None => platform::run_as_admin_packaged(&hit.target, hwnd),
            };
            if ok {
                with_app(|a| a.worker.send(Req::Launched(hit)));
            }
        }
        Fx::OpenLocation { hit, path } => {
            run_one(Fx::HideOverlay);
            platform::reveal(&path);
            with_app(|a| a.worker.send(Req::Launched(hit)));
        }
        Fx::TrayMenu => tray_menu(),
        Fx::CheckUpdate { manual } => {
            let Some(hwnd) = with_app(|a| a.overlay.hwnd) else { return };
            let hwnd = hwnd.0 as isize;
            let _ = std::thread::Builder::new().name("update".into()).spawn(move || {
                let check: Box<UpdateCheck> = Box::new((manual, update::check()));
                let ptr = Box::into_raw(check);
                unsafe {
                    if PostMessageW(Some(HWND(hwnd as *mut _)), WM_APP_UPDATE, WPARAM(0), LPARAM(ptr as isize)).is_err() {
                        drop(Box::from_raw(ptr));
                    }
                }
            });
        }
        Fx::OfferUpdate => {
            let Some(Some(v)) = with_app(|a| a.update.as_ref().map(|u| u.version.clone())) else { return };
            let text = format!(
                "Lightning {v} is available (you have {}).\n\nUpdate now? Lightning restarts by itself.",
                env!("CARGO_PKG_VERSION")
            );
            if message_box(&text, MB_YESNO | MB_ICONINFORMATION) == IDYES {
                run_one(Fx::InstallUpdate);
            }
        }
        Fx::InstallUpdate => {
            // Taken, so a second click can't start a second installer.
            let Some(Some(u)) = with_app(|a| a.update.take()) else { return };
            let _ = std::thread::Builder::new().name("update".into()).spawn(move || {
                if let Err(e) = update::install(&u) {
                    message_box(&format!("Couldn't update Lightning: {e}"), MB_OK | MB_ICONWARNING);
                }
            });
        }
        Fx::Message(text) => {
            message_box(&text, MB_OK | MB_ICONINFORMATION);
        }
        Fx::Quit => unsafe { PostQuitMessage(0) },
        Fx::ApplyTheme => {
            with_app(|a| a.apply_theme());
        }
        Fx::ApplyGlass => {
            with_app(|a| {
                a.pal = theme::effective(a.cfg.theme, a.cfg.glass);
                a.overlay.invalidate();
                a.settings.invalidate();
            });
        }
        Fx::Sources => {
            with_app(|a| {
                let _ = a.worker.send(Req::Sources(a.cfg.sources()));
                a.search();
            });
        }
        Fx::SuspendHotkey => {
            with_app(|a| {
                Hotkey::unregister(a.overlay.hwnd);
                // Recording: catch a lone Win tap (and keep Start closed).
                winkey::enable(a.overlay.hwnd, WM_APP_WINKEY);
            });
        }
        Fx::ApplyHotkey => {
            with_app(|a| {
                if !a.register_hotkey() {
                    a.settings.hotkey_error = Some("Shortcut unavailable");
                }
                a.settings.invalidate();
            });
        }
        Fx::SetHotkey(s) => {
            with_app(|a| {
                let old = std::mem::replace(&mut a.cfg.hotkey, s);
                if a.register_hotkey() {
                    a.settings.hotkey_error = None;
                    a.cfg.save();
                } else {
                    a.cfg.hotkey = old;
                    a.register_hotkey();
                    a.settings.hotkey_error = Some("Taken by another app");
                }
                a.settings.invalidate();
            });
        }
    }
}

fn tray_menu() {
    let Some((hwnd, version)) = with_app(|a| (a.overlay.hwnd, a.update.as_ref().map(|u| u.version.clone())))
    else {
        return;
    };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, 1, w!("Open Lightning"));
        let _ = AppendMenuW(menu, MF_STRING, 2, w!("Settings"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        match &version {
            Some(v) => {
                let label = windows::core::HSTRING::from(format!("Update to {v}"));
                let _ = AppendMenuW(menu, MF_STRING, 4, &label);
            }
            None => {
                let _ = AppendMenuW(menu, MF_STRING, 5, w!("Check for updates"));
            }
        }
        let _ = AppendMenuW(menu, MF_STRING, 3, w!("Quit"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, None, hwnd, None);
        let _ = DestroyMenu(menu);
        match cmd.0 {
            1 => run_one(Fx::ShowOverlay),
            2 => run_one(Fx::ShowSettings),
            3 => run_one(Fx::Quit),
            4 => run_one(Fx::InstallUpdate),
            5 => run_one(Fx::CheckUpdate { manual: true }),
            _ => {}
        }
    }
}

fn lo(l: LPARAM) -> i32 {
    (l.0 & 0xFFFF) as i16 as i32
}
fn hi(l: LPARAM) -> i32 {
    ((l.0 >> 16) & 0xFFFF) as i16 as i32
}

fn client_size(hwnd: HWND) -> (u32, u32) {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    ((rc.right - rc.left) as u32, (rc.bottom - rc.top) as u32)
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let mut fx = Vec::new();
    let handled: Option<LRESULT> = match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe { BeginPaint(hwnd, &mut ps) };
            with_app(|a| {
                if a.overlay.target.is_none() {
                    a.overlay.target = Target::new(&a.gfx, hwnd, a.overlay.dpi).ok();
                }
                let hk = a.cfg.hotkey.clone();
                a.overlay.paint(&a.gfx, &a.pal, &hk);
            });
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            Some(LRESULT(0))
        }
        WM_ERASEBKGND => Some(LRESULT(1)),
        WM_SIZE => {
            let (w, h) = client_size(hwnd);
            with_app(|a| {
                if let Some(t) = &mut a.overlay.target {
                    t.resize(w, h, a.overlay.dpi);
                }
            });
            Some(LRESULT(0))
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => with_app(|a| a.overlay.on_key(wp.0 as u16, &mut fx))
            .filter(|&h| h)
            .map(|_| LRESULT(0)),
        WM_CHAR => {
            with_app(|a| a.overlay.on_char(wp.0 as u16, &mut fx));
            Some(LRESULT(0))
        }
        WM_MOUSEMOVE => {
            // Ask for WM_MOUSELEAVE so hover highlights clear off-window.
            let mut tme = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
            with_app(|a| a.overlay.on_mouse_move(lo(lp), hi(lp)));
            Some(LRESULT(0))
        }
        WM_MOUSELEAVE => {
            with_app(|a| a.overlay.on_mouse_leave());
            Some(LRESULT(0))
        }
        WM_LBUTTONDOWN => {
            with_app(|a| a.overlay.on_click(lo(lp), hi(lp), &mut fx));
            Some(LRESULT(0))
        }
        WM_RBUTTONUP => {
            with_app(|a| a.overlay.on_right_click(lo(lp), hi(lp)));
            Some(LRESULT(0))
        }
        WM_MOUSEWHEEL => {
            with_app(|a| a.overlay.on_wheel(((wp.0 >> 16) & 0xFFFF) as i16));
            Some(LRESULT(0))
        }
        WM_ACTIVATE => {
            // With "Always on top" the overlay stays up when another app takes
            // focus (it hides on Esc, the hotkey, or after launching);
            // otherwise losing focus closes it.
            if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE && with_app(|a| !a.cfg.always_on_top) == Some(true) {
                fx.push(Fx::HideOverlay);
            }
            None
        }
        WM_TIMER if wp.0 == CARET_TIMER => {
            with_app(|a| {
                a.overlay.caret_on = !a.overlay.caret_on;
                a.overlay.invalidate();
            });
            Some(LRESULT(0))
        }
        WM_TIMER if wp.0 == UPDATE_TIMER => {
            unsafe {
                SetTimer(Some(hwnd), UPDATE_TIMER, UPDATE_EVERY_MS, None);
            }
            fx.push(Fx::CheckUpdate { manual: false });
            Some(LRESULT(0))
        }
        WM_APP_UPDATE => {
            let (manual, result) = *unsafe { Box::from_raw(lp.0 as *mut UpdateCheck) };
            match result {
                Ok(Some(u)) => {
                    let known = with_app(|a| a.update.as_ref().is_some_and(|k| k.version == u.version));
                    if manual {
                        fx.push(Fx::OfferUpdate);
                    } else if known == Some(false)
                        && let Some(icon) = with_app(|a| a.tray_icon)
                    {
                        let text = format!("Version {} is ready. Click to update.", u.version);
                        notify(hwnd, icon, "Lightning update available", &text);
                    }
                    with_app(|a| a.update = Some(u));
                }
                Ok(None) if manual => fx.push(Fx::Message(format!(
                    "Lightning {} is the latest version.",
                    env!("CARGO_PKG_VERSION")
                ))),
                Err(e) if manual => fx.push(Fx::Message(format!("Couldn't check for updates: {e}"))),
                _ => {}
            }
            Some(LRESULT(0))
        }
        WM_HOTKEY => {
            fx.push(Fx::ToggleOverlay);
            Some(LRESULT(0))
        }
        WM_APP_WINKEY => {
            let recording = with_app(|a| std::mem::take(&mut a.settings.recording)).unwrap_or(false);
            fx.push(if recording { Fx::SetHotkey(hotkey::WIN_ONLY.into()) } else { Fx::ToggleOverlay });
            Some(LRESULT(0))
        }
        WM_APP_RESULTS => {
            let resp = unsafe { Box::from_raw(lp.0 as *mut Resp) };
            with_app(|a| {
                a.overlay.on_results(resp);
                if a.settings.visible {
                    a.settings.invalidate();
                }
            });
            fx.push(Fx::FitOverlay);
            Some(LRESULT(0))
        }
        WM_APP_ICONS => {
            let icons = unsafe { Box::from_raw(lp.0 as *mut worker::Icons) };
            with_app(|a| a.overlay.on_icons(*icons));
            Some(LRESULT(0))
        }
        WM_APP_QUICK => {
            with_app(|a| {
                a.overlay.quick = quick::unpack(wp.0);
                a.overlay.invalidate();
            });
            Some(LRESULT(0))
        }
        WM_APP_TRAY => {
            match (lp.0 & 0xFFFF) as u32 {
                WM_LBUTTONUP => fx.push(Fx::ToggleSettings),
                WM_RBUTTONUP => fx.push(Fx::TrayMenu),
                NIN_BALLOONUSERCLICK => fx.push(Fx::InstallUpdate),
                _ => {}
            }
            Some(LRESULT(0))
        }
        WM_APP_SHOW => {
            fx.push(Fx::ShowOverlay);
            Some(LRESULT(0))
        }
        WM_SETTINGCHANGE => {
            let p = lp.0 as *const u16;
            if !p.is_null() && unsafe { PCWSTR(p).to_string() }.is_ok_and(|s| s == "ImmersiveColorSet") {
                fx.push(Fx::ApplyTheme);
            }
            None
        }
        WM_DPICHANGED => {
            let dpi = (wp.0 & 0xFFFF) as f32;
            with_app(|a| {
                a.overlay.dpi = dpi;
                a.overlay.drop_target();
            });
            Some(LRESULT(0))
        }
        WM_CLOSE => {
            fx.push(Fx::HideOverlay);
            Some(LRESULT(0))
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            if let Some(icon) = with_app(|a| a.tray_icon) {
                add_tray(hwnd, icon);
            }
            Some(LRESULT(0))
        }
        _ => None,
    };
    run_fx(fx);
    handled.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, wp, lp) })
}

unsafe extern "system" fn settings_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let mut fx = Vec::new();
    let handled: Option<LRESULT> = match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe { BeginPaint(hwnd, &mut ps) };
            with_app(|a| {
                if a.settings.target.is_none() {
                    a.settings.target = Target::new(&a.gfx, hwnd, a.settings.dpi).ok();
                }
                let indexed = a.overlay.indexed;
                a.settings.paint(&a.gfx, &a.pal, &a.cfg, indexed);
            });
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            Some(LRESULT(0))
        }
        WM_ERASEBKGND => Some(LRESULT(1)),
        WM_SIZE => {
            let (w, h) = client_size(hwnd);
            with_app(|a| {
                if let Some(t) = &mut a.settings.target {
                    t.resize(w, h, a.settings.dpi);
                }
            });
            Some(LRESULT(0))
        }
        WM_LBUTTONDOWN => {
            if with_app(|a| {
                a.settings.on_click(lo(lp), hi(lp), &mut a.cfg, &mut fx);
                a.settings.dragging
            }) == Some(true)
            {
                // Keep receiving moves while the slider is dragged off-panel.
                unsafe { SetCapture(hwnd) };
            }
            Some(LRESULT(0))
        }
        WM_MOUSEMOVE => {
            with_app(|a| a.settings.on_mouse_move(lo(lp), &mut a.cfg, &mut fx));
            Some(LRESULT(0))
        }
        WM_LBUTTONUP => {
            if with_app(|a| a.settings.dragging) == Some(true) {
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            Some(LRESULT(0))
        }
        WM_CAPTURECHANGED => {
            with_app(|a| a.settings.on_mouse_up(&a.cfg));
            Some(LRESULT(0))
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => with_app(|a| a.settings.on_key(wp.0 as u32, &mut fx))
            .filter(|&h| h)
            .map(|_| LRESULT(0)),
        WM_SYSCHAR => Some(LRESULT(0)),
        WM_ACTIVATE => {
            if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE {
                fx.push(Fx::HideSettings);
            }
            None
        }
        WM_TIMER if wp.0 == STATS_TIMER => {
            with_app(|a| a.settings.invalidate());
            Some(LRESULT(0))
        }
        WM_DPICHANGED => {
            let dpi = (wp.0 & 0xFFFF) as f32;
            with_app(|a| {
                a.settings.dpi = dpi;
                a.settings.target = None;
            });
            Some(LRESULT(0))
        }
        WM_CLOSE => {
            fx.push(Fx::HideSettings);
            Some(LRESULT(0))
        }
        _ => None,
    };
    run_fx(fx);
    handled.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, wp, lp) })
}
