//! Small Win32 helpers: launching, clipboard, autostart, memory, icon.

use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject, HGDIOBJ,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW};
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, GetSystemMetrics, HICON, ICONINFO, SM_CXSMICON, SW_SHOWNORMAL,
};
use windows::core::{HSTRING, PCSTR, PCWSTR, PSTR, w};

pub fn open(target: &str) -> bool {
    if let Some(hex) = target.strip_prefix("pidl:") {
        return open_pidl(hex);
    }
    let r = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL) };
    r.0 as isize > 32
}

/// Opens a shell item (Control Panel task) from its serialized ID list.
fn open_pidl(hex: &str) -> bool {
    use windows::Win32::UI::Shell::{SEE_MASK_IDLIST, SHELLEXECUTEINFOW, ShellExecuteExW};
    let bytes: Option<Vec<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| hex.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()))
        .collect();
    // A valid ID list ends with a zero u16 terminator.
    let Some(bytes) = bytes.filter(|b| b.len() >= 2 && b.ends_with(&[0, 0])) else { return false };
    let mut sei = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_IDLIST,
        lpIDList: bytes.as_ptr() as *mut _,
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut sei).is_ok() }
}

/// Acrylic blur-behind via `SetWindowCompositionAttribute`, an undocumented
/// but long-stable user32 export (resolved at runtime; a missing export just
/// leaves the panel tinted without blur).
pub fn set_acrylic(hwnd: HWND) {
    use windows::core::BOOL;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::core::s;

    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient: u32,
        animation: u32,
    }
    #[repr(C)]
    struct CompositionData {
        attribute: u32,
        data: *mut AccentPolicy,
        size: usize,
    }
    type SetWca = unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL;
    const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
    const WCA_ACCENT_POLICY: u32 = 19;

    unsafe {
        let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else { return };
        let Some(f) = GetProcAddress(user32, s!("SetWindowCompositionAttribute")) else { return };
        let f: SetWca = std::mem::transmute(f);
        // The panel paints its own tint, so the blur itself stays untinted
        // (a fully transparent gradient renders black, hence alpha 1).
        let mut accent = AccentPolicy { state: ACCENT_ENABLE_ACRYLICBLURBEHIND, flags: 0, gradient: 0x0100_0000, animation: 0 };
        let mut data = CompositionData {
            attribute: WCA_ACCENT_POLICY,
            data: &mut accent,
            size: size_of::<AccentPolicy>(),
        };
        let _ = f(hwnd, &mut data);
    }
}

/// Runs `path` elevated (a UAC prompt). False if it was refused or failed.
pub fn run_as_admin(path: &str, args: Option<&str>) -> bool {
    let args = args.map(HSTRING::from);
    let params = args.as_ref().map_or(PCWSTR::null(), |a| PCWSTR(a.as_ptr()));
    let r = unsafe { ShellExecuteW(None, w!("runas"), &HSTRING::from(path), params, None, SW_SHOWNORMAL) };
    r.0 as isize > 32
}

