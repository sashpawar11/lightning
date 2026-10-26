//! UI icons as vector paths: Fluent UI System Icons, 20px regular (MIT, see
//! assets/FLUENT-ICONS-LICENSE.txt). Drawn as Direct2D geometry, so they
//! don't depend on which icon fonts the installed Windows ships.

/// An icon, indexing [`PATHS`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Glyph(pub usize);

pub mod glyph {
    use super::Glyph;
    pub const BOLT: Glyph = Glyph(0);
    pub const APPS: Glyph = Glyph(1);
    pub const PAGE: Glyph = Glyph(2);
    pub const SETTING: Glyph = Glyph(3);
    pub const MINIMIZE: Glyph = Glyph(4);
    pub const WIFI: Glyph = Glyph(5);
    pub const VOLUME: Glyph = Glyph(6);
    pub const MUTE: Glyph = Glyph(7);
    pub const DARK: Glyph = Glyph(8);
    pub const LIGHT: Glyph = Glyph(9);
    pub const BLUETOOTH: Glyph = Glyph(10);
    pub const ADMIN: Glyph = Glyph(11);
    pub const FOLDER: Glyph = Glyph(12);
    pub const POWER: Glyph = Glyph(13);
    pub const RESTART: Glyph = Glyph(14);
    pub const SIGN_OUT: Glyph = Glyph(15);
    pub const LOCK: Glyph = Glyph(16);
    pub const HIBERNATE: Glyph = Glyph(17);
}

