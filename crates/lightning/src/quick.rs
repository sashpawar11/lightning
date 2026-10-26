//! Quick settings toggles: Wi-Fi, Bluetooth, sound mute, dark mode.
//!
//! Radio and audio calls can block for tens of milliseconds, so they run on
//! one idle thread; the UI flips its tile at once and the thread posts the
//! real states back (no allocation: two bits per toggle in a WPARAM).

use std::sync::mpsc::{self, Receiver, Sender};

use windows::Devices::Radios::{Radio, RadioKind, RadioState};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, PostMessageW, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
};
use windows::core::{PCWSTR, w};

use crate::WM_APP_QUICK;
use crate::icon::{Glyph, glyph};

/// Per tile: `None` while unknown or when the device has no such radio.
/// Order: Wi-Fi, Bluetooth, sound, theme (`true` = dark).
pub type States = [Option<bool>; 4];
pub const THEME: usize = 3;

/// Tile `k` as (glyph, name, status line).
pub fn describe(k: usize, state: Option<bool>) -> (Glyph, &'static str, &'static str) {
    let on_off = match state {
        Some(true) => "On",
        Some(false) => "Off",
        None => "Unavailable",
    };
    match (k, state) {
        (0, _) => (glyph::WIFI, "Wi‑Fi", on_off),
        (1, _) => (glyph::BLUETOOTH, "Bluetooth", on_off),
        (2, Some(false)) => (glyph::MUTE, "Sound", "Muted"),
        (2, _) => (glyph::VOLUME, "Sound", on_off),
        (_, Some(false)) => (glyph::LIGHT, "Light mode", "Switch to dark"),
        (_, _) => (glyph::DARK, "Dark mode", "Switch to light"),
    }
}

pub enum Req {
    Refresh,
    Set(usize, bool),
}

pub fn spawn(hwnd: HWND) -> Sender<Req> {
    let (tx, rx) = mpsc::channel();
    let hwnd = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("quick".into())
        .stack_size(256 * 1024)
        .spawn(move || run(rx, HWND(hwnd as *mut _)))
        .expect("spawn quick thread");
    tx
}

fn run(rx: Receiver<Req>, hwnd: HWND) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    while let Ok(req) = rx.recv() {
        // Only the latest refresh matters; every set is applied in order.
        for r in std::iter::once(req).chain(rx.try_iter()) {
            if let Req::Set(i, on) = r {
                let _ = match i {
                    0 => set_radio(RadioKind::WiFi, on),
                    1 => set_radio(RadioKind::Bluetooth, on),
                    2 => set_sound(on),
                    _ => set_dark(on),
                };
            }
        }
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APP_QUICK, WPARAM(pack(&read())), LPARAM(0));
        }
    }
}

fn read() -> States {
    let (wifi, bt) = radios().unwrap_or((None, None));
    [wifi, bt, sound().ok(), Some(dark())]
}

pub fn pack(s: &States) -> usize {
    s.iter().enumerate().fold(0, |acc, (i, v)| acc | (v.map_or(0, |on| 2 | on as usize) << (i * 2)))
}

pub fn unpack(bits: usize) -> States {
    std::array::from_fn(|i| match (bits >> (i * 2)) & 3 {
        2 => Some(false),
        3 => Some(true),
        _ => None,
    })
}

fn find_radio(kind: RadioKind) -> windows::core::Result<Option<Radio>> {
    let all = Radio::GetRadiosAsync()?.join()?;
    for i in 0..all.Size()? {
        let r = all.GetAt(i)?;
        if r.Kind()? == kind {
            return Ok(Some(r));
        }
    }
    Ok(None)
}

fn radios() -> windows::core::Result<(Option<bool>, Option<bool>)> {
    let all = Radio::GetRadiosAsync()?.join()?;
    let (mut wifi, mut bt) = (None, None);
    for i in 0..all.Size()? {
        let r = all.GetAt(i)?;
        let on = Some(r.State()? == RadioState::On);
        match r.Kind()? {
            RadioKind::WiFi => wifi = wifi.or(on),
            RadioKind::Bluetooth => bt = bt.or(on),
            _ => {}
        }
    }
    Ok((wifi, bt))
}

fn set_radio(kind: RadioKind, on: bool) -> windows::core::Result<()> {
    let _ = Radio::RequestAccessAsync()?.join();
    if let Some(r) = find_radio(kind)? {
        r.SetStateAsync(if on { RadioState::On } else { RadioState::Off })?.join()?;
    }
    Ok(())
}

fn endpoint() -> windows::core::Result<IAudioEndpointVolume> {
    unsafe {
        let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        e.GetDefaultAudioEndpoint(eRender, eConsole)?.Activate(CLSCTX_ALL, None)
    }
}

/// Sound "on" means not muted.
fn sound() -> windows::core::Result<bool> {
    unsafe { Ok(!endpoint()?.GetMute()?.as_bool()) }
}

fn set_sound(on: bool) -> windows::core::Result<()> {
    unsafe { endpoint()?.SetMute(!on, std::ptr::null()) }
}

const PERSONALIZE: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");

fn dark() -> bool {
    let mut v = 1u32;
    let mut n = 4u32;
    unsafe {
        let _ = RegGetValueW(
            HKEY_CURRENT_USER,
            PERSONALIZE,
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut _ as *mut _),
            Some(&mut n),
        );
    }
    v == 0
}

fn set_dark(on: bool) -> windows::core::Result<()> {
    let light = u32::from(!on);
    unsafe {
        for name in [w!("AppsUseLightTheme"), w!("SystemUsesLightTheme")] {
            RegSetKeyValueW(HKEY_CURRENT_USER, PERSONALIZE, name, REG_DWORD.0, Some(&light as *const _ as *const _), 4)
                .ok()?;
        }
        // Apps (this one included) and the taskbar re-theme on this broadcast.
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("ImmersiveColorSet").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            1000,
            None,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_roundtrip() {
        let s: States = [Some(true), None, Some(false), Some(true)];
        assert_eq!(unpack(pack(&s)), s);
        assert_eq!(unpack(0), [None; 4]);
    }

    /// Reads real device state and reports the memory it costs:
    /// `cargo test -p lightning quick -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn reads_states() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let before = crate::platform::private_bytes();
        let s = read();
        let after = crate::platform::private_bytes();
        println!("{s:?}: +{:.2} MB private", (after as f64 - before as f64) / 1048576.0);
    }
}