/// Files that can run elevated.
pub fn is_executable(path: &str) -> bool {
    const EXT: &[&str] = &["exe", "bat", "cmd", "com", "msi", "lnk"];
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXT.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// A classic app's executable and shortcut arguments, read from its
/// AppsFolder item's properties (cheap, unlike building its context menu).
/// Packaged apps, and apps that are shell folders, have none.
pub fn app_exe(target: &str) -> Option<(String, Option<String>)> {
    use windows::Win32::Storage::EnhancedStorage::{PKEY_Link_Arguments, PKEY_Link_TargetParsingPath};
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{IShellItem2, SHCreateItemFromParsingName};
    unsafe {
        let item: IShellItem2 = SHCreateItemFromParsingName(&HSTRING::from(target), None).ok()?;
        let get = |key| {
            let p = item.GetString(key).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s.filter(|s| !s.is_empty())
        };
        let exe = get(&PKEY_Link_TargetParsingPath).filter(|p| !p.starts_with("::"))?;
        Some((exe, get(&PKEY_Link_Arguments)))
    }
}

/// Packaged apps have no executable to elevate, so this invokes "Run as
/// administrator" from the app's own shell menu, as the Start menu does.
/// That handler is heavy, so it is loaded only here and unloaded after.
pub fn run_as_admin_packaged(target: &str, owner: HWND) -> bool {
    use windows::Win32::System::Com::CoFreeUnusedLibrariesEx;
    use windows::Win32::UI::Shell::{
        BHID_SFUIObject, CMF_NORMAL, CMINVOKECOMMANDINFO, GCS_VERBW, IContextMenu, IShellItem,
        SHCreateItemFromParsingName,
    };
    use windows::Win32::UI::WindowsAndMessaging::{CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuItemID};
    let run = || unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(target), None).ok()?;
        let cm: IContextMenu = item.BindToHandler(None, &BHID_SFUIObject).ok()?;
        let menu = CreatePopupMenu().ok()?;
        let mut cmd = None;
        if cm.QueryContextMenu(menu, 0, 1, 0x7FFF, CMF_NORMAL).is_ok() {
            for i in 0..GetMenuItemCount(Some(menu)) {
                let id = GetMenuItemID(menu, i);
                if id == 0 || id == u32::MAX {
                    continue; // separator or submenu
                }
                let mut buf = [0u16; 64];
                let ok = cm.GetCommandString((id - 1) as usize, GCS_VERBW, None, PSTR(buf.as_mut_ptr() as _), 64);
                let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
                if ok.is_ok() && String::from_utf16_lossy(&buf[..n]).eq_ignore_ascii_case("runas") {
                    cmd = Some(id - 1);
                    break;
                }
            }
        }
        let ok = cmd.is_some_and(|c| {
            let info = CMINVOKECOMMANDINFO {
                cbSize: size_of::<CMINVOKECOMMANDINFO>() as u32,
                hwnd: owner,
                lpVerb: PCSTR(c as usize as *const u8), // MAKEINTRESOURCE
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            cm.InvokeCommand(&info).is_ok()
        });
        let _ = DestroyMenu(menu);
        Some(ok)
    };
    let ok = run().unwrap_or(false);
    // The handler is released by now, so its DLLs can unload.
    unsafe { CoFreeUnusedLibrariesEx(0, None) };
    ok
}

pub fn reveal(path: &str) {
    let args = HSTRING::from(format!("/select,\"{path}\""));
    unsafe {
        ShellExecuteW(None, w!("open"), w!("explorer.exe"), &args, None, SW_SHOWNORMAL);
    }
}

pub fn copy_text(hwnd: HWND, s: &str) {
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    wide.push(0);
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                let _ = GlobalUnlock(h);
                let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(h.0)));
            }
        }
        let _ = CloseClipboard();
    }
}

pub fn paste_text(hwnd: HWND) -> Option<String> {
    unsafe {
        OpenClipboard(Some(hwnd)).ok()?;
        let out = (|| {
            let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
            let p = GlobalLock(windows::Win32::Foundation::HGLOBAL(h.0)) as *const u16;
            if p.is_null() {
                return None;
            }
            let len = (0..).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            let _ = GlobalUnlock(windows::Win32::Foundation::HGLOBAL(h.0));
            Some(s)
        })();
        let _ = CloseClipboard();
        out
    }
}

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

pub fn set_autostart(enabled: bool) {
    unsafe {
        if enabled {
            let Ok(exe) = std::env::current_exe() else { return };
            let mut v: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().collect();
            v.push(0);
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                w!("Lightning"),
                REG_SZ.0,
                Some(v.as_ptr() as *const _),
                (v.len() * 2) as u32,
            );
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, w!("Lightning"));
        }
    }
}

/// Whether the HKCU Run entry exists (set by the toggle or the installer).
pub fn autostart_enabled() -> bool {
    use windows::Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW};
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, w!("Lightning"), RRF_RT_REG_SZ, None, None, None).is_ok() }
}

/// Private bytes of this process.
pub fn private_bytes() -> usize {
    let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
    unsafe {
        let _ = GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        );
    }
    c.PrivateUsage
}

/// The tray icon, rasterized at the exact tray size (the embedded .ico is
/// for Explorer, Start and the installer).
pub fn bolt_icon() -> Option<HICON> {
    let s = unsafe { GetSystemMetrics(SM_CXSMICON) }.clamp(16, 64);
    let px = crate::bolt::rasterize(s as usize);
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: s,
                biHeight: -s,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u8, px.len());
        let mask = CreateBitmap(s, s, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), hbmMask: mask, hbmColor: color, ..Default::default() };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        icon
    }
}

#[cfg(test)]
mod tests {
    /// Opens a real window: `cargo test -p lightning -- --ignored`.
    #[test]
    #[ignore]
    fn opens_control_panel_task() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let tasks = lightning_core::apps::enumerate_tasks();
        let t = tasks.iter().find(|t| t.name.contains("environment variables for your account")).expect("task listed");
        assert!(super::open(&t.target()), "failed to open {}", t.name);
    }
}
