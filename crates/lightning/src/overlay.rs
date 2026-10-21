//! The search overlay: input editing, result list, painting.

use std::collections::HashMap;
use std::sync::Arc;

use lightning_core::{Hit, Kind};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};

use crate::gfx::{Gfx, Target, caret_x, glyph, rect};
use crate::quick::{self, States};
use crate::theme::Palette;
use crate::worker::{Icon, Icons, Resp};
use crate::{Fx, platform};

pub const W: f32 = 680.0;
const HEADER: f32 = 64.0;
const LIST_Y: f32 = 65.0;
/// The list grows with its content up to this height, then scrolls.
const MAX_LIST_H: f32 = 372.0;
const MIN_LIST_H: f32 = 120.0;
const FOOT_H: f32 = 36.0;
const ROW_H: f32 = 48.0;
const ROW_STEP: f32 = 50.0;
const TILE_H: f32 = 52.0;
/// Recents grid: columns, and the height of one row of tiles.
const COLS: usize = 6;
const GRID_H: f32 = 80.0;
const CELL_W: f32 = (W - 40.0) / COLS as f32;

const TILE_W: f32 = (W - 40.0 - 24.0) / 4.0;

fn tile_rect(k: usize, y: f32) -> D2D_RECT_F {
    rect(20.0 + k as f32 * (TILE_W + 8.0), y, TILE_W, TILE_H)
}

/// Right-click menu entries, in order.
const MENU_ITEMS: [(&str, &str); 2] = [(glyph::ADMIN, "Run as administrator"), (glyph::FOLDER, "Open file location")];
const MENU_W: f32 = 208.0;
const MENU_ITEM_H: f32 = 32.0;
const MENU_PAD: f32 = 5.0;
const MENU_H: f32 = MENU_PAD * 2.0 + MENU_ITEMS.len() as f32 * MENU_ITEM_H;

/// The right-click menu, drawn inside the overlay so it costs no window.
struct Menu {
    hit: usize,
    /// Top-left corner, DIPs.
    x: f32,
    y: f32,
    /// Highlighted entry (mouse or arrow keys).
    sel: Option<usize>,
    enabled: [bool; 2],
    /// The executable or file the entries act on; packaged apps have none.
    path: Option<String>,
    args: Option<String>,
}

impl Menu {
    fn item_rect(&self, k: usize) -> D2D_RECT_F {
        rect(self.x + MENU_PAD, self.y + MENU_PAD + k as f32 * MENU_ITEM_H, MENU_W - MENU_PAD * 2.0, MENU_ITEM_H)
    }

    fn contains(&self, x: f32, y: f32) -> bool {
        (self.x..self.x + MENU_W).contains(&x) && (self.y..self.y + MENU_H).contains(&y)
    }

    fn item_at(&self, x: f32, y: f32) -> Option<usize> {
        if !self.contains(x, y) {
            return None;
        }
        let k = ((y - self.y - MENU_PAD) / MENU_ITEM_H).floor();
        (k >= 0.0 && (k as usize) < MENU_ITEMS.len() && self.enabled[k as usize]).then_some(k as usize)
    }
}

/// Controls with a hover state that isn't the selection.
#[derive(Clone, Copy, PartialEq)]
enum Hover {
    Tile(usize),
    Gear,
}

