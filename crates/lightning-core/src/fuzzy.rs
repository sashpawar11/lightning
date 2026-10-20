//! Tiny fuzzy matcher for the small in-memory sources (apps, settings).
//!
//! Files go through fff; these sources are a few hundred entries, so a
//! single-pass greedy matcher with word-boundary preference is both faster
//! and more predictable than a full alignment.

/// Byte ranges `[start, end)` into the matched string.
pub type Ranges = Vec<(u32, u32)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub score: i32,
    pub ranges: Ranges,
}

/// Precomputed form of a searchable entry, built once when a source loads:
/// a character bitmask that rejects most entries with one AND, plus the
/// lowercase words used by the typo-tolerant fallback.
pub struct Prepared {
    pub title_mask: u64,
    words: Box<[Word]>,
}

struct Word {
    start: u32,
    end: u32,
    in_title: bool,
    mask: u64,
    lower: Box<[u8]>,
}

impl Prepared {
    pub fn new(title: &str, keywords: &str) -> Self {
        let words = [(true, title), (false, keywords)]
            .into_iter()
            .flat_map(|(in_title, text)| {
                words(text).map(move |(start, w)| {
                    let lower = w.to_lowercase().into_bytes().into_boxed_slice();
                    Word {
                        start: start as u32,
                        end: (start + w.len()) as u32,
                        in_title,
                        mask: byte_mask(&lower),
                        lower,
                    }
                })
            })
            .collect();
        Self { title_mask: char_mask(title), words }
    }
}

/// Bit per lowercase ASCII letter/digit; bit 63 for anything non-ASCII.
pub fn char_mask(s: &str) -> u64 {
    s.chars().filter(|c| !c.is_whitespace()).fold(0, |m, c| m | char_bit(lower(c)))
}

fn byte_mask(b: &[u8]) -> u64 {
    b.iter().fold(0, |m, &c| m | char_bit(c as char))
}

#[inline]
fn char_bit(c: char) -> u64 {
    match c {
        'a'..='z' => 1 << (c as u32 - 'a' as u32),
        '0'..='9' => 1 << (26 + c as u32 - '0' as u32),
        c if c.is_ascii() => 1 << 36,
        _ => 1 << 63,
    }
}

struct QueryWord {
    lower: Vec<u8>,
    mask: u64,
}

/// Reusable matcher; keeps its scratch buffers between calls so matching a
/// whole source allocates only for the returned ranges.
#[derive(Default)]
pub struct Matcher {
    query: Vec<char>,
    mask: u64,
    words: Vec<QueryWord>,
    hay: Vec<(u32, char, bool)>, // (byte offset, lowercase char, is word start)
}

impl Matcher {
    pub fn set_query(&mut self, query: &str) {
        self.query.clear();
        self.query
            .extend(query.chars().filter(|c| !c.is_whitespace()).map(lower));
        self.mask = char_mask(query);
        self.words = words(query)
            .map(|(_, w)| {
                let lower = w.to_lowercase().into_bytes();
                QueryWord { mask: byte_mask(&lower), lower }
            })
            .collect();
    }

    /// `score`, after a bitmask check that rejects entries missing any of
    /// the query's characters (a subsequence match needs all of them).
    pub fn score_masked(&mut self, haystack: &str, mask: u64) -> Option<Match> {
        if self.mask & !mask != 0 {
            return None;
        }
        self.score(haystack)
    }

    /// Word-level fallback, tolerant of a typo per word: "volumne control"
    /// -> "Volume mixer". Each query word must prefix (or nearly prefix) a
    /// word of the entry's title or keywords. Queries of up to two words
    /// must match every word; longer ones at least half. Only title words
    /// are highlighted.
    pub fn word_score(&self, p: &Prepared) -> Option<Match> {
        let n = self.words.len();
        if n == 0 || self.query.len() < 3 {
            return None;
        }
        let required = if n <= 2 { n } else { n.div_ceil(2) };
        let mut matched = 0;
        let mut score = 0i32;
        let mut ranges: Ranges = Vec::new();
        for (i, qw) in self.words.iter().enumerate() {
            let mut best: Option<(i32, &Word)> = None;
            for w in p.words.iter() {
                let q = word_quality(qw, w);
                // Title hits beat keyword hits of the same quality.
                let q = q * 2 + w.in_title as i32;
                if q > 1 && best.is_none_or(|b| q > b.0) {
                    best = Some((q, w));
                }
            }
            match best {
                Some((q, w)) => {
                    matched += 1;
                    score += q * 40;
                    if w.in_title {
                        ranges.push((w.start, w.end));
                    }
                }
                // Not enough words left to reach the requirement.
                None if n - i - 1 + matched < required => return None,
                None => {}
            }
        }
        if matched < required {
            return None;
        }
        ranges.sort_unstable();
        ranges.dedup();
        Some(Match { score: score - (n - matched) as i32 * 60, ranges })
    }

