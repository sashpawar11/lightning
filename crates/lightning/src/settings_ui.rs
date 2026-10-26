//! The small settings panel that opens from the tray icon.

use std::path::Path;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_CONTROL, VK_LWIN,
    VK_MENU, VK_RWIN, VK_SHIFT,
};

use crate::config::{Config, ThemeMode};
use crate::gfx::{Gfx, Target, rect};
use crate::icon::glyph;
use crate::hotkey::{Hotkey, bindable_key, display_parts};
use crate::theme::Palette;
use crate::{Fx, platform};

pub const W: f32 = 380.0;
const PAD: f32 = 16.0;

#[derive(Clone, Copy, PartialEq)]
enum Ctl {
    Record,
    Theme(ThemeMode),
    Glass,
    Toggle(u8),
    Quit,
    Close,
}

pub struct Settings {
    pub hwnd: HWND,
    pub target: Option<Target>,
    pub dpi: f32,
    pub visible: bool,
    pub recording: bool,
    pub hotkey_error: Option<&'static str>,
    hits: Vec<(D2D_RECT_F, Ctl)>,
    drives: Vec<(String, usize)>,
    /// Glass slider track, for drag hit-testing.
    track: D2D_RECT_F,
    pub dragging: bool,
}

impl Settings {
    pub fn new(hwnd: HWND, roots: &[std::path::PathBuf]) -> Self {
        // "C:\" -> number of top-level folders indexed on it.
        let mut drives: Vec<(String, usize)> = Vec::new();
        for r in roots {
            let d = drive_of(r);
            // A drive indexed as one root counts 0 folders ("Whole drive").
            let n = usize::from(r.parent().is_some());
            match drives.iter_mut().find(|(k, _)| *k == d) {
                Some(e) => e.1 += n,
                None => drives.push((d, n)),
            }
        }
        Self {
            hwnd,
            target: None,
            dpi: 96.0,
            visible: false,
            recording: false,
            hotkey_error: None,
            hits: Vec::new(),
            drives,
            track: D2D_RECT_F::default(),
            dragging: false,
        }
    }

    pub fn height(&self) -> f32 {
        let drives = self.drives.len().clamp(1, 4) as f32;
        54.0 + PAD + (16.0 + 8.0 + 40.0) + 18.0 + (16.0 + 8.0 + 40.0) + 18.0 + (22.0 + 28.0) + 18.0
            + (22.0 + 5.0 * 36.0) + 18.0
            + (22.0 + drives * 38.0 + 16.0) + PAD + 40.0
    }

    pub fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    pub fn on_click(&mut self, x: i32, y: i32, cfg: &mut Config, fx: &mut Vec<Fx>) {
        let (x, y) = (x as f32 * 96.0 / self.dpi, y as f32 * 96.0 / self.dpi);
        let Some(&(_, ctl)) = self
            .hits
            .iter()
            .find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
        else {
            return;
        };
        match ctl {
            Ctl::Record => {
                self.recording = !self.recording;
                self.hotkey_error = None;
                fx.push(if self.recording { Fx::SuspendHotkey } else { Fx::ApplyHotkey });
            }
            Ctl::Theme(m) => {
                cfg.theme = m;
                cfg.save();
                fx.push(Fx::ApplyTheme);
            }
            Ctl::Toggle(k) => {
                let v = match k {
                    0 => &mut cfg.search_apps,
                    1 => &mut cfg.search_files,
                    2 => &mut cfg.search_settings,
                    3 => &mut cfg.launch_at_startup,
                    _ => &mut cfg.always_on_top,
                };
                *v = !*v;
                if k == 3 {
                    platform::set_autostart(cfg.launch_at_startup);
                }
                cfg.save();
                fx.push(Fx::Sources);
            }
            Ctl::Glass => {
                self.dragging = true;
                self.drag_to(x, cfg, fx);
            }
            Ctl::Quit => fx.push(Fx::Quit),
            Ctl::Close => fx.push(Fx::HideSettings),
        }
        self.invalidate();
    }

    fn drag_to(&mut self, x_dip: f32, cfg: &mut Config, fx: &mut Vec<Fx>) {
        let t = self.track;
        let v = ((x_dip - t.left) / (t.right - t.left)).clamp(0.0, 1.0);
        let v = (v * 100.0).round() as u8;
        if v != cfg.glass {
            cfg.glass = v;
            fx.push(Fx::ApplyGlass);
        }
    }

    /// Live update while dragging; saved on release.
    pub fn on_mouse_move(&mut self, x: i32, cfg: &mut Config, fx: &mut Vec<Fx>) {
        if self.dragging {
            self.drag_to(x as f32 * 96.0 / self.dpi, cfg, fx);
        }
    }

    pub fn on_mouse_up(&mut self, cfg: &Config) {
        if std::mem::take(&mut self.dragging) {
            cfg.save();
        }
    }

