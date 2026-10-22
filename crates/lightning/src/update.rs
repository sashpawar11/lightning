//! Updates from GitHub Releases. Each release carries a `latest.json`
//! (`{"version": "0.4.0", "url": "https://.../Lightning-Setup-0.4.0.exe"}`);
//! `releases/latest/download/` redirects to the newest one without touching
//! the rate-limited GitHub API.
//!
//! Downloads go through Windows' own curl.exe, so no HTTP or TLS code is
//! linked or loaded into this process. The installer closes Lightning,
//! updates it and starts it again.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

const MANIFEST: &str = "https://github.com/sashpawar11/lightning/releases/latest/download/latest.json";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Deserialize)]
pub struct Update {
    pub version: String,
    url: String,
}

/// The newer release, if there is one. Blocks while curl runs (a second or
/// two; at most 2 min on a stalled connection).
pub fn check() -> Result<Option<Update>, String> {
    // Debug builds can point at a test server.
    let manifest = std::env::var("LIGHTNING_UPDATE_MANIFEST").ok().filter(|_| cfg!(debug_assertions));
    let out = curl(&[manifest.as_deref().unwrap_or(MANIFEST)])?;
    let u: Update = serde_json::from_slice(&out).map_err(|e| format!("bad latest.json: {e}"))?;
    Ok((parse(&u.version) > parse(env!("CARGO_PKG_VERSION"))).then_some(u))
}

/// Downloads the installer and starts it silently. Lightning is closed by
/// the installer, which starts the new version when it's done.
pub fn install(u: &Update) -> Result<(), String> {
    let dir = crate::config::data_dir().join("update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let setup = dir.join("Lightning-Setup.exe");
    let _ = std::fs::remove_file(&setup);
    curl(&["-o", &setup.to_string_lossy(), &u.url])?;
    Command::new(&setup)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not start the installer: {e}"))
}

/// Automatic checks only run for the installed copy, never a dev build.
pub fn is_installed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.ends_with("Programs\\Lightning")))
        .unwrap_or(false)
}

fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let exe = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from("curl.exe"), |r| {
        PathBuf::from(r).join("System32").join("curl.exe")
    });
    let out = Command::new(exe)
        .args(["-fsSL", "--max-time", "120", "--connect-timeout", "15"])
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("curl.exe unavailable: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("download failed: {}", err.trim()));
    }
    Ok(out.stdout)
}

fn parse(v: &str) -> (u32, u32, u32) {
    let mut n = v.trim_start_matches('v').split(['.', '-']).map(|p| p.parse().unwrap_or(0));
    (n.next().unwrap_or(0), n.next().unwrap_or(0), n.next().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn versions_compare_numerically() {
        assert!(parse("0.10.0") > parse("0.9.3"));
        assert!(parse("v1.0.0") > parse("0.99.99"));
        assert_eq!(parse("0.3.0"), (0, 3, 0));
    }
}
