//! Global hotkey parsing ("Ctrl+Shift+K") and registration.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey,
};

pub const HOTKEY_ID: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub mods: HOT_KEY_MODIFIERS,
    pub vk: u32,
}

const NAMED: &[(&str, u32)] = &[
    ("Space", 0x20),
    ("Enter", 0x0D),
    ("Tab", 0x09),
    ("Backquote", 0xC0),
    ("Semicolon", 0xBA),
    ("Period", 0xBE),
    ("Comma", 0xBC),
    ("Slash", 0xBF),
    ("Insert", 0x2D),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
];

impl Hotkey {
    pub fn parse(s: &str) -> Option<Self> {
        let mut mods = HOT_KEY_MODIFIERS(0);
        let mut vk = None;
        for part in s.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods |= MOD_CONTROL,
                "alt" => mods |= MOD_ALT,
                "shift" => mods |= MOD_SHIFT,
                "win" | "super" => mods |= MOD_WIN,
                _ => vk = Some(key_from_name(part)?),
            }
        }
        if mods.0 == 0 {
            return None;
        }
        Some(Self { mods, vk: vk? })
    }

    pub fn is_modifier_vk(vk: u32) -> bool {
        matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
    }

    pub fn register(&self, hwnd: HWND) -> bool {
        unsafe { RegisterHotKey(Some(hwnd), HOTKEY_ID, self.mods | MOD_NOREPEAT, self.vk).is_ok() }
    }

    pub fn unregister(hwnd: HWND) {
        unsafe {
            let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
        }
    }

    /// Display parts, e.g. `["Alt", "Space"]`.
    pub fn parts(&self) -> Vec<String> {
        let mut v = Vec::new();
        for (m, n) in [(MOD_CONTROL, "Ctrl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift"), (MOD_WIN, "Win")] {
            if self.mods.0 & m.0 != 0 {
                v.push(n.to_string());
            }
        }
        v.push(key_name(self.vk));
        v
    }
}

impl std::fmt::Display for Hotkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.parts().join("+"))
    }
}

fn key_from_name(name: &str) -> Option<u32> {
    if let Some(&(_, vk)) = NAMED.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Some(vk);
    }
    let up = name.to_ascii_uppercase();
    let b = up.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_uppercase() || b[0].is_ascii_digit()) {
        return Some(b[0] as u32);
    }
    if let Some(n) = up.strip_prefix('F').and_then(|n| n.parse::<u32>().ok())
        && (1..=24).contains(&n)
    {
        return Some(0x70 + n - 1);
    }
    None
}

pub fn key_name(vk: u32) -> String {
    if let Some((n, _)) = NAMED.iter().find(|(_, v)| *v == vk) {
        return n.to_string();
    }
    match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        _ => format!("0x{vk:02X}"),
    }
}

/// Like `key_name` but `None` for keys we can't round-trip through config.
pub fn bindable_key(vk: u32) -> Option<u32> {
    key_from_name(&key_name(vk)).filter(|&v| v == vk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for s in ["Alt+Space", "Ctrl+Shift+K", "Win+F5", "Ctrl+Alt+1"] {
            assert_eq!(Hotkey::parse(s).unwrap().to_string(), s);
        }
    }

    #[test]
    fn rejects_bare_keys() {
        assert!(Hotkey::parse("Space").is_none());
        assert!(Hotkey::parse("Alt+Nope").is_none());
    }
}
