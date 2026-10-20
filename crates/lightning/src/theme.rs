//! Colors from the design canvas, plus system light/dark detection.

use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::w;

use crate::config::ThemeMode;

/// 0xRRGGBB
pub type Rgb = u32;

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Opacity of the panel tint over the acrylic backdrop.
    pub glass: f32,
    /// Opacity of the accent tint behind matched characters.
    pub match_tint: f32,
    pub panel: Rgb,
    pub border: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    pub placeholder: Rgb,
    pub keycap: Rgb,
    /// Bright fill: top bar, selected tile, badges.
    pub accent: Rgb,
    /// Accent used as text/strokes on `panel` (match highlights, bolt).
    pub accent_ink: Rgb,
    pub on_accent: Rgb,
    pub toggle_off: Rgb,
    pub knob_off: Rgb,
}

pub const DARK: Palette = Palette {
    dark: true,
    glass: 0.55,
    match_tint: 0.22,
    panel: 0x141518,
    border: 0x2A2C31,
    text: 0xECEDEE,
    muted: 0x8B8F98,
    placeholder: 0x7D818A,
    keycap: 0x2E3036,
    accent: 0xD4FF3A,
    accent_ink: 0xD4FF3A,
    on_accent: 0x141518,
    toggle_off: 0x34363C,
    knob_off: 0x9A9EA6,
};

pub const LIGHT: Palette = Palette {
    dark: false,
    glass: 0.72,
    match_tint: 0.60,
    panel: 0xFFFFFF,
    border: 0xDAD7CF,
    text: 0x17181A,
    muted: 0x5B5E66,
    placeholder: 0x7A7D84,
    keycap: 0xE4E2DB,
    accent: 0xD4FF3A,
    accent_ink: 0x3F4D00,
    on_accent: 0x17181A,
    toggle_off: 0xDAD7CF,
    knob_off: 0xFFFFFF,
};

pub fn system_prefers_dark() -> bool {
    let mut value: u32 = 1;
    let mut size = 4u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    ok.is_ok() && value == 0
}

/// The theme's palette with its panel tint set from the Glass slider.
pub fn effective(mode: ThemeMode, glass: u8) -> Palette {
    let mut p = *palette(mode);
    p.glass = glass_tint(p.glass, glass);
    p
}

/// Slider 0..=100 -> panel tint opacity. 50 keeps the theme's tuned tint;
/// 0 is fully opaque, 100 is a light 30% tint over the blur.
fn glass_tint(base: f32, slider: u8) -> f32 {
    let s = slider.min(100) as f32 / 50.0;
    if s <= 1.0 { 1.0 + (base - 1.0) * s } else { base + (0.30 - base) * (s - 1.0) }
}

#[cfg(test)]
mod tests {
    use super::glass_tint;

    #[test]
    fn slider_maps_around_the_tuned_default() {
        assert_eq!(glass_tint(0.55, 0), 1.0);
        assert!((glass_tint(0.55, 50) - 0.55).abs() < 1e-6);
        assert!((glass_tint(0.55, 100) - 0.30).abs() < 1e-6);
        assert!(glass_tint(0.55, 25) > glass_tint(0.55, 75));
    }
}

fn palette(mode: ThemeMode) -> &'static Palette {
    let dark = match mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::Auto => system_prefers_dark(),
    };
    if dark { &DARK } else { &LIGHT }
}