/// Path data in a 20×20 box, in [`glyph`] order. Names are the Fluent ones.
pub const PATHS: [&str; 18] = [
    // flash
    "M6.2 2.77C6.31 2.31 6.73 2 7.2 2h5.25c.73 0 1.24.71 1 1.4L12.22 7h2.56c.95 0 1.4 1.14.77 1.81l-8.68 8.86c-.76.79-2.06.06-1.8-1L6.23 12H4.96c-.7 0-1.21-.68-1.02-1.36zM7.2 3a.1.1 0 0 0-.06.05L4.9 10.92q0 .07.06.08h1.91a.5.5 0 0 1 .49.62l-1.33 5.3v.04l.03.03.04.01h.01l.03-.02 8.68-8.86.01-.03v-.05l-.03-.03-.03-.01H11.5a.5.5 0 0 1-.47-.67l1.5-4.25a.06.06 0 0 0-.07-.08z",
    // apps
    "M4.5 17A1.5 1.5 0 0 1 3 15.65V4.5c0-.78.6-1.42 1.36-1.5H9c.78 0 1.42.6 1.5 1.36v.4l2.19-2.27a1.5 1.5 0 0 1 2-.14l.12.1 2.76 2.72c.55.55.6 1.42.11 2.02l-.1.11-2.31 2.2h.23c.78 0 1.42.6 1.5 1.36v4.64c0 .78-.6 1.42-1.36 1.5H4.5m5-6.5H4v5c0 .22.14.4.33.47l.08.02.09.01h5zm6 0h-5V16h5a.5.5 0 0 0 .5-.4V11a.5.5 0 0 0-.41-.5zm-5-2.79V9.5h1.79zM9 4.01H4.5a.5.5 0 0 0-.5.4v5.1h5.5v-5a.5.5 0 0 0-.33-.48l-.08-.02zm5.12-.84a.5.5 0 0 0-.64-.05l-.07.06-2.62 2.71a.5.5 0 0 0-.05.63l.06.07 2.61 2.62c.17.17.43.2.62.07l.08-.06 2.76-2.63a.5.5 0 0 0 .05-.64l-.05-.06z",
    // document
    "M6 2a2 2 0 0 0-2 2v12c0 1.1.9 2 2 2h8a2 2 0 0 0 2-2V7.41c0-.4-.16-.78-.44-1.06l-3.91-3.91A1.5 1.5 0 0 0 10.59 2zM5 4a1 1 0 0 1 1-1h4v3.5c0 .83.67 1.5 1.5 1.5H15v8a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1zm9.8 3h-3.3a.5.5 0 0 1-.5-.5V3.2z",
    // settings
    "M1.91 7.38A8.5 8.5 0 0 1 3.7 4.3a.5.5 0 0 1 .54-.13l1.92.68a1 1 0 0 0 1.32-.76l.36-2a.5.5 0 0 1 .4-.4 9 9 0 0 1 3.55 0q.32.08.38.4l.37 2a1 1 0 0 0 1.32.76l1.92-.68a.5.5 0 0 1 .54.13 8.5 8.5 0 0 1 1.78 3.08q.08.31-.15.54l-1.56 1.32a1 1 0 0 0 0 1.52l1.56 1.32a.5.5 0 0 1 .15.54 8.5 8.5 0 0 1-1.78 3.08.5.5 0 0 1-.54.13l-1.92-.68a1 1 0 0 0-1.32.76l-.37 2a.5.5 0 0 1-.38.4 8.5 8.5 0 0 1-3.56 0 .5.5 0 0 1-.39-.4l-.36-2a1 1 0 0 0-1.32-.76l-1.92.68a.5.5 0 0 1-.54-.13 8.5 8.5 0 0 1-1.78-3.08.5.5 0 0 1 .15-.54l1.56-1.32a1 1 0 0 0 0-1.52L2.06 7.92a.5.5 0 0 1-.15-.54m1.06 0 1.3 1.1a2 2 0 0 1 0 3.04l-1.3 1.1q.45 1.19 1.25 2.16l1.6-.58a2 2 0 0 1 2.63 1.53l.3 1.67a8 8 0 0 0 2.5 0l.3-1.67a2 2 0 0 1 2.64-1.53l1.6.58a8 8 0 0 0 1.24-2.16l-1.3-1.1a2 2 0 0 1 0-3.04l1.3-1.1a8 8 0 0 0-1.25-2.16l-1.6.58a2 2 0 0 1-2.63-1.53l-.3-1.67a8 8 0 0 0-2.5 0l-.3 1.67A2 2 0 0 1 5.81 5.8l-1.6-.58a8 8 0 0 0-1.24 2.16M7.5 10a2.5 2.5 0 1 1 5 0 2.5 2.5 0 0 1-5 0m1 0a1.5 1.5 0 1 0 3 0 1.5 1.5 0 0 0-3 0",
    // subtract
    "M3 10c0-.28.22-.5.5-.5h13a.5.5 0 0 1 0 1h-13A.5.5 0 0 1 3 10",
    // wifi_1
    "M16.83 7.38q.62.63 1.14 1.37a.5.5 0 0 1-.83.57 8.61 8.61 0 0 0-14.2 0 .5.5 0 0 1-.82-.58 9.61 9.61 0 0 1 14.71-1.36M14.6 9.36q.71.72 1.19 1.65a.5.5 0 0 1-.89.46q-.4-.8-1.01-1.4a5.45 5.45 0 0 0-8.72 1.38.5.5 0 1 1-.89-.45q.45-.9 1.2-1.64a6.45 6.45 0 0 1 9.12 0m-1.71 2.5q.53.53.85 1.25a.5.5 0 0 1-.92.4 3.03 3.03 0 0 0-5.56-.01.5.5 0 1 1-.92-.4q.3-.72.84-1.25a4.03 4.03 0 0 1 5.7 0m-1.93 1.93a1.3 1.3 0 1 1-1.83 1.83 1.3 1.3 0 0 1 1.83-1.83",
    // speaker_2
    "M12 3a1 1 0 0 0-1.68-.73l-3.88 3.6A.5.5 0 0 1 6.1 6H3.5C2.67 6 2 6.67 2 7.5v5c0 .83.67 1.5 1.5 1.5h2.6a.5.5 0 0 1 .34.13l3.88 3.6a1 1 0 0 0 1.68-.74zM7.12 6.6 11 3v14l-3.88-3.6A1.5 1.5 0 0 0 6.1 13H3.5a.5.5 0 0 1-.5-.5v-5c0-.28.22-.5.5-.5h2.6c.38 0 .75-.14 1.02-.4m8.14-1.97a.5.5 0 0 1 .7.04 8 8 0 0 1 0 10.66.5.5 0 0 1-.74-.66 7 7 0 0 0 0-9.34.5.5 0 0 1 .04-.7m-1.18 8.3a.5.5 0 0 1-.18-.68 4.5 4.5 0 0 0 0-4.5.5.5 0 1 1 .86-.5 5.5 5.5 0 0 1 0 5.5.5.5 0 0 1-.68.18",
    // speaker_mute
    "M12 3a1 1 0 0 0-1.68-.73l-3.88 3.6A.5.5 0 0 1 6.1 6H3.5C2.67 6 2 6.67 2 7.5v5c0 .83.67 1.5 1.5 1.5h2.6a.5.5 0 0 1 .34.13l3.88 3.6a1 1 0 0 0 1.68-.74zM7.12 6.6 11 3v14l-3.88-3.6A1.5 1.5 0 0 0 6.1 13H3.5a.5.5 0 0 1-.5-.5v-5c0-.28.22-.5.5-.5h2.6c.38 0 .75-.14 1.02-.4m6.02 1.05c.2-.2.51-.2.71 0l1.65 1.64 1.64-1.64a.5.5 0 1 1 .71.7L16.2 10l1.65 1.65a.5.5 0 1 1-.7.7l-1.65-1.64-1.65 1.64a.5.5 0 1 1-.7-.7L14.78 10l-1.65-1.65a.5.5 0 0 1 0-.7",
    // weather_moon
    "M15.5 13.5A6.98 6.98 0 0 1 4 14.39c2.83-1.09 4.56-2.42 5.6-4.4 1.04-2 1.33-4.16.75-6.9q1.36.17 2.58.87a7 7 0 0 1 2.55 9.54M5.45 16.92A7.98 7.98 0 1 0 9.88 2.04a.6.6 0 0 0-.61.73c.69 2.82.43 4.88-.55 6.76-.94 1.78-2.55 3.03-5.55 4.1a.6.6 0 0 0-.3.9 8 8 0 0 0 2.59 2.39",
    // weather_sunny
    "M10 2c.28 0 .5.22.5.5v1a.5.5 0 0 1-1 0v-1c0-.28.22-.5.5-.5m0 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8m0-1a3 3 0 1 1 0-6 3 3 0 0 1 0 6m7.5-2.5a.5.5 0 0 0 0-1h-1a.5.5 0 0 0 0 1zM10 16c.28 0 .5.22.5.5v1a.5.5 0 0 1-1 0v-1c0-.28.22-.5.5-.5m-6.5-5.5a.5.5 0 0 0 0-1H2.46a.5.5 0 0 0 0 1zm.65-6.35c.2-.2.5-.2.7 0l1 1a.5.5 0 1 1-.7.7l-1-1a.5.5 0 0 1 0-.7m.7 11.7a.5.5 0 0 1-.7-.7l1-1a.5.5 0 0 1 .7.7zm11-11.7a.5.5 0 0 0-.7 0l-1 1a.5.5 0 0 0 .7.7l1-1a.5.5 0 0 0 0-.7m-.7 11.7a.5.5 0 0 0 .7-.7l-1-1a.5.5 0 0 0-.7.7z",
    // bluetooth
    "M9.3 2.04a.5.5 0 0 1 .55.1l4 4a.5.5 0 0 1-.02.74L10.26 10l3.57 3.12a.5.5 0 0 1 .02.73l-4 4A.5.5 0 0 1 9 17.5v-6.4l-3.17 2.78a.5.5 0 0 1-.66-.76L8.74 10 5.17 6.88a.5.5 0 0 1 .66-.76L9 8.9V2.5q.01-.32.3-.46m.7 9.06v5.2l2.77-2.78zm0-2.2 2.77-2.42L10 3.7z",
    // shield
    "M9.72 2.08a.5.5 0 0 1 .56 0q2.9 1.94 6.3 2.43A.5.5 0 0 1 17 5v4.5c0 3.9-2.3 6.73-6.82 8.47a.5.5 0 0 1-.36 0C5.31 16.23 3 13.39 3 9.5V5a.5.5 0 0 1 .43-.5 15 15 0 0 0 6.3-2.42M9.6 3.35A16 16 0 0 1 4 5.43V9.5c0 3.4 1.97 5.86 6 7.46 4.03-1.6 6-4.07 6-7.46V5.43a16 16 0 0 1-5.6-2.08L10 3.1z",
    // folder_open
    "M3 5.5v6.6l1.5-2.6A3 3 0 0 1 7.1 8H15v-.5c0-.83-.67-1.5-1.5-1.5h-4a.5.5 0 0 1-.35-.15l-1.71-1.7A.5.5 0 0 0 7.09 4H4.5C3.67 4 3 4.67 3 5.5m1.28 10.48.22.02h9.4a2 2 0 0 0 1.73-1l2.17-3.75A1.5 1.5 0 0 0 16.5 9H7.1a2 2 0 0 0-1.73 1L3.2 13.75a1.5 1.5 0 0 0 1.08 2.23M2 14.46V5.5A2.5 2.5 0 0 1 4.5 3h2.59c.4 0 .78.16 1.06.44L9.7 5h3.79A2.5 2.5 0 0 1 16 7.5V8h.5a2.5 2.5 0 0 1 2.16 3.75L16.5 15.5a3 3 0 0 1-2.6 1.5H4.5a2.5 2.5 0 0 1-1.62-.6A2.5 2.5 0 0 1 2 14.46",
    // power: ring open at the top, bar through the gap
    "M9.25 2h1.5v8h-1.5zM4.7 5.7A7.5 7.5 0 1 0 15.3 5.7L14.24 6.76A6 6 0 1 1 5.76 6.76z",
    // restart: ring open at the top right, arrowhead at its end
    "M14.95 6.05A7 7 0 1 1 10 4V5.5A5.5 5.5 0 1 0 13.89 7.11zM9.5 1.5l4.5 3.25L9.5 8z",
    // sign out: open door frame, arrow leaving it
    "M4 3h6v1.5H5.5v11H10V17H4zM9 9.25h6.2l-2-2 1.05-1.05L18.1 10l-3.85 3.8-1.05-1.05 2-2H9z",
    // lock: shackle over a body
    "M6.5 9V6.5a3.5 3.5 0 0 1 7 0V9H12V6.5a2 2 0 0 0-4 0V9zM5 9h10a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1z",
    // hibernate: arrow down into a tray
    "M9.25 3h1.5v8.2l2.2-2.2 1.05 1.05L10 14 6 10.05 7.05 9l2.2 2.2zM4 15.5h12V17H4z",
];

