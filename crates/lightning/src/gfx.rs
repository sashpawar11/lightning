//! Thin Direct2D / DirectWrite layer. Everything is drawn in DIPs; the render
//! target's DPI maps DIPs to pixels.

use windows::Win32::Foundation::{E_INVALIDARG, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SRCCOPY,
    SelectObject,
};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{Interface, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::icon::{self, Glyph, Seg};
use crate::theme::Rgb;

pub fn col(rgb: Rgb) -> D2D1_COLOR_F {
    col_a(rgb, 1.0)
}

pub fn col_a(rgb: Rgb, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
        g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
        b: (rgb & 0xFF) as f32 / 255.0,
        a,
    }
}

/// Work Sans (SIL OFL 1.1, see assets/OFL.txt), compiled into the binary.
static WORK_SANS: &[u8] = include_bytes!("../assets/WorkSans.ttf");

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }
}

pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Converts a UTF-8 byte offset in `s` into a UTF-16 code unit offset.
pub fn byte_to_u16(s: &str, byte: usize) -> u32 {
    s[..byte.min(s.len())].encode_utf16().count() as u32
}

pub struct Fonts {
    pub input: IDWriteTextFormat,
    pub title: IDWriteTextFormat,
    pub body: IDWriteTextFormat,
    pub small: IDWriteTextFormat,
    pub small_center: IDWriteTextFormat,
    pub label: IDWriteTextFormat,
    pub heading: IDWriteTextFormat,
}

pub struct Gfx {
    pub d2d: ID2D1Factory,
    pub dw: IDWriteFactory,
    pub fonts: Fonts,
    /// One geometry per [`icon::PATHS`] entry, built once.
    icons: Vec<ID2D1PathGeometry>,
}

impl Gfx {
    pub fn new() -> Result<Self> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let ui = w!("Work Sans");
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            // Falls back to the system UI font if the embedded font can't load.
            let work_sans = load_embedded_font(&dw).ok();
            let fmt = |family: windows::core::PCWSTR,
                       size: f32,
                       weight: DWRITE_FONT_WEIGHT,
                       align: DWRITE_TEXT_ALIGNMENT|
             -> Result<IDWriteTextFormat> {
                let collection = if family == ui { work_sans.as_ref() } else { None };
                let f = dw.CreateTextFormat(
                    family,
                    collection,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-us"),
                )?;
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                f.SetTextAlignment(align)?;
                let trimming = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                let sign = dw.CreateEllipsisTrimmingSign(&f)?;
                f.SetTrimming(&trimming, &sign)?;
                Ok(f)
            };
            let lead = DWRITE_TEXT_ALIGNMENT_LEADING;
            let center = DWRITE_TEXT_ALIGNMENT_CENTER;
            let fonts = Fonts {
                input: fmt(ui, 20.0, DWRITE_FONT_WEIGHT_NORMAL, lead)?,
                title: fmt(ui, 14.0, DWRITE_FONT_WEIGHT_MEDIUM, lead)?,
                body: fmt(ui, 13.0, DWRITE_FONT_WEIGHT_NORMAL, lead)?,
                small: fmt(ui, 12.0, DWRITE_FONT_WEIGHT_NORMAL, lead)?,
                small_center: fmt(ui, 12.0, DWRITE_FONT_WEIGHT_NORMAL, center)?,
                label: fmt(ui, 11.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, lead)?,
                heading: fmt(ui, 14.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, lead)?,
            };
            let icons = icon::PATHS.iter().map(|d| icon_geometry(&d2d, d)).collect::<Result<_>>()?;
            Ok(Self { d2d, dw, fonts, icons })
        }
    }

    pub fn layout(&self, s: &str, fmt: &IDWriteTextFormat, w: f32, h: f32) -> Result<IDWriteTextLayout> {
        unsafe { self.dw.CreateTextLayout(&utf16(s), fmt, w, h) }
    }

    pub fn text_width(&self, s: &str, fmt: &IDWriteTextFormat) -> f32 {
        self.layout(s, fmt, 10_000.0, 100.0)
            .and_then(|l| unsafe {
                let mut m = DWRITE_TEXT_METRICS::default();
                l.GetMetrics(&mut m)?;
                Ok(m.widthIncludingTrailingWhitespace)
            })
            .unwrap_or(0.0)
    }
}

