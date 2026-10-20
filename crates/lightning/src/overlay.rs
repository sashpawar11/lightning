//! The search overlay: input editing, result list, painting.

use std::collections::HashMap;
use std::sync::Arc;

use lightning_core::{Hit, Kind};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};

use crate::gfx::{Gfx, Target, caret_x, glyph, rect};
use crate::theme::Palette;
use crate::worker::{Icon, Resp};
use crate::{Fx, platform};

pub const W: f32 = 680.0;
const HEADER: f32 = 64.0;
const LIST_Y: f32 = 65.0;
const LIST_H: f32 = 372.0;
const FOOT_H: f32 = 36.0;
pub const H: f32 = LIST_Y + LIST_H + FOOT_H;
const ROW_H: f32 = 48.0;
const ROW_STEP: f32 = 50.0;
const TILE_H: f32 = 64.0;

pub const QUICK_SETTINGS: [(&str, &str, &str); 4] = [
    ("Wi‑Fi", glyph::WIFI, "ms-settings:network-wifi"),
    ("Display", glyph::DISPLAY, "ms-settings:display"),
    ("Sound", glyph::VOLUME, "ms-settings:sound"),
    ("Bluetooth", glyph::BLUETOOTH, "ms-settings:bluetooth"),
];

#[derive(Clone, Copy)]
enum Item {
    Label(&'static str),
    Row(usize),
    Tiles,
    Empty,
}

pub struct Overlay {
    pub hwnd: HWND,
    pub target: Option<Target>,
    pub dpi: f32,
    pub visible: bool,
    query: String,
    caret: usize, // byte offset
    select_all: bool,
    high_surrogate: Option<u16>,
    pub generation: u64,
    hits: Vec<Hit>,
    icons: Vec<Option<Arc<Icon>>>,
    bitmaps: HashMap<usize, ID2D1Bitmap>,
    results_query: Option<String>,
    sel: usize,
    scroll: f32,
    pub elapsed_us: u64,
    pub indexed: usize,
    pub scanning: bool,
    pub caret_on: bool,
    last_mouse: (i32, i32),
}

impl Overlay {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            target: None,
            dpi: 96.0,
            visible: false,
            query: String::new(),
            caret: 0,
            select_all: false,
            high_surrogate: None,
            generation: 0,
            hits: Vec::new(),
            icons: Vec::new(),
            bitmaps: HashMap::new(),
            results_query: None,
            sel: 0,
            scroll: 0.0,
            elapsed_us: 0,
            indexed: 0,
            scanning: true,
            caret_on: true,
            last_mouse: (i32::MIN, i32::MIN),
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    pub fn on_show(&mut self) {
        self.select_all = !self.query.is_empty();
        self.caret = self.query.len();
        self.caret_on = true;
        self.last_mouse = (i32::MIN, i32::MIN);
    }

    pub fn drop_target(&mut self) {
        self.target = None;
        self.bitmaps.clear();
    }

    pub fn on_results(&mut self, resp: Box<Resp>) {
        self.indexed = resp.file_count;
        self.scanning = resp.scanning;
        if resp.generation != self.generation {
            return;
        }
        let same_query = self.results_query.as_deref() == Some(resp.results.query.as_str());
        self.elapsed_us = resp.results.elapsed_us;
        self.hits = resp.results.hits;
        self.icons = resp.icons;
        self.results_query = Some(resp.results.query);
        if same_query {
            self.sel = self.sel.min(self.hits.len().saturating_sub(1));
        } else {
            self.sel = 0;
            self.scroll = 0.0;
        }
        // Drop bitmaps for icons no longer referenced.
        let live: Vec<usize> = self.icons.iter().flatten().map(|i| Arc::as_ptr(i) as usize).collect();
        self.bitmaps.retain(|k, _| live.contains(k));
        self.invalidate();
    }

    // ---------------------------------------------------------------- input

    fn edited(&mut self, fx: &mut Vec<Fx>) {
        self.select_all = false;
        self.caret_on = true;
        fx.push(Fx::Search);
        self.invalidate();
    }