    pub fn query_is_empty(&self) -> bool {
        self.query.is_empty()
    }

    pub fn score(&mut self, haystack: &str) -> Option<Match> {
        let q = &self.query;
        if q.is_empty() {
            return None;
        }
        self.hay.clear();
        let mut prev: Option<char> = None;
        for (i, c) in haystack.char_indices() {
            let ws = match prev {
                None => true,
                Some(p) => !p.is_alphanumeric() || (p.is_lowercase() && c.is_uppercase()),
            };
            self.hay.push((i as u32, lower(c), ws));
            prev = Some(c);
        }
        let hay = &self.hay;
        if q.len() > hay.len() {
            return None;
        }
        let end_of = |k: usize| hay.get(k + 1).map_or(haystack.len() as u32, |h| h.0);
        let len_penalty = (hay.len() - q.len()) as i32;

        // 1. Contiguous substring: the strongest signal.
        if let Some(p) = find_sub(hay, q) {
            let mut score = 1000 - len_penalty;
            if p == 0 {
                score += 400;
            } else if hay[p].2 {
                score += 250;
            } else {
                score -= p as i32 * 2;
            }
            if q.len() == hay.len() {
                score += 300;
            }
            return Some(Match {
                score,
                ranges: vec![(hay[p].0, end_of(p + q.len() - 1))],
            });
        }

        // 2. Word-aware subsequence ("vsc" -> Visual Studio Code, "wifi" ->
        //    Wi-Fi): every matched char must start a word or directly follow
        //    the previous match. Scattered letters are noise, not matches.
        let mut score = 0i32;
        let mut ranges: Ranges = Vec::with_capacity(q.len());
        let mut pos = 0usize;
        let mut last: Option<usize> = None;
        for &qc in q.iter() {
            let chosen = match last {
                Some(l) if hay.get(l + 1).is_some_and(|h| h.1 == qc) => l + 1,
                _ => (pos..hay.len()).find(|&k| hay[k].1 == qc && hay[k].2)?,
            };
            score += 10;
            if hay[chosen].2 {
                score += 30;
            }
            if last.is_some_and(|l| l + 1 == chosen) {
                score += 20;
            } else if let Some(l) = last {
                score -= (chosen - l - 1).min(8) as i32;
            }
            let (s, e) = (hay[chosen].0, end_of(chosen));
            match ranges.last_mut() {
                Some(r) if r.1 == s => r.1 = e,
                _ => ranges.push((s, e)),
            }
            last = Some(chosen);
            pos = chosen + 1;
        }
        if hay[0].1 == q[0] {
            score += 40;
        }
        Some(Match {
            score: score - len_penalty / 2,
            ranges,
        })
    }
}

#[inline]
fn lower(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// (byte offset, word) for each alphanumeric run.
fn words(s: &str) -> impl Iterator<Item = (usize, &str)> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(move |w| (w.as_ptr() as usize - s.as_ptr() as usize, w))
}