/// Builds an icon's path data (20-unit box) into a filled geometry.
fn icon_geometry(d2d: &ID2D1Factory, d: &str) -> Result<ID2D1PathGeometry> {
    let segs = icon::parse(d).ok_or_else(|| windows::core::Error::from(E_INVALIDARG))?;
    let pt = |(x, y): icon::Pt| Vector2 { X: x, Y: y };
    unsafe {
        let geo = d2d.CreatePathGeometry()?;
        let sink = geo.Open()?;
        // SVG's default fill rule is nonzero.
        sink.SetFillMode(D2D1_FILL_MODE_WINDING);
        let (mut open, mut start) = (false, (0.0, 0.0));
        for s in segs {
            if let Seg::Move(p) = s {
                if open {
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                }
                start = p;
                sink.BeginFigure(pt(p), D2D1_FIGURE_BEGIN_FILLED);
                open = true;
                continue;
            }
            if let Seg::Close = s {
                if open {
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    open = false;
                }
                continue;
            }
            // Drawing after a close continues from the subpath's start.
            if !open {
                sink.BeginFigure(pt(start), D2D1_FIGURE_BEGIN_FILLED);
                open = true;
            }
            match s {
                Seg::Line(p) => sink.AddLine(pt(p)),
                Seg::Cubic(a, b, e) => {
                    sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: pt(a), point2: pt(b), point3: pt(e) })
                }
                Seg::Quad(a, e) => sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT { point1: pt(a), point2: pt(e) }),
                Seg::Arc(e, (rx, ry), rot, large, sweep) => sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: pt(e),
                    size: D2D_SIZE_F { width: rx, height: ry },
                    rotationAngle: rot,
                    sweepDirection: if sweep { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
                    arcSize: if large { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                }),
                Seg::Move(_) | Seg::Close => unreachable!(),
            }
        }
        if open {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close()?;
        Ok(geo)
    }
}

/// Registers the embedded Work Sans with DirectWrite without copying it: the
/// bytes stay in the executable image.
unsafe fn load_embedded_font(dw: &IDWriteFactory) -> Result<IDWriteFontCollection> {
    unsafe {
        let dw5: IDWriteFactory5 = dw.cast()?;
        let loader = dw5.CreateInMemoryFontFileLoader()?;
        dw5.RegisterFontFileLoader(&loader)?;
        // Any owner object skips the copy; the data is 'static anyway.
        let file = loader.CreateInMemoryFontFileReference(
            dw,
            WORK_SANS.as_ptr() as *const _,
            WORK_SANS.len() as u32,
            Some(&dw.cast::<windows::core::IUnknown>()?),
        )?;
        let builder: IDWriteFontSetBuilder1 = dw5.CreateFontSetBuilder()?.cast()?;
        builder.AddFontFile(&file)?;
        let set = builder.CreateFontSet()?;
        dw5.CreateFontCollectionFromFontSet(&set)?.cast()
    }
}

/// Software Direct2D drawing into a premultiplied 32-bit DIB that is then
/// blitted to the window. Unlike a software HWND target, this keeps the
/// alpha channel, which DWM needs to show the acrylic backdrop through the
/// panel. No GPU device is created in this process.
pub struct Target {
    pub rt: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    hwnd: HWND,
    dc: HDC,
    dib: HBITMAP,
    old: HGDIOBJ,
    size: (i32, i32),
}

