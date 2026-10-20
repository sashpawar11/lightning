//! The Lightning mark: a lime bolt on a graphite rounded tile. Shared by the
//! app (tray icon) and build.rs (the embedded .ico), so both stay identical.

/// BGRA (straight alpha), top-down, `s`×`s`.
pub fn rasterize(s: usize) -> Vec<u8> {
    // Bolt outline in a 24-unit box (same path as the designs).
    const BOLT: [(f32, f32); 6] = [(13., 2.), (4., 14.), (11., 14.), (10., 22.), (19., 10.), (12., 10.)];
    const TILE: [u8; 3] = [0x18, 0x15, 0x14]; // #141518 as BGR
    const LIME: [u8; 3] = [0x3A, 0xFF, 0xD4]; // #D4FF3A as BGR
    let n = 4; // supersampling per axis
    let fs = s as f32;
    let radius = fs * 0.22;
    let mut out = vec![0u8; s * s * 4];
    for y in 0..s {
        for x in 0..s {
            let (mut tile, mut bolt) = (0u32, 0u32);
            for sy in 0..n {
                for sx in 0..n {
                    let px = x as f32 + (sx as f32 + 0.5) / n as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / n as f32;
                    if !in_round_rect(px, py, fs, radius) {
                        continue;
                    }
                    // Map the 24-unit bolt into the tile with a small inset.
                    let u = (px / fs) * 26.0 - 1.0;
                    let v = (py / fs) * 26.0 - 1.0;
                    if in_poly(u, v, &BOLT) { bolt += 1 } else { tile += 1 }
                }
            }
            let total = (n * n) as u32;
            let cov = tile + bolt;
            if cov == 0 {
                continue;
            }
            let i = (y * s + x) * 4;
            for c in 0..3 {
                out[i + c] = ((TILE[c] as u32 * tile + LIME[c] as u32 * bolt) / cov) as u8;
            }
            out[i + 3] = (cov * 255 / total) as u8;
        }
    }
    out
}

fn in_round_rect(x: f32, y: f32, s: f32, r: f32) -> bool {
    let cx = x.clamp(r, s - r);
    let cy = y.clamp(r, s - r);
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

fn in_poly(x: f32, y: f32, pts: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let (xi, yi) = pts[i];
        let (xj, yj) = pts[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}