    /// While recording, turns the next modifier+key press into the hotkey.
    pub fn on_key(&mut self, vk: u32, fx: &mut Vec<Fx>) -> bool {
        if !self.recording {
            if vk == 0x1B {
                fx.push(Fx::HideSettings);
                return true;
            }
            return false;
        }
        if vk == 0x1B {
            self.recording = false;
            fx.push(Fx::ApplyHotkey);
            self.invalidate();
            return true;
        }
        if Hotkey::is_modifier_vk(vk) {
            return true;
        }
        let down = |k: u16| unsafe { GetKeyState(k as i32) < 0 };
        let mut mods = HOT_KEY_MODIFIERS(0);
        if down(VK_CONTROL.0) {
            mods |= MOD_CONTROL;
        }
        if down(VK_MENU.0) {
            mods |= MOD_ALT;
        }
        if down(VK_SHIFT.0) {
            mods |= MOD_SHIFT;
        }
        if down(VK_LWIN.0) || down(VK_RWIN.0) {
            mods |= MOD_WIN;
        }
        let Some(vk) = bindable_key(vk) else { return true };
        if mods.0 == 0 {
            self.hotkey_error = Some("Add Ctrl, Alt, Shift or Win");
        } else {
            self.recording = false;
            fx.push(Fx::SetHotkey(Hotkey { mods, vk }.to_string()));
        }
        self.invalidate();
        true
    }