/// 3 = prefix, 2 = prefix within the typo budget, 0 = no match.
fn word_quality(q: &QueryWord, w: &Word) -> i32 {
    let (q_bytes, w_bytes) = (q.lower.as_slice(), &*w.lower);
    if w_bytes.starts_with(q_bytes) {
        return 3;
    }
    let budget = match q_bytes.len() {
        0..=3 => return 0,
        4..=6 => 1,
        _ => 2,
    };
    // Each edit can drop at most one distinct character from the overlap.
    if (q.mask & w.mask).count_ones() + budget < q.mask.count_ones() {
        return 0;
    }
    // Compare against the word cut near the query's length, so partially
    // typed words and one extra or missing letter still line up.
    let lo = q_bytes.len().saturating_sub(1).min(w_bytes.len());
    let hi = (q_bytes.len() + 1).min(w_bytes.len());
    if (lo..=hi).any(|l| l >= 3 && edit_distance(q_bytes, &w_bytes[..l]) <= budget as usize) { 2 } else { 0 }
}

/// Levenshtein distance on short words, without heap allocation.
fn edit_distance(a: &[u8], b: &[u8]) -> usize {
    const MAX: usize = 48;
    if a.len() >= MAX || b.len() >= MAX {
        return usize::MAX;
    }
    let mut prev = [0usize; MAX];
    let mut cur = [0usize; MAX];
    for (j, p) in prev.iter_mut().enumerate().take(b.len() + 1) {
        *p = j;
    }
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + (ca != cb) as usize).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn find_sub(hay: &[(u32, char, bool)], q: &[char]) -> Option<usize> {
    if q.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - q.len()).find(|&i| q.iter().enumerate().all(|(j, &c)| hay[i + j].1 == c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(q: &str, h: &str) -> Option<Match> {
        let mut mm = Matcher::default();
        mm.set_query(q);
        mm.score(h)
    }

    #[test]
    fn prefix_beats_infix() {
        let a = m("cal", "Calculator").unwrap();
        let b = m("cal", "Local Security Policy").unwrap();
        assert!(a.score > b.score);
        assert_eq!(a.ranges, vec![(0, 3)]);
    }

    #[test]
    fn acronym_uses_word_starts() {
        let r = m("vsc", "Visual Studio Code").unwrap();
        assert_eq!(r.ranges, vec![(0, 1), (7, 8), (14, 15)]);
    }

    #[test]
    fn scattered_letters_rejected() {
        assert!(m("wifi", "Ori and the Will of the Wisps").is_none());
        assert!(m("main rs", "Marvel's Spider-Man Remastered").is_none());
        assert!(m("vsc", "RivaTuner Statistics Server").is_none());
    }

    #[test]
    fn word_chunks_match() {
        assert!(m("wifi", "Wi-Fi").is_some());
        assert!(m("vscode", "Visual Studio Code").is_some());
        assert!(m("wdf", "Windows Defender Firewall").is_some());
    }

    #[test]
    fn word_fallback_tolerates_typos() {
        let mut mm = Matcher::default();
        let p = Prepared::new;
        mm.set_query("volumne control");
        let r = mm.word_score(&p("Volume mixer", "volume control app sound levels")).unwrap();
        assert_eq!(r.ranges, vec![(0, 6)]);
        // Two-word queries need both words: "control" alone is too generic.
        assert!(mm.word_score(&p("Change User Account Control settings", "")).is_none());
        mm.set_query("volumne");
        assert!(mm.word_score(&p("Adjust system volume", "")).is_some());
        mm.set_query("environment variables");
        assert!(mm.word_score(&p("Edit the system environment variables", "")).is_some());
        mm.set_query("wallpaper");
        assert!(mm.word_score(&p("Background", "wallpaper desktop image")).is_some());
        mm.set_query("xyz abc");
        assert!(mm.word_score(&p("Adjust system volume", "")).is_none());
    }

    #[test]
    fn mask_rejects_missing_chars() {
        let mut mm = Matcher::default();
        mm.set_query("vsc");
        assert!(mm.score_masked("Calculator", char_mask("Calculator")).is_none());
        assert!(mm.score_masked("Visual Studio Code", char_mask("Visual Studio Code")).is_some());
    }

    #[test]
    fn no_match() {
        assert!(m("xyz", "Calculator").is_none());
    }

    #[test]
    fn case_and_spaces_ignored() {
        assert!(m("WIN term", "Windows Terminal").is_some());
    }

    #[test]
    fn exact_is_best() {
        let a = m("sound", "Sound").unwrap();
        let b = m("sound", "Sound Recorder").unwrap();
        assert!(a.score > b.score);
    }
}
