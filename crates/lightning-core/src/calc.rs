//! Calculator: a recursive-descent parser over `f64`, no dependencies.
//!
//! Only a query that parses as a whole *and* computes something (an operator
//! or a function call) is an answer, so `2024`, `7zip` and `5+` stay quiet.

/// Longest query considered; also bounds parenthesis nesting.
const MAX_LEN: usize = 256;

/// Whether the input is shaped like arithmetic: digits, operators and
/// brackets only (plus `of`), with at least one operator that isn't a lone
/// `-`, so `2024-10-24` stays a file name. Such input skips every other source.
pub fn is_math(query: &str) -> bool {
    let b = query.as_bytes();
    let (mut op, mut i) = (false, 0);
    while i < b.len() {
        match b[i] {
            b'0'..=b'9' | b'.' | b',' | b'-' | b')' | b' ' | b'\t' => {}
            b'+' | b'*' | b'/' | b'^' | b'%' | b'(' => op = true,
            b'o' | b'O' if matches!(b.get(i + 1), Some(b'f' | b'F')) => i += 1,
            // × is C3 97, ÷ is C3 B7.
            0xC3 if matches!(b.get(i + 1), Some(0x97 | 0xB7)) => {
                op = true;
                i += 1;
            }
            _ => return false,
        }
        i += 1;
    }
    op
}

/// Like [`eval`], but unfinished input (`355+`, `355+(2`) answers with what
/// it has so far, so the panel doesn't flicker while typing.
pub fn eval_partial(query: &str) -> Option<String> {
    if let Some(a) = eval(query) {
        return Some(a);
    }
    let mut q = query.trim_end();
    loop {
        let t = q.trim_end_matches(|c: char| c.is_whitespace() || "+-*/^×÷(".contains(c));
        let t = match t.len().checked_sub(2).and_then(|k| t.get(k..)) {
            Some(w) if w.eq_ignore_ascii_case("of") => &t[..t.len() - 2],
            _ => t,
        };
        if t.len() == q.len() {
            break;
        }
        q = t;
    }
    let open = q.matches('(').count().checked_sub(q.matches(')').count())?;
    if q.is_empty() || q.len() > MAX_LEN {
        return None;
    }
    let closed = format!("{q}{}", ")".repeat(open));
    // Nothing was missing, so `eval` already rejected it (`(5)` stays quiet).
    if closed == query.trim_end() {
        return None;
    }
    let mut p = Parser { s: &closed, i: 0, computed: false, pct: false };
    let v = p.expr()?.0;
    p.skip_ws();
    (p.i == closed.len()).then(|| format(v)).flatten()
}

/// The answer to `query`, formatted for display, if it is a calculation.
pub fn eval(query: &str) -> Option<String> {
    // Most queries are words: one byte scan rejects them before parsing.
    let b = query.as_bytes();
    if b.len() > MAX_LEN || !b.iter().any(|&c| matches!(c, b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'(') || c >= 0x80) {
        return None;
    }
    let mut p = Parser { s: query, i: 0, computed: false, pct: false };
    let v = p.expr()?.0;
    p.skip_ws();
    (p.i == query.len() && p.computed).then(|| format(v)).flatten()
}

type Func = fn(f64) -> f64;

const FUNCTIONS: [(&str, Func); 10] = [
    ("sqrt", f64::sqrt),
    ("abs", f64::abs),
    ("sin", |x| snap(x.sin())),
    ("cos", |x| snap(x.cos())),
    ("tan", |x| snap(x.tan())),
    ("ln", f64::ln),
    ("log", f64::log10),
    ("round", f64::round),
    ("floor", f64::floor),
    ("ceil", f64::ceil),
];