    pub fn paint(&mut self, gfx: &Gfx, pal: &Palette, cfg: &Config, indexed: usize) {
        let Some(t) = self.target.as_ref() else { return };
        let f = &gfx.fonts;
        self.hits.clear();
        let h = self.height();
        t.begin();
        t.clear_a(pal.panel, pal.glass);

        // Header
        t.fill_round(rect(PAD, 14.0, 26.0, 26.0), 7.0, pal.accent);
        t.icon(gfx, glyph::BOLT, 18.0, rect(PAD, 14.0, 26.0, 26.0), pal.on_accent);
        t.text("Lightning", &f.heading, rect(PAD + 36.0, 14.0, 200.0, 26.0), pal.text);
        // Minimize: hides the panel, like Esc or clicking away.
        let close = rect(W - PAD - 26.0, 14.0, 26.0, 26.0);
        t.fill_round_a(close, 7.0, pal.text, 0.06);
        t.icon(gfx, glyph::MINIMIZE, 18.0, close, pal.text);
        self.hits.push((close, Ctl::Close));
        let ver = concat!("v", env!("CARGO_PKG_VERSION"));
        let vw = gfx.text_width(ver, &f.small);
        t.text(ver, &f.small, rect(close.left - 10.0 - vw, 14.0, vw + 2.0, 26.0), pal.muted);
        t.fill_a(rect(0.0, 54.0, W, 1.0), pal.text, 0.08);

        let mut y = 54.0 + PAD;
        let label = |t: &Target, s: &str, y: f32| t.text(s, &f.label, rect(PAD, y, 300.0, 16.0), pal.muted);

        // Hotkey
        label(t, "Hotkey", y);
        y += 24.0;
        let field = rect(PAD, y, W - PAD * 2.0 - 90.0, 40.0);
        t.fill_round_a(field, 8.0, pal.text, 0.05);
        t.stroke_round(field, 8.0, if self.recording { pal.accent_ink } else { pal.border }, 1.0);
        if self.recording {
            let msg = self.hotkey_error.unwrap_or("Press a shortcut or tap Win…  Esc cancels");
            t.text(msg, &f.body, rect(PAD + 12.0, y, field.right - PAD - 20.0, 40.0), pal.muted);
        } else if let Some(parts) = display_parts(&cfg.hotkey) {
            let mut x = PAD + 10.0;
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    t.text("+", &f.body, rect(x, y, 14.0, 40.0), pal.muted);
                    x += 14.0;
                }
                let w = gfx.text_width(part, &f.small) + 16.0;
                let r = rect(x, y + 10.0, w, 20.0);
                t.fill_round(r, 5.0, pal.keycap);
                t.text(part, &f.small_center, r, pal.text);
                x += w + 4.0;
            }
            if let Some(e) = self.hotkey_error {
                t.text(e, &f.small, rect(x + 6.0, y, field.right - x - 10.0, 40.0), pal.muted);
            }
        }
        let btn = rect(W - PAD - 82.0, y, 82.0, 40.0);
        t.stroke_round(btn, 8.0, pal.accent_ink, 1.0);
        t.text(if self.recording { "Cancel" } else { "Record" }, &f.small_center, btn, pal.accent_ink);
        self.hits.push((btn, Ctl::Record));
        y += 40.0 + 18.0;

        // Theme
        label(t, "Theme", y);
        y += 24.0;
        let seg = rect(PAD, y, W - PAD * 2.0, 40.0);
        t.fill_round_a(seg, 9.0, pal.text, 0.05);
        t.stroke_round_a(seg, 9.0, pal.text, 0.10, 1.0);
        let sw = (seg.right - seg.left - 8.0 - 8.0) / 3.0;
        for (i, (name, mode)) in [("Auto", ThemeMode::Auto), ("Light", ThemeMode::Light), ("Dark", ThemeMode::Dark)]
            .into_iter()
            .enumerate()
        {
            let r = rect(PAD + 4.0 + i as f32 * (sw + 4.0), y + 4.0, sw, 32.0);
            let on = cfg.theme == mode;
            if on {
                t.fill_round(r, 6.0, pal.accent);
            }
            t.text(name, &f.small_center, r, if on { pal.on_accent } else { pal.text });
            self.hits.push((r, Ctl::Theme(mode)));
        }
        y += 40.0 + 18.0;

        // Glass
        label(t, "Glass", y);
        let pct = format!("{}%", cfg.glass);
        let pw = gfx.text_width(&pct, &f.small);
        t.text(&pct, &f.small, rect(W - PAD - pw, y, pw + 2.0, 16.0), pal.muted);
        y += 22.0;
        let row = rect(PAD, y, W - PAD * 2.0, 28.0);
        // Knob travel stays inside the row.
        let track = rect(PAD + 8.0, y + 12.0, W - PAD * 2.0 - 16.0, 4.0);
        let kx = track.left + (track.right - track.left) * cfg.glass as f32 / 100.0;
        t.fill_round_a(track, 2.0, pal.text, 0.12);
        t.fill_round(rect(track.left, track.top, kx - track.left, 4.0), 2.0, pal.accent);
        t.circle(kx, y + 14.0, 8.0, pal.accent);
        t.circle(kx, y + 14.0, 3.0, pal.on_accent);
        self.track = track;
        self.hits.push((row, Ctl::Glass));
        y += 28.0 + 18.0;

        // Sources
        label(t, "Search & behavior", y);
        y += 22.0;
        let toggles = [
            ("Installed apps", cfg.search_apps),
            ("Files & folders", cfg.search_files),
            ("Windows settings", cfg.search_settings),
            ("Launch at startup", cfg.launch_at_startup),
            ("Always on top", cfg.always_on_top),
        ];
        for (i, (name, on)) in toggles.iter().enumerate() {
            t.text(name, &f.body, rect(PAD, y, 260.0, 36.0), pal.text);
            let sw = rect(W - PAD - 36.0, y + 8.0, 36.0, 20.0);
            t.fill_round(sw, 10.0, if *on { pal.accent } else { pal.toggle_off });
            let kx = if *on { sw.right - 10.0 } else { sw.left + 10.0 };
            t.circle(kx, y + 18.0, 8.0, if *on { pal.on_accent } else { pal.knob_off });
            self.hits.push((rect(PAD, y, W - PAD * 2.0, 36.0), Ctl::Toggle(i as u8)));
            y += 36.0;
        }
        y += 18.0;

        // Indexed drives
        label(t, "Indexed drives", y);
        y += 22.0;
        if self.drives.is_empty() {
            t.text("No fixed drives found", &f.small, rect(PAD, y, 300.0, 32.0), pal.muted);
            y += 38.0;
        }
        for (d, n) in self.drives.iter().take(4) {
            let r = rect(PAD, y, W - PAD * 2.0, 32.0);
            t.fill_round_a(r, 7.0, pal.text, 0.05);
            t.text(d, &f.small, rect(PAD + 10.0, y, 100.0, 32.0), pal.text);
            let s = match n {
                0 => "Whole drive".to_string(),
                1 => "1 folder".to_string(),
                n => format!("{n} folders"),
            };
            let w = gfx.text_width(&s, &f.small);
            t.text(&s, &f.small, rect(r.right - 10.0 - w, y, w + 2.0, 32.0), pal.muted);
            y += 38.0;
        }
        t.text("System folders (Windows, Program Files…) are skipped", &f.small, rect(PAD, y, W - PAD * 2.0, 16.0), pal.muted);

        // Footer
        let fy = h - 40.0;
        t.fill_a(rect(0.0, fy, W, 40.0), pal.text, 0.03);
        t.fill_a(rect(0.0, fy, W, 1.0), pal.text, 0.08);
        t.text(&format!("{indexed} indexed"), &f.small, rect(PAD, fy, 280.0, 40.0), pal.muted);
        let q = rect(W - PAD - 44.0, fy + 6.0, 44.0, 28.0);
        t.text("Quit", &f.small_center, q, pal.text);
        self.hits.push((q, Ctl::Quit));

        if !t.end() {
            self.target = None;
            self.invalidate();
        }
    }
}

fn drive_of(p: &Path) -> String {
    p.components()
        .next()
        .map(|c| format!("{}\\", c.as_os_str().to_string_lossy()))
        .unwrap_or_default()
}