impl Target {
    pub fn new(gfx: &Gfx, hwnd: HWND, dpi: f32) -> Result<Self> {
        unsafe {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: dpi,
                dpiY: dpi,
                ..Default::default()
            };
            let rt = gfx.d2d.CreateDCRenderTarget(&props)?;
            // Grayscale: ClearType needs an opaque backdrop, the panel is glass.
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let brush = rt.CreateSolidColorBrush(&col(0), None)?;
            let dc = CreateCompatibleDC(None);
            let mut t = Self { rt, brush, hwnd, dc, dib: HBITMAP::default(), old: HGDIOBJ::default(), size: (0, 0) };
            let mut rc = RECT::default();
            GetClientRect(hwnd, &mut rc)?;
            t.resize((rc.right - rc.left) as u32, (rc.bottom - rc.top) as u32, dpi);
            Ok(t)
        }
    }

    pub fn resize(&mut self, w: u32, h: u32, dpi: f32) {
        let (w, h) = (w.max(1) as i32, h.max(1) as i32);
        unsafe {
            self.rt.SetDpi(dpi, dpi);
            if (w, h) == self.size {
                return;
            }
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let Ok(dib) = CreateDIBSection(Some(self.dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else { return };
            let prev = SelectObject(self.dc, HGDIOBJ(dib.0));
            if self.dib.is_invalid() {
                self.old = prev;
            } else {
                let _ = DeleteObject(HGDIOBJ(self.dib.0));
            }
            self.dib = dib;
            self.size = (w, h);
        }
    }
    fn b(&self, c: Rgb) -> &ID2D1Brush {
        self.ba(c, 1.0)
    }

    fn ba(&self, c: Rgb, a: f32) -> &ID2D1Brush {
        unsafe { self.brush.SetColor(&col_a(c, a)) };
        &self.brush
    }

    /// Clears to a translucent tint; the window's acrylic shows through.
    pub fn clear_a(&self, c: Rgb, a: f32) {
        let mut k = col_a(c, a);
        // Premultiplied target.
        (k.r, k.g, k.b) = (k.r * a, k.g * a, k.b * a);
        unsafe { self.rt.Clear(Some(&k)) }
    }

    pub fn fill(&self, r: D2D_RECT_F, c: Rgb) {
        self.fill_a(r, c, 1.0)
    }

    pub fn fill_a(&self, r: D2D_RECT_F, c: Rgb, a: f32) {
        unsafe { self.rt.FillRectangle(&r, self.ba(c, a)) }
    }

    pub fn fill_round(&self, r: D2D_RECT_F, radius: f32, c: Rgb) {
        self.fill_round_a(r, radius, c, 1.0)
    }

    pub fn fill_round_a(&self, r: D2D_RECT_F, radius: f32, c: Rgb, a: f32) {
        let rr = D2D1_ROUNDED_RECT { rect: r, radiusX: radius, radiusY: radius };
        unsafe { self.rt.FillRoundedRectangle(&rr, self.ba(c, a)) }
    }

    pub fn stroke_round(&self, r: D2D_RECT_F, radius: f32, c: Rgb, width: f32) {
        self.stroke_round_a(r, radius, c, 1.0, width)
    }

    pub fn stroke_round_a(&self, r: D2D_RECT_F, radius: f32, c: Rgb, a: f32, width: f32) {
        // Inset by half the stroke so the line stays inside `r`.
        let h = width / 2.0;
        let r = D2D_RECT_F { left: r.left + h, top: r.top + h, right: r.right - h, bottom: r.bottom - h };
        let rr = D2D1_ROUNDED_RECT { rect: r, radiusX: radius, radiusY: radius };
        unsafe { self.rt.DrawRoundedRectangle(&rr, self.ba(c, a), width, None) }
    }

    pub fn circle(&self, cx: f32, cy: f32, r: f32, c: Rgb) {
        let e = D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r };
        unsafe { self.rt.FillEllipse(&e, self.b(c)) }
    }

    pub fn text(&self, s: &str, fmt: &IDWriteTextFormat, r: D2D_RECT_F, c: Rgb) {
        if s.is_empty() {
            return;
        }
        unsafe {
            self.rt.DrawText(
                &utf16(s),
                fmt,
                &r,
                self.b(c),
                D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        }
    }

    /// Draws icon `g` with its 20-unit box scaled to `size` DIPs, centered in `r`.
    pub fn icon(&self, gfx: &Gfx, g: Glyph, size: f32, r: D2D_RECT_F, c: Rgb) {
        let Some(geo) = gfx.icons.get(g.0) else { return };
        let s = size / 20.0;
        let (x, y) = ((r.left + r.right - size) / 2.0, (r.top + r.bottom - size) / 2.0);
        let m = Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: x, M32: y };
        unsafe {
            self.rt.SetTransform(&m);
            self.rt.FillGeometry(geo, self.b(c), None);
            self.rt.SetTransform(&Matrix3x2::identity());
        }
    }

    /// Text with byte `ranges` marked by a soft tint behind the glyphs.
    pub fn text_hl(
        &self,
        gfx: &Gfx,
        s: &str,
        ranges: &[(u32, u32)],
        fmt: &IDWriteTextFormat,
        r: D2D_RECT_F,
        c: Rgb,
        (tint, tint_a): (Rgb, f32),
    ) {
        let Ok(layout) = gfx.layout(s, fmt, r.right - r.left, r.bottom - r.top) else { return };
        unsafe {
            let mut m = [DWRITE_HIT_TEST_METRICS::default(); 4];
            for &(a, b) in ranges {
                let start = byte_to_u16(s, a as usize);
                let len = byte_to_u16(s, b as usize) - start;
                let mut n = 0u32;
                if layout.HitTestTextRange(start, len, r.left, r.top, Some(&mut m), &mut n).is_ok() {
                    for h in &m[..(n as usize).min(4)] {
                        let box_ = rect(h.left - 1.0, h.top + 1.0, h.width + 2.0, h.height - 2.0);
                        self.fill_round_a(box_, 3.0, tint, tint_a);
                    }
                }
            }
            self.rt.DrawTextLayout(
                Vector2 { X: r.left, Y: r.top },
                &layout,
                self.b(c),
                D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            );
        }
    }

    pub fn bitmap(&self, bmp: &ID2D1Bitmap, r: D2D_RECT_F) {
        unsafe {
            self.rt.DrawBitmap(bmp, Some(&r), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None)
        }
    }

    pub fn create_bitmap(&self, w: u32, h: u32, bgra_premul: &[u8]) -> Result<ID2D1Bitmap> {
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        unsafe {
            self.rt.CreateBitmap(
                D2D_SIZE_U { width: w, height: h },
                Some(bgra_premul.as_ptr() as *const _),
                w * 4,
                &props,
            )
        }
    }

    pub fn clip(&self, r: D2D_RECT_F) {
        unsafe { self.rt.PushAxisAlignedClip(&r, D2D1_ANTIALIAS_MODE_ALIASED) }
    }

    pub fn unclip(&self) {
        unsafe { self.rt.PopAxisAlignedClip() }
    }

    pub fn begin(&self) {
        let rc = RECT { left: 0, top: 0, right: self.size.0, bottom: self.size.1 };
        unsafe {
            let _ = self.rt.BindDC(self.dc, &rc);
            self.rt.BeginDraw()
        }
    }

    /// Finishes the frame and copies it to the window. Returns false when
    /// the target must be recreated.
    pub fn end(&self) -> bool {
        unsafe {
            if self.rt.EndDraw(None, None).is_err() {
                return false;
            }
            let hdc = GetDC(Some(self.hwnd));
            let _ = BitBlt(hdc, 0, 0, self.size.0, self.size.1, Some(self.dc), 0, 0, SRCCOPY);
            ReleaseDC(Some(self.hwnd), hdc);
        }
        true
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(HGDIOBJ(self.dib.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

/// Caret x offset (DIPs) for a UTF-16 position within `s`.
pub fn caret_x(gfx: &Gfx, s: &str, fmt: &IDWriteTextFormat, pos16: u32) -> f32 {
    let Ok(l) = gfx.layout(s, fmt, 10_000.0, 100.0) else { return 0.0 };
    let (mut x, mut y) = (0f32, 0f32);
    let mut m = DWRITE_HIT_TEST_METRICS::default();
    unsafe {
        let _ = l.HitTestTextPosition(pos16, false, &mut x, &mut y, &mut m);
    }
    x
}