    fn insert(&mut self, s: &str, fx: &mut Vec<Fx>) {
        if self.select_all {
            self.query.clear();
            self.caret = 0;
        }
        self.query.insert_str(self.caret, s);
        self.caret += s.len();
        self.edited(fx);
    }

    pub fn on_char(&mut self, unit: u16, fx: &mut Vec<Fx>) {
        if ctrl_down() {
            return; // Ctrl+letter shortcuts are handled in on_key.
        }
        if (0xD800..0xDC00).contains(&unit) {
            self.high_surrogate = Some(unit);
            return;
        }
        let units: Vec<u16> = match self.high_surrogate.take() {
            Some(hi) if (0xDC00..0xE000).contains(&unit) => vec![hi, unit],
            _ => vec![unit],
        };
        let s = String::from_utf16_lossy(&units);
        if s.chars().all(|c| !c.is_control()) {
            self.insert(&s, fx);
        }
    }

    pub fn on_key(&mut self, vk: u16, fx: &mut Vec<Fx>) -> bool {
        let ctrl = ctrl_down();
        match vk {
            // One press always closes; the query is kept and pre-selected
            // on the next show, so typing replaces it.
            0x1B => fx.push(Fx::HideOverlay),
            0x28 => self.move_sel(1),
            0x26 => self.move_sel(-1),
            0x22 => self.move_sel(5),
            0x21 => self.move_sel(-5),
            0x0D => self.launch(self.sel, ctrl, fx),
            0x31..=0x39 if ctrl => self.launch((vk - 0x31) as usize, false, fx),
            0x25 => {
                self.caret = if self.select_all || ctrl { 0 } else { prev_boundary(&self.query, self.caret) };
                self.after_caret_move();
            }
            0x27 => {
                self.caret = if self.select_all || ctrl {
                    self.query.len()
                } else {
                    next_boundary(&self.query, self.caret)
                };
                self.after_caret_move();
            }
            0x24 => {
                self.caret = 0;
                self.after_caret_move();
            }
            0x23 => {
                self.caret = self.query.len();
                self.after_caret_move();
            }
            0x08 => {
                if self.select_all {
                    self.query.clear();
                    self.caret = 0;
                } else if self.caret > 0 {
                    let start = if ctrl { word_start(&self.query, self.caret) } else { prev_boundary(&self.query, self.caret) };
                    self.query.replace_range(start..self.caret, "");
                    self.caret = start;
                } else {
                    return true;
                }
                self.edited(fx);
            }
            0x2E => {
                if self.select_all {
                    self.query.clear();
                    self.caret = 0;
                } else if self.caret < self.query.len() {
                    let end = next_boundary(&self.query, self.caret);
                    self.query.replace_range(self.caret..end, "");
                } else {
                    return true;
                }
                self.edited(fx);
            }
            0x41 if ctrl => {
                self.select_all = !self.query.is_empty();
                self.invalidate();
            }
            0x56 if ctrl => {
                if let Some(s) = platform::paste_text(self.hwnd) {
                    let s: String = s.chars().filter(|c| !c.is_control()).collect();
                    if !s.is_empty() {
                        self.insert(&s, fx);
                    }
                }
            }
            0x43 if ctrl => {
                if self.select_all {
                    platform::copy_text(self.hwnd, &self.query);
                } else if let Some(h) = self.hits.get(self.sel) {
                    platform::copy_text(self.hwnd, &h.target);
                }
            }
            0xBC if ctrl => fx.push(Fx::ShowSettings),
            _ => return false,
        }
        true
    }

    fn after_caret_move(&mut self) {
        self.select_all = false;
        self.caret_on = true;
        self.invalidate();
    }

    fn move_sel(&mut self, d: isize) {
        if self.hits.is_empty() {
            return;
        }
        let n = self.hits.len() as isize;
        self.sel = (self.sel as isize + d).clamp(0, n - 1) as usize;
        self.ensure_visible();
        self.invalidate();
    }