struct Parser<'a> {
    s: &'a str,
    i: usize,
    /// An operator or function was applied (not just a number or constant).
    computed: bool,
    /// The last operand ended in a postfix `%`.
    pct: bool,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }

    /// Consumes `c` (after whitespace) if it is next.
    fn eat(&mut self, c: char) -> bool {
        self.skip_ws();
        let hit = self.peek() == Some(c);
        if hit {
            self.i += c.len_utf8();
        }
        hit
    }

    /// Sum; the flag says the whole expression was a bare `B%`, so `A + B%`
    /// can take B percent *of A*.
    fn expr(&mut self) -> Option<(f64, bool)> {
        let (mut v, mut solo) = self.term()?;
        loop {
            let sub = if self.eat('+') {
                false
            } else if self.eat('-') {
                true
            } else {
                return Some((v, solo));
            };
            let (r, pct) = self.term()?;
            let r = if pct { v * r } else { r };
            v = if sub { v - r } else { v + r };
            solo = false;
            self.computed = true;
        }
    }

    fn term(&mut self) -> Option<(f64, bool)> {
        let mut v = self.unary()?;
        let mut solo = self.pct;
        loop {
            if self.eat('*') || self.eat('×') {
                v *= self.unary()?;
            } else if self.eat('/') || self.eat('÷') {
                v /= self.unary()?;
            } else if self.eat('%') {
                v %= self.unary()?;
            } else if solo && self.eat_of() {
                v *= self.unary()?;
            } else {
                return Some((v, solo));
            }
            solo = false;
            self.computed = true;
        }
    }

    /// The word `of`, after whitespace.
    fn eat_of(&mut self) -> bool {
        self.skip_ws();
        let hit = word_of(&self.s[self.i..]);
        if hit {
            self.i += 2;
        }
        hit
    }

    /// `%` is a percentage when nothing that could be an operand follows it,
    /// otherwise it is modulo (`10%3`).
    fn postfix_pct(&mut self) -> bool {
        let save = self.i;
        if !self.eat('%') {
            return false;
        }
        self.skip_ws();
        let rest = &self.s[self.i..];
        if rest.is_empty() || rest.starts_with([')', '+', '-', '*', '/', '^', '×', '÷']) || word_of(rest) {
            self.computed = true;
            return true;
        }
        self.i = save;
        false
    }

    fn unary(&mut self) -> Option<f64> {
        if self.eat('-') {
            Some(-self.unary()?)
        } else if self.eat('+') {
            self.unary()
        } else {
            let v = self.power()?;
            self.pct = self.postfix_pct();
            Some(if self.pct { v / 100.0 } else { v })
        }
    }

    /// Right-associative, and binds tighter than a leading minus: `-2^2 = -4`.
    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        if self.eat('^') {
            self.computed = true;
            return Some(base.powf(self.unary()?));
        }
        Some(base)
    }

    fn atom(&mut self) -> Option<f64> {
        self.skip_ws();
        let c = self.peek()?;
        if c == '(' {
            self.i += 1;
            let v = self.expr()?.0;
            return self.eat(')').then_some(v);
        }
        if c.is_ascii_digit() || c == '.' {
            return self.number();
        }
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        let name = &self.s[start..self.i];
        if name.eq_ignore_ascii_case("pi") {
            return Some(std::f64::consts::PI);
        }
        if name.eq_ignore_ascii_case("e") {
            return Some(std::f64::consts::E);
        }
        let (_, f) = FUNCTIONS.iter().find(|(n, _)| name.eq_ignore_ascii_case(n))?;
        if !self.eat('(') {
            return None;
        }
        let v = self.expr()?.0;
        self.eat(')').then(|| {
            self.computed = true;
            f(v)
        })
    }

    /// Digits with optional `1,000` grouping and a decimal part.
    /// Parsed in place; only grouped numbers are copied, to a stack buffer.
    fn number(&mut self) -> Option<f64> {
        let b = self.s.as_bytes();
        let digits = |mut i: usize| {
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            i
        };
        let start = self.i;
        let mut i = digits(start);
        let mut grouped = false;
        // A comma groups only when exactly three digits follow.
        while b.get(i) == Some(&b',')
            && b.get(i + 1..i + 4).is_some_and(|g| g.iter().all(u8::is_ascii_digit))
            && !b.get(i + 4).is_some_and(u8::is_ascii_digit)
        {
            grouped = true;
            i += 4;
        }
        if b.get(i) == Some(&b'.') {
            i = digits(i + 1);
        }
        self.i = i;
        let text = &b[start..i];
        if !grouped {
            return std::str::from_utf8(text).ok()?.parse().ok();
        }
        // `eval_partial` may append closing brackets, so up to twice MAX_LEN.
        let mut buf = [0u8; 2 * MAX_LEN];
        let mut n = 0;
        for &c in text.iter().filter(|&&c| c != b',') {
            buf[n] = c;
            n += 1;
        }
        std::str::from_utf8(&buf[..n]).ok()?.parse().ok()
    }
}

fn word_of(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[..2].eq_ignore_ascii_case(b"of") && !b.get(2).is_some_and(u8::is_ascii_alphabetic)
}

/// Trig of multiples of pi should read 0, not 1.2e-16.
fn snap(x: f64) -> f64 {
    if x.abs() < 1e-15 { 0.0 } else { x }
}