pub type Pt = (f32, f32);

/// One path segment in absolute coordinates.
#[derive(Debug, PartialEq)]
pub enum Seg {
    Move(Pt),
    Line(Pt),
    Cubic(Pt, Pt, Pt),
    Quad(Pt, Pt),
    /// End point, radii, x-axis rotation (degrees), large-arc, sweep.
    Arc(Pt, Pt, f32, bool, bool),
    Close,
}

/// Parses the SVG path subset the icons use (M L H V C Q A Z, either case).
/// Returns `None` on anything else.
pub fn parse(d: &str) -> Option<Vec<Seg>> {
    let mut p = Reader { s: d.as_bytes(), i: 0 };
    let mut out = Vec::new();
    let (mut cur, mut start) = ((0.0, 0.0), (0.0, 0.0));
    let mut cmd = 0u8;
    while let Some(c) = p.peek() {
        if c.is_ascii_alphabetic() {
            cmd = c;
            p.i += 1;
        } else if cmd == 0 {
            return None;
        }
        let rel = cmd.is_ascii_lowercase();
        let at = |(x, y): Pt, cur: Pt| if rel { (cur.0 + x, cur.1 + y) } else { (x, y) };
        match cmd.to_ascii_uppercase() {
            b'M' => {
                cur = at(p.pt()?, cur);
                start = cur;
                out.push(Seg::Move(cur));
                // Further pairs after a move are lines.
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                cur = at(p.pt()?, cur);
                out.push(Seg::Line(cur));
            }
            b'H' => {
                let x = p.num()?;
                cur.0 = if rel { cur.0 + x } else { x };
                out.push(Seg::Line(cur));
            }
            b'V' => {
                let y = p.num()?;
                cur.1 = if rel { cur.1 + y } else { y };
                out.push(Seg::Line(cur));
            }
            b'C' => {
                let (a, b, e) = (at(p.pt()?, cur), at(p.pt()?, cur), at(p.pt()?, cur));
                cur = e;
                out.push(Seg::Cubic(a, b, e));
            }
            b'Q' => {
                let (a, e) = (at(p.pt()?, cur), at(p.pt()?, cur));
                cur = e;
                out.push(Seg::Quad(a, e));
            }
            b'A' => {
                let radii = p.pt()?;
                let rot = p.num()?;
                let (large, sweep) = (p.flag()?, p.flag()?);
                cur = at(p.pt()?, cur);
                out.push(Seg::Arc(cur, radii, rot, large, sweep));
            }
            b'Z' => {
                cur = start;
                out.push(Seg::Close);
                p.skip();
                if p.peek().is_some_and(|c| !c.is_ascii_alphabetic()) {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

struct Reader<'a> {
    s: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn skip(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b',' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.s.get(self.i).copied()
    }

    /// A number; minified data packs them, e.g. ".5.5" or "1-2".
    fn num(&mut self) -> Option<f32> {
        self.skip();
        let start = self.i;
        if matches!(self.s.get(self.i), Some(b'-' | b'+')) {
            self.i += 1;
        }
        let mut dot = false;
        while let Some(&c) = self.s.get(self.i) {
            match c {
                b'0'..=b'9' => {}
                b'.' if !dot => dot = true,
                _ => break,
            }
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()
    }

    fn pt(&mut self) -> Option<Pt> {
        Some((self.num()?, self.num()?))
    }

    /// Arc flags are single digits and may be packed without separators.
    fn flag(&mut self) -> Option<bool> {
        let c = self.peek()?;
        self.i += 1;
        match c {
            b'0' => Some(false),
            b'1' => Some(true),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_icons_parse_inside_their_box() {
        for (k, d) in PATHS.iter().enumerate() {
            let segs = parse(d).unwrap_or_else(|| panic!("icon {k} failed to parse"));
            assert!(matches!(segs.first(), Some(Seg::Move(_))), "icon {k}");
            for s in &segs {
                let p = match s {
                    Seg::Move(p) | Seg::Line(p) | Seg::Cubic(_, _, p) | Seg::Quad(_, p) | Seg::Arc(p, ..) => *p,
                    Seg::Close => continue,
                };
                assert!((0.0..=20.0).contains(&p.0) && (0.0..=20.0).contains(&p.1), "icon {k}: {p:?}");
            }
        }
    }

    #[test]
    fn packed_numbers_relative_commands_and_arcs() {
        let segs = parse("M1 2l.5.5-1-1h2v-3a.5.5 0 0 1 .5.5z").unwrap();
        assert_eq!(
            segs,
            vec![
                Seg::Move((1.0, 2.0)),
                Seg::Line((1.5, 2.5)),
                Seg::Line((0.5, 1.5)),
                Seg::Line((2.5, 1.5)),
                Seg::Line((2.5, -1.5)),
                Seg::Arc((3.0, -1.0), (0.5, 0.5), 0.0, false, true),
                Seg::Close,
            ]
        );
    }

    #[test]
    fn rejects_unsupported_commands() {
        assert_eq!(parse("M0 0S1 1 2 2"), None);
    }
}