    fn ensure_visible(&mut self) {
        for (y, h, item) in self.layout() {
            if let Item::Row(i) = item
                && i == self.sel
            {
                // Keep the group label visible when selecting a group's first row.
                let top = if i == 0 { 0.0 } else { y - 4.0 };
                if top < self.scroll {
                    self.scroll = top;
                } else if y + h + 8.0 > self.scroll + LIST_H {
                    self.scroll = y + h + 8.0 - LIST_H;
                }
            }
        }
    }

    fn launch(&mut self, i: usize, reveal: bool, fx: &mut Vec<Fx>) {
        if let Some(hit) = self.hits.get(i) {
            fx.push(Fx::Launch { hit: hit.clone(), reveal });
        }
    }

    // ---------------------------------------------------------------- mouse

    fn item_at(&self, y_dip: f32) -> Option<Item> {
        if !(LIST_Y..LIST_Y + LIST_H).contains(&y_dip) {
            return None;
        }
        let cy = y_dip - LIST_Y + self.scroll;
        self.layout()
            .into_iter()
            .find(|&(y, h, _)| cy >= y && cy < y + h)
            .map(|(_, _, it)| it)
    }

    pub fn on_mouse_move(&mut self, x: i32, y: i32) {
        if (x, y) == self.last_mouse {
            return;
        }
        let first = self.last_mouse.0 == i32::MIN;
        self.last_mouse = (x, y);
        if first {
            return; // The window appearing under a still cursor isn't a hover.
        }
        if let Some(Item::Row(i)) = self.item_at(y as f32 * 96.0 / self.dpi)
            && i != self.sel
        {
            self.sel = i;
            self.invalidate();
        }
    }

    pub fn on_click(&mut self, x: i32, y: i32, fx: &mut Vec<Fx>) {
        let (x, y) = (x as f32 * 96.0 / self.dpi, y as f32 * 96.0 / self.dpi);
        match self.item_at(y) {
            Some(Item::Row(i)) => self.launch(i, ctrl_down(), fx),
            Some(Item::Tiles) => {
                let col = ((x - 20.0) / ((W - 40.0) / 4.0)).floor();
                if (0.0..4.0).contains(&col) {
                    fx.push(Fx::Open(QUICK_SETTINGS[col as usize].2.into()));
                }
            }
            _ => {}
        }
    }

    pub fn on_wheel(&mut self, delta: i16) {
        let max = (self.content_height() - LIST_H).max(0.0);
        self.scroll = (self.scroll - delta as f32 / 120.0 * ROW_STEP).clamp(0.0, max);
        self.invalidate();
    }

    // --------------------------------------------------------------- layout

    /// `(y, height, item)` in list-content coordinates.
    fn layout(&self) -> Vec<(f32, f32, Item)> {
        let mut out = Vec::with_capacity(self.hits.len() + 6);
        let mut y = 6.0;
        let idle = self.query.trim().is_empty();
        let mut prev: Option<Kind> = None;
        for (i, h) in self.hits.iter().enumerate() {
            let label = if idle {
                (i == 0).then_some("Recent")
            } else if prev != Some(h.kind) {
                Some(match h.kind {
                    Kind::App => "Applications",
                    Kind::File => "Files",
                    Kind::Setting => "Settings",
                })
            } else {
                None
            };
            if let Some(l) = label {
                let lh = if out.is_empty() { 26.0 } else { 34.0 };
                out.push((y, lh, Item::Label(l)));
                y += lh;
            }
            out.push((y, ROW_STEP, Item::Row(i)));
            y += ROW_STEP;
            prev = Some(h.kind);
        }
        if idle {
            let lh = if out.is_empty() { 26.0 } else { 34.0 };
            out.push((y, lh, Item::Label("Quick settings")));
            y += lh;
            out.push((y, TILE_H, Item::Tiles));
        } else if self.hits.is_empty() && self.results_query.as_deref() == Some(self.query.as_str()) {
            out.push((y, 120.0, Item::Empty));
        }
        out
    }