/// Up to 12 significant digits, trailing zeros trimmed; `None` if not finite.
fn format(v: f64) -> Option<String> {
    if !v.is_finite() {
        return None;
    }
    if v == 0.0 {
        return Some("0".into());
    }
    // "d.ddddddddddde±x": 12 significant digits, correctly rounded. Built on
    // the stack; the answer is the only allocation.
    use std::io::Write;
    let mut sci = [0u8; 32];
    let len = {
        let mut w = &mut sci[..];
        write!(w, "{:.11e}", v.abs()).ok()?;
        32 - w.len()
    };
    let sci = std::str::from_utf8(&sci[..len]).ok()?;
    let (mant, exp) = sci.split_once('e')?;
    let exp: i32 = exp.parse().ok()?;
    let (lead, frac) = mant.split_at(1);
    let frac = frac.trim_start_matches('.').trim_end_matches('0');
    let n = 1 + frac.len() as i32;
    let mut out = String::with_capacity(24);
    if v < 0.0 {
        out.push('-');
    }
    let zeros = |out: &mut String, k: i32| out.extend(std::iter::repeat_n('0', k as usize));
    if !(-7..15).contains(&exp) {
        out.push_str(lead);
        if !frac.is_empty() {
            out.push('.');
            out.push_str(frac);
        }
        out.push('e');
        out.push_str(&exp.to_string());
    } else if exp < 0 {
        out.push_str("0.");
        zeros(&mut out, -exp - 1);
        out.push_str(lead);
        out.push_str(frac);
    } else if n <= exp + 1 {
        out.push_str(lead);
        out.push_str(frac);
        zeros(&mut out, exp + 1 - n);
    } else {
        let (int, rest) = frac.split_at(exp as usize);
        out.push_str(lead);
        out.push_str(int);
        out.push('.');
        out.push_str(rest);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{eval, eval_partial, is_math};

    #[test]
    fn answers() {
        let cases = [
            ("5+2", "7"),
            (" 5 + 2 ", "7"),
            ("2*3+4", "10"),
            ("2+3*4", "14"),
            ("(2+3)*4", "20"),
            ("10/4", "2.5"),
            ("1/3", "0.333333333333"),
            ("0.1+0.2", "0.3"),
            ("10%3", "1"),
            ("2^10", "1024"),
            ("2^3^2", "512"),
            ("-2^2", "-4"),
            ("2^-1", "0.5"),
            ("2*-3", "-6"),
            ("--5+1", "6"),
            ("6×7", "42"),
            ("84÷2", "42"),
            ("1,000*2", "2000"),
            ("1,000,000/3", "333333.333333"),
            ("pi*2", "6.28318530718"),
            ("e^2", "7.38905609893"),
            ("sqrt(16)", "4"),
            ("SQRT(2)", "1.41421356237"),
            ("abs(-3)+1", "4"),
            ("sin(pi)", "0"),
            ("cos(0)+1", "2"),
            ("ln(e)", "1"),
            ("log(1000)", "3"),
            ("round(2.5)+0", "3"),
            ("floor(2.7)", "2"),
            ("ceil(2.1)", "3"),
            ("2*.5", "1"),
            ("123456789*1000", "123456789000"),
            ("1/8", "0.125"),
            ("2^60", "1.15292150461e18"),
            ("1/30000000", "3.33333333333e-8"),
            ("10^15", "1e15"),
            ("10^14", "100000000000000"),
            ("0*5", "0"),
            ("0*-5", "0"),
            ("300% of 2000", "6000"),
            ("20% of 150", "30"),
            ("20%of150", "30"),
            ("200 + 10%", "220"),
            ("200 - 10%", "180"),
            ("200+10%+10%", "242"),
            ("50%", "0.5"),
            ("5 %", "0.05"),
            ("200 * 10%", "20"),
            ("100/20%", "500"),
            ("10%3", "1"),
            ("10 % 3", "1"),
            ("10%(2+1)", "1"),
            ("(50%)*4", "2"),
        ];
        for (q, want) in cases {
            assert_eq!(eval(q).as_deref(), Some(want), "{q}");
        }
    }

    #[test]
    fn quiet() {
        let cases = [
            "", "   ", "2024", "3.14", "pi", "e", "7zip", "7-zip", "3d builder", "chrome", "5+", "+", "-5",
            "(5)", "(2", "2)", "1/0", "0/0", "5%0", "sqrt(-1)", "ln(0)", "10^999", "2^1024", "abs", "abs(",
            "sqrt 16", "foo(2)", "1,00+2", "5 5", "2++", "*2", "c:\\temp", "report-2024.pdf", "2 + two",
            "10% of", "of 5", "5 of 3", "10 of",
        ];
        for q in cases {
            assert_eq!(eval(q), None, "{q:?}");
        }
        assert_eq!(eval(&"(".repeat(300)), None);
    }

    #[test]
    fn math_mode() {
        for q in ["5+2", "355+", "355+(2", "(2", "300% of 2000", "20% of", "6×7", "1,000*2", "10%3", "5 %", "2^"] {
            assert!(is_math(q), "{q}");
        }
        for q in ["", "chrome", "2024", "2024-10-24", "1-2", "-5", "5-", "sqrt(4)", "pi*2", "7zip", "of", "2 of", "1+x"] {
            assert!(!is_math(q), "{q}");
        }
    }

    #[test]
    fn partial_answers() {
        let cases = [
            ("355+", Some("355")),
            ("355+(2", Some("357")),
            ("355 + ", Some("355")),
            ("(2", Some("2")),
            ("2*(3+", Some("6")),
            ("20% of", Some("0.2")),
            ("5+2", Some("7")),
            ("5/0", None),
            ("5/", Some("5")),
            ("(", None),
            ("+(", None),
            ("2)", None),
            ("(5)", None),
            ("5 + 2) ", None),
        ];
        for (q, want) in cases {
            assert_eq!(eval_partial(q).as_deref(), want, "{q}");
        }
    }
}