#[derive(Clone, Copy)]
enum Item {
    Label(&'static str),
    Row(usize),
    /// A row of recent tiles, starting at this hit index.
    Grid(usize),
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
    pub quick: States,
    hover: Option<Hover>,
    recents: (Vec<Hit>, Vec<Option<Arc<Icon>>>),
    last_mouse: (i32, i32),
    menu: Option<Menu>,
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
            quick: [None; 4],
            hover: None,
            recents: (Vec::new(), Vec::new()),
            last_mouse: (i32::MIN, i32::MIN),
            menu: None,
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

    /// Every show starts a fresh search.
    pub fn on_show(&mut self) {
        self.query.clear();
        self.caret = 0;
        self.select_all = false;
        // The last recents, so the window opens at its final size.
        (self.hits, self.icons) = self.recents.clone();
        self.results_query = Some(String::new());
        self.sel = 0;
        self.scroll = 0.0;
        self.caret_on = true;
        self.hover = None;
        self.last_mouse = (i32::MIN, i32::MIN);
        self.menu = None;
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
        // The menu stays only if its hit is still at the same index.
        if let Some(m) = &self.menu
            && resp.results.hits.get(m.hit).map(|h| &h.target) != self.hits.get(m.hit).map(|h| &h.target)
        {
            self.menu = None;
        }
        let same_query = self.results_query.as_deref() == Some(resp.results.query.as_str());
        self.elapsed_us = resp.results.elapsed_us;
        self.hits = resp.results.hits;
        self.icons = resp.icons;
        self.results_query = Some(resp.results.query);
        if self.results_query.as_deref() == Some("") {
            self.recents = (self.hits.clone(), self.icons.clone());
        }
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

    pub fn on_icons(&mut self, icons: Icons) {
        if icons.generation != self.generation {
            return;
        }
        for (i, icon) in icons.icons {
            if let Some(slot) = self.icons.get_mut(i) {
                *slot = Some(icon);
            }
        }
        if self.results_query.as_deref() == Some("") {
            self.recents.1 = self.icons.clone();
        }
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
        self.close_menu(); // typing goes to the query
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
        if let Some(m) = &mut self.menu {
            match vk {
                0x1B => self.close_menu(),
                // Down/Up: the next enabled entry, wrapping.
                0x28 | 0x26 => {
                    let n = MENU_ITEMS.len();
                    let (step, from) = if vk == 0x28 { (1, n - 1) } else { (n - 1, 0) };
                    let mut k = m.sel.unwrap_or(from);
                    for _ in 0..n {
                        k = (k + step) % n;
                        if m.enabled[k] {
                            m.sel = Some(k);
                            break;
                        }
                    }
                    self.invalidate();
                }
                0x0D => {
                    if let Some(k) = m.sel {
                        self.menu_action(k, fx);
                    }
                }
                // Anything else closes the menu and acts as usual.
                _ => {
                    self.close_menu();
                    return self.on_key(vk, fx);
                }
            }
            return true;
        }
        match vk {
            // Context-menu key or Shift+F10: the menu for the selection.
            0x5D => self.open_menu_at_sel(),
            0x79 if shift_down() => self.open_menu_at_sel(),
            // One press always closes; the query is kept and pre-selected
            // on the next show, so typing replaces it.
            0x1B => fx.push(Fx::HideOverlay),
            0x28 if self.idle() => self.move_grid(COLS as isize),
            0x26 if self.idle() => self.move_grid(-(COLS as isize)),
            0x27 if self.idle() => self.move_sel(1),
            0x25 if self.idle() => self.move_sel(-1),
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

    fn idle(&self) -> bool {
        self.query.trim().is_empty()
    }

    /// Up/down in the recents grid: only when a tile is there.
    fn move_grid(&mut self, d: isize) {
        let to = self.sel as isize + d;
        if (0..self.hits.len() as isize).contains(&to) {
            self.move_sel(d);
        }
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
            let hit = match item {
                Item::Row(i) => i == self.sel,
                Item::Grid(start) => (start..start + COLS).contains(&self.sel),
                _ => false,
            };
            if hit {
                let i = self.sel;
                // Keep the group label visible when selecting a group's first row.
                let top = if i == 0 { 0.0 } else { y - 4.0 };
                if top < self.scroll {
                    self.scroll = top;
                } else if y + h + 8.0 > self.scroll + self.list_h() {
                    self.scroll = y + h + 8.0 - self.list_h();
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
        if !(LIST_Y..LIST_Y + self.list_h()).contains(&y_dip) {
            return None;
        }
        let cy = y_dip - LIST_Y + self.scroll;
        self.layout()
            .into_iter()
            .find(|&(y, h, _)| cy >= y && cy < y + h)
            .map(|(_, _, it)| it)
    }

    /// The hit under a point (DIPs), list rows and recent tiles alike.
    fn hit_at(&self, x: f32, y: f32) -> Option<usize> {
        match self.item_at(y)? {
            Item::Row(i) => Some(i),
            Item::Grid(start) => {
                let i = start + ((x - 20.0) / CELL_W).floor().clamp(0.0, COLS as f32 - 1.0) as usize;
                ((20.0..W - 20.0).contains(&x) && i < self.hits.len()).then_some(i)
            }
            _ => None,
        }
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
        let s = 96.0 / self.dpi;
        if let Some(m) = &mut self.menu {
            // While the menu is open, only its entries track the mouse.
            let sel = m.item_at(x as f32 * s, y as f32 * s);
            if sel != m.sel {
                m.sel = sel;
                self.invalidate();
            }
            return;
        }
        let hover = self.hover_at(x as f32 * s, y as f32 * s);
        if hover != self.hover {
            self.hover = hover;
            self.invalidate();
        }
        if let Some(i) = self.hit_at(x as f32 * s, y as f32 * s)
            && i != self.sel
        {
            self.sel = i;
            self.invalidate();
        }
    }

    pub fn on_click(&mut self, x: i32, y: i32, fx: &mut Vec<Fx>) {
        let (x, y) = (x as f32 * 96.0 / self.dpi, y as f32 * 96.0 / self.dpi);
        if let Some(m) = &self.menu {
            // A click outside only dismisses the menu.
            match m.item_at(x, y) {
                Some(k) => self.menu_action(k, fx),
                None if m.contains(x, y) => {}
                None => self.close_menu(),
            }
            return;
        }
        if let Some(i) = self.hit_at(x, y) {
            self.launch(i, ctrl_down(), fx);
            return;
        }
        match self.hover_at(x, y) {
            Some(Hover::Tile(k)) => {
                if let Some(on) = self.quick[k] {
                    // Flip now; the quick thread posts the real state back.
                    self.quick[k] = Some(!on);
                    fx.push(Fx::Quick(k, !on));
                    self.invalidate();
                }
            }
            Some(Hover::Gear) => fx.push(Fx::ShowSettings),
            None => {}
        }
    }

    pub fn on_right_click(&mut self, x: i32, y: i32) {
        let (x, y) = (x as f32 * 96.0 / self.dpi, y as f32 * 96.0 / self.dpi);
        self.close_menu();
        if let Some(i) = self.hit_at(x, y) {
            self.sel = i;
            self.open_menu(i, x, y, false);
        }
    }

    // ----------------------------------------------------------------- menu

    /// Opens the menu for hit `i` with its corner near (x, y), in DIPs.
    fn open_menu(&mut self, i: usize, x: f32, y: f32, keyboard: bool) {
        let hit = &self.hits[i];
        // The file to elevate or reveal; packaged apps have none.
        let (path, args) = match hit.kind {
            Kind::File => (Some(hit.target.clone()), None),
            Kind::App => platform::app_exe(&hit.target).map_or((None, None), |(p, a)| (Some(p), a)),
            Kind::Setting => return,
        };
        // Packaged app ids are AUMIDs: `PackageFamilyName!AppId`.
        let packaged = hit.kind == Kind::App && path.is_none() && hit.target.contains('!');
        let enabled = [packaged || path.as_deref().is_some_and(platform::is_executable), path.is_some()];
        // Kept inside the window: flipped above the point near the bottom.
        let x = x.clamp(8.0, W - MENU_W - 8.0);
        let y = if y + MENU_H > self.height() - 8.0 { (y - MENU_H).max(8.0) } else { y };
        let sel = if keyboard { enabled.iter().position(|&e| e) } else { None };
        self.menu = Some(Menu { hit: i, x, y, sel, enabled, path, args });
        self.invalidate();
    }

    /// Context-menu key: the menu just below the selected row or tile.
    fn open_menu_at_sel(&mut self) {
        let at = self.layout().into_iter().find_map(|(y, h, item)| {
            let y = LIST_Y + y - self.scroll;
            match item {
                Item::Row(i) if i == self.sel => Some((64.0, y + ROW_H)),
                Item::Grid(start) if (start..start + COLS).contains(&self.sel) => {
                    Some((20.0 + (self.sel - start) as f32 * CELL_W + 8.0, y + h - 4.0))
                }
                _ => None,
            }
        });
        if let Some((x, y)) = at {
            self.open_menu(self.sel, x, y, true);
        }
    }

    fn close_menu(&mut self) {
        if self.menu.take().is_some() {
            self.invalidate();
        }
    }

    fn menu_action(&mut self, k: usize, fx: &mut Vec<Fx>) {
        let Some(m) = self.menu.take() else { return };
        if !m.enabled[k] {
            return;
        }
        let hit = self.hits[m.hit].clone();
        fx.push(match (k, m.path) {
            (0, exe) => Fx::RunAsAdmin { hit, exe: exe.map(|p| (p, m.args)) },
            (_, path) => Fx::OpenLocation { hit, path: path.unwrap_or_default() },
        });
    }

    /// Quick settings tile or footer gear under a point (DIPs).
    fn hover_at(&self, x: f32, y: f32) -> Option<Hover> {
        let g = self.gear_rect();
        if x >= g.left && x < g.right && y >= g.top && y < g.bottom {
            return Some(Hover::Gear);
        }
        if !matches!(self.item_at(y), Some(Item::Tiles)) {
            return None;
        }
        (0..4).find(|&k| {
            let r = tile_rect(k, 0.0);
            x >= r.left && x < r.right
        })
        .map(Hover::Tile)
    }

    fn gear_rect(&self) -> D2D_RECT_F {
        rect(W - 20.0 - 26.0, LIST_Y + self.list_h() + 5.0, 26.0, 26.0)
    }

    pub fn on_mouse_leave(&mut self) {
        if self.hover.take().is_some() {
            self.invalidate();
        }
    }

    pub fn on_wheel(&mut self, delta: i16) {
        self.close_menu(); // it would float away from its row
        let max = (self.content_height() - self.list_h()).max(0.0);
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
        if idle && !self.hits.is_empty() {
            out.push((y, 26.0, Item::Label("Recent")));
            y += 26.0;
            for start in (0..self.hits.len()).step_by(COLS) {
                out.push((y, GRID_H, Item::Grid(start)));
                y += GRID_H;
            }
        }
        for (i, h) in self.hits.iter().enumerate().filter(|_| !idle) {
            let label = if prev != Some(h.kind) {
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

    fn list_h(&self) -> f32 {
        self.content_height().clamp(MIN_LIST_H, MAX_LIST_H)
    }

    /// Window height (DIPs) that fits the current content.
    pub fn height(&self) -> f32 {
        LIST_Y + self.list_h() + FOOT_H
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
        let list_h = self.list_h();
        t.clip(rect(0.0, LIST_Y, W, list_h));
        for (y, h, item) in self.layout() {
            let y = LIST_Y + y - self.scroll;
            if y + h < LIST_Y || y > LIST_Y + list_h {
                continue;
            }
            match item {
                Item::Label(l) => t.text(l, &f.label, rect(20.0, y + h - 22.0, 300.0, 16.0), pal.muted),
                Item::Row(i) => self.paint_row(gfx, t, pal, i, y),
                Item::Grid(start) => {
                    for i in start..(start + COLS).min(self.hits.len()) {
                        self.paint_tile(gfx, t, pal, i, 20.0 + (i - start) as f32 * CELL_W, y);
                    }
                }
                Item::Tiles => {
                    let tw = TILE_W;
                    for (k, state) in self.quick.into_iter().enumerate() {
                        let r = tile_rect(k, y);
                        let (g, name, status) = quick::describe(k, state);
                        // The theme tile names the current mode; it has no "off".
                        let lit = state == Some(true) && k != quick::THEME;
                        let hovered = self.hover == Some(Hover::Tile(k)) && state.is_some();
                        let (ink, sub) = if lit {
                            t.fill_round(r, 10.0, pal.accent);
                            if hovered {
                                t.fill_round_a(r, 10.0, pal.on_accent, 0.08);
                            }
                            (pal.on_accent, pal.on_accent)
                        } else if state.is_some() {
                            t.fill_round_a(r, 10.0, pal.text, if hovered { 0.11 } else { 0.06 });
                            (pal.text, pal.muted)
                        } else {
                            t.fill_round_a(r, 10.0, pal.text, 0.03);
                            (pal.muted, pal.muted)
                        };
                        t.text(g, &f.icon18, rect(r.left + 8.0, y, 36.0, TILE_H), ink);
                        let tx = r.left + 48.0;
                        t.text(name, &f.title, rect(tx, y + 8.0, tw - 56.0, 19.0), ink);
                        t.text(status, &f.small, rect(tx, y + 27.0, tw - 56.0, 16.0), sub);
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
        let fy = LIST_Y + list_h;
        t.fill_a(rect(0.0, fy, W, FOOT_H), pal.text, 0.03);
        t.fill_a(rect(0.0, fy, W, 1.0), pal.text, 0.08);
        let left = if self.scanning {
            format!("Indexing · {} items", group(self.indexed))
        } else {
            format!("{} indexed", group(self.indexed))
        };
        t.text(&left, &f.small, rect(20.0, fy, 300.0, FOOT_H), pal.muted);
        let gear = self.gear_rect();
        if self.hover == Some(Hover::Gear) {
            t.fill_round_a(gear, 6.0, pal.text, 0.08);
        }
        let ink = if self.hover == Some(Hover::Gear) { pal.text } else { pal.muted };
        t.text(glyph::SETTING, &f.icon14, gear, ink);
        let hints = if idle { "Esc Close" } else { "Ctrl+Enter Reveal     Esc Close" };
        let hw = gfx.text_width(hints, &f.small);
        t.text(hints, &f.small, rect(gear.left - 12.0 - hw, fy, hw + 2.0, FOOT_H), pal.muted);

        if let Some(m) = &self.menu {
            let r = rect(m.x, m.y, MENU_W, MENU_H);
            // A soft drop shadow: two offset layers under an opaque panel.
            t.fill_round_a(rect(m.x - 2.0, m.y + 2.0, MENU_W + 4.0, MENU_H + 6.0), 12.0, 0x000000, 0.10);
            t.fill_round_a(rect(m.x - 1.0, m.y + 1.0, MENU_W + 2.0, MENU_H + 3.0), 11.0, 0x000000, 0.14);
            t.fill_round(r, 10.0, pal.panel);
            t.stroke_round_a(r, 10.0, pal.text, 0.12, 1.0);
            for (k, (g, label)) in MENU_ITEMS.into_iter().enumerate() {
                let ir = m.item_rect(k);
                if m.sel == Some(k) {
                    t.fill_round_a(ir, 6.0, pal.text, if pal.dark { 0.08 } else { 0.06 });
                }
                let ink = if m.enabled[k] { pal.text } else { pal.placeholder };
                t.text(g, &f.icon14, rect(ir.left + 4.0, ir.top, 28.0, MENU_ITEM_H), ink);
                t.text(label, &f.body, rect(ir.left + 36.0, ir.top, ir.right - ir.left - 40.0, MENU_ITEM_H), ink);
            }
        }

        if t.end() {
            self.target = Some(target);
        } else {
            self.bitmaps.clear();
            self.invalidate();
        }
    }

    /// Draws hit `i`'s icon centered in `tile`: the real icon at `size`, or
    /// a kind glyph on a soft square.
    fn paint_icon(&mut self, gfx: &Gfx, t: &Target, pal: &Palette, i: usize, tile: D2D_RECT_F, size: f32) {
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
            let (cx, cy) = ((tile.left + tile.right) / 2.0, (tile.top + tile.bottom) / 2.0);
            t.bitmap(b, rect(cx - size / 2.0, cy - size / 2.0, size, size));
        } else {
            let g = match self.hits[i].kind {
                Kind::App => glyph::APPS,
                Kind::File => glyph::PAGE,
                Kind::Setting => glyph::SETTING,
            };
            t.fill_round_a(tile, 8.0, pal.text, 0.07);
            t.text(g, &gfx.fonts.icon14, tile, pal.text);
        }
    }

    fn paint_tile(&mut self, gfx: &Gfx, t: &Target, pal: &Palette, i: usize, x: f32, y: f32) {
        if i == self.sel {
            let r = rect(x + 2.0, y, CELL_W - 4.0, GRID_H - 4.0);
            t.fill_round_a(r, 10.0, pal.text, if pal.dark { 0.08 } else { 0.06 });
        }
        self.paint_icon(gfx, t, pal, i, rect(x + CELL_W / 2.0 - 16.0, y + 10.0, 32.0, 32.0), 32.0);
        let title = &self.hits[i].title;
        t.text(title, &gfx.fonts.small_center, rect(x + 6.0, y + 48.0, CELL_W - 12.0, 18.0), pal.text);
    }

    fn paint_row(&mut self, gfx: &Gfx, t: &Target, pal: &Palette, i: usize, y: f32) {
        let f = &gfx.fonts;
        let selected = i == self.sel;
        if selected {
            t.fill_round_a(rect(10.0, y, W - 20.0, ROW_H), 10.0, pal.text, if pal.dark { 0.08 } else { 0.06 });
        }
        self.paint_icon(gfx, t, pal, i, rect(20.0, y + 8.0, 32.0, 32.0), 26.0);
        let hit = &self.hits[i];
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

fn shift_down() -> bool {
    unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 }
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
