//! Embeds the app icon and version info into lightning.exe. The .ico is
//! rasterized from the same bolt artwork as the tray icon (src/bolt.rs) and
//! also left in OUT_DIR for the installer (scripts/release.ps1 picks it up).

#[path = "src/bolt.rs"]
mod bolt;

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/bolt.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::env::var("OUT_DIR").unwrap();
    let ico = Path::new(&out).join("lightning.ico");
    std::fs::write(&ico, ico_bytes(&[16, 20, 24, 32, 40, 48, 64, 128, 256])).unwrap();

    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<u32> = version.split(['.', '-']).filter_map(|p| p.parse().ok()).collect();
    parts.resize(4, 0);
    let commas = parts.iter().take(4).map(u32::to_string).collect::<Vec<_>>().join(",");
    let rc = format!(
        r#"1 ICON "{ico}"
1 VERSIONINFO
FILEVERSION {commas}
PRODUCTVERSION {commas}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "Lightning launcher"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "lightning"
      VALUE "OriginalFilename", "lightning.exe"
      VALUE "ProductName", "Lightning"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = ico.display().to_string().replace('\\', "\\\\"),
    );
    let rc_path = Path::new(&out).join("app.rc");
    std::fs::write(&rc_path, rc).unwrap();
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional().unwrap();
}

/// A multi-size .ico with 32-bit BMP entries.
fn ico_bytes(sizes: &[usize]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| bmp_entry(s)).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (&s, img) in sizes.iter().zip(&images) {
        let dim = if s >= 256 { 0 } else { s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += img.len();
    }
    for img in images {
        out.extend_from_slice(&img);
    }
    out
}

fn bmp_entry(s: usize) -> Vec<u8> {
    let px = bolt::rasterize(s); // top-down BGRA
    let mask_row = s.div_ceil(32) * 4;
    let mut out = Vec::with_capacity(40 + px.len() + mask_row * s);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(s as i32).to_le_bytes());
    out.extend_from_slice(&(2 * s as i32).to_le_bytes()); // colour + mask
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]); // compression, sizes, resolution, palette
    for row in px.chunks_exact(s * 4).rev() {
        out.extend_from_slice(row); // bottom-up
    }
    out.extend(std::iter::repeat_n(0u8, mask_row * s)); // alpha decides; mask all-opaque
    out
}