    fn content_height(&self) -> f32 {
        self.layout().last().map_or(0.0, |(y, h, _)| y + h + 8.0)
    }

    // ---------------------------------------------------------------- paint

    pub fn paint(&mut self, gfx: &Gfx, pal: &Palette, hotkey: &str) {
        // Taken out so rows can fill the bitmap cache while drawing.
        let Some(target) = self.target.take() else { return };
        let t = &target;
        let f = &gfx.fonts;
        t.begin();
        t.clear_a(pal.panel, pal.glass);

        // Accent bar grows with the query: the overlay's signature.
        let qlen = self.query.chars().count() as f32;
        let frac = if self.query.is_empty() { 0.22 } else { (0.18 + qlen * 0.14).min(1.0) };
        t.fill(rect(0.0, 0.0, W * frac, 2.0), pal.accent);

        // Header
        t.text(glyph::BOLT, &f.icon20, rect(14.0, 17.0, 30.0, 30.0), pal.accent_ink);
        let idle = self.query.trim().is_empty();
        let badge = if idle {
            hotkey.replace('+', " ")
        } else {
            format!("{} results · {:.1} ms", self.hits.len(), self.elapsed_us as f64 / 1000.0)
        };
        let bw = gfx.text_width(&badge, &f.small) + 16.0;
        let bx = W - 20.0 - bw;
        let br = rect(bx, 22.0, bw, 20.0);
        if idle {
            t.stroke_round_a(br, 6.0, pal.text, 0.14, 1.0);
        }
        t.text(&badge, &f.small_center, br, pal.muted);
        let input = rect(54.0, 16.0, bx - 54.0 - 12.0, 32.0);
        if self.query.is_empty() {
            t.text("Search apps, files, settings…", &f.input, input, pal.placeholder);
        } else {
            if self.select_all {
                let w = gfx.text_width(&self.query, &f.input).min(input.right - input.left);
                t.fill_round_a(rect(52.0, 19.0, w + 4.0, 26.0), 4.0, pal.accent, pal.match_tint);
            }
            t.text(&self.query, &f.input, input, pal.text);
        }
        if self.caret_on && !self.select_all {
            let pos16 = self.query[..self.caret].encode_utf16().count() as u32;
            let x = 54.0 + if self.query.is_empty() { 0.0 } else { caret_x(gfx, &self.query, &f.input, pos16) };
            t.fill(rect(x.min(input.right), 21.0, 1.5, 22.0), pal.accent_ink);
        }
        t.fill_a(rect(0.0, HEADER, W, 1.0), pal.text, 0.08);

        // List
        t.clip(rect(0.0, LIST_Y, W, LIST_H));
        for (y, h, item) in self.layout() {
            let y = LIST_Y + y - self.scroll;
            if y + h < LIST_Y || y > LIST_Y + LIST_H {
                continue;
            }
            match item {
                Item::Label(l) => t.text(l, &f.label, rect(20.0, y + h - 22.0, 300.0, 16.0), pal.muted),
                Item::Row(i) => self.paint_row(gfx, t, pal, i, y),
                Item::Tiles => {
                    let tw = (W - 40.0 - 24.0) / 4.0;
                    for (k, (label, g, _)) in QUICK_SETTINGS.iter().enumerate() {
                        let r = rect(20.0 + k as f32 * (tw + 8.0), y, tw, TILE_H);
                        t.fill_round_a(r, 10.0, pal.text, 0.05);
                        t.text(g, &f.icon18, rect(r.left, y + 10.0, tw, 24.0), pal.text);
                        t.text(label, &f.small_center, rect(r.left, y + 38.0, tw, 16.0), pal.text);
                    }
                }
                Item::Empty => {
                    let msg = if self.scanning { "No matches yet. Still indexing…" } else { "No matches. Try fewer letters." };
                    t.text(msg, &f.small_center, rect(0.0, y + 40.0, W, 20.0), pal.muted);
                }
            }
        }
        t.unclip();

        // Footer
        let fy = LIST_Y + LIST_H;
        t.fill_a(rect(0.0, fy, W, FOOT_H), pal.text, 0.03);
        t.fill_a(rect(0.0, fy, W, 1.0), pal.text, 0.08);
        let left = if self.scanning {
            format!("Indexing · {} items", group(self.indexed))
        } else {
            format!("{} indexed", group(self.indexed))
        };
        t.text(&left, &f.small, rect(20.0, fy, 300.0, FOOT_H), pal.muted);
        let hints = "↑↓ Navigate     Enter Open     Ctrl+Enter Reveal     Esc Close";
        let hw = gfx.text_width(hints, &f.small);
        t.text(hints, &f.small, rect(W - 20.0 - hw, fy, hw + 2.0, FOOT_H), pal.muted);

        if t.end() {
            self.target = Some(target);
        } else {
            self.bitmaps.clear();
            self.invalidate();
        }
    }

    fn paint_row(&mut self, gfx: &Gfx, t: &Target, pal: &Palette, i: usize, y: f32) {
        let f = &gfx.fonts;
        let hit = &self.hits[i];
        let selected = i == self.sel;
        if selected {
            t.fill_round_a(rect(10.0, y, W - 20.0, ROW_H), 10.0, pal.text, if pal.dark { 0.08 } else { 0.06 });
        }
        let bmp = self.icons.get(i).and_then(|o| o.as_ref()).and_then(|icon| {
            let key = Arc::as_ptr(icon) as usize;
            if !self.bitmaps.contains_key(&key)
                && let Ok(b) = t.create_bitmap(icon.size, icon.size, &icon.px)
            {
                self.bitmaps.insert(key, b);
            }
            self.bitmaps.get(&key)
        });
        if let Some(b) = bmp {
            t.bitmap(b, rect(23.0, y + 11.0, 26.0, 26.0));
        } else {
            let g = match hit.kind {
                Kind::App => glyph::APPS,
                Kind::File => glyph::PAGE,
                Kind::Setting => glyph::SETTING,
            };
            let tile = rect(20.0, y + 8.0, 32.0, 32.0);
            t.fill_round_a(tile, 8.0, pal.text, 0.07);
            t.text(g, &f.icon14, tile, pal.text);
        }
        let tw = W - 64.0 - 90.0;
        t.text_hl(gfx, &hit.title, &hit.ranges, &f.title, rect(64.0, y + 5.0, tw, 22.0), pal.text, (pal.accent, pal.match_tint));
        t.text(&hit.subtitle, &f.small, rect(64.0, y + 26.0, tw, 16.0), pal.muted);
        if selected {
            let cap = if hit.kind == Kind::Setting { "Go" } else { "Enter" };
            let cw = gfx.text_width(cap, &f.small) + 16.0;
            let r = rect(W - 22.0 - cw, y + 14.0, cw, 20.0);
            t.fill_round_a(r, 6.0, pal.text, 0.08);
            t.text(cap, &f.small_center, r, pal.muted);
        }
    }
}

fn ctrl_down() -> bool {
    unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 }
}

fn prev_boundary(s: &str, i: usize) -> usize {
    s[..i].char_indices().next_back().map_or(0, |(k, _)| k)
}

fn next_boundary(s: &str, i: usize) -> usize {
    s[i..].chars().next().map_or(i, |c| i + c.len_utf8())
}

fn word_start(s: &str, i: usize) -> usize {
    let t = s[..i].trim_end();
    t.rfind(|c: char| c.is_whitespace() || c == '\\' || c == '/').map_or(0, |k| k + 1)
}

fn group(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping() {
        assert_eq!(group(0), "0");
        assert_eq!(group(1234), "1,234");
        assert_eq!(group(142381), "142,381");
    }

    #[test]
    fn word_delete() {
        assert_eq!(word_start("hello world", 11), 6);
        assert_eq!(word_start("hello world  ", 13), 6);
        assert_eq!(word_start("abc", 3), 0);
    }
}
