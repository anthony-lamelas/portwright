//! Replacement templates: `re._parser.parse_template` (CPython 3.12),
//! `_sre.template` and `expand_template`, plus the `pattern_subx` loop.

use num_bigint::BigInt;

use crate::error::{Error, ReError, Result};
use crate::pattern::Pattern;
use crate::pyrepr;
use crate::scan::Scanner;
use crate::sre::MAXGROUPS;
use crate::text::{cps, push_str, PyStr};
use crate::unicode;

/// `sys.get_int_max_str_digits()` default.
const INT_MAX_STR_DIGITS: usize = 4300;

const BACKSLASH: u32 = b'\\' as u32;

/// One piece of a parsed template.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Chunk {
    /// Literal text.
    Literal(PyStr),
    /// A group reference (`\1`, `\g<name>`).
    Group(usize),
}

/// A parsed replacement template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    chunks: Vec<Chunk>,
}

/// `_parser.Tokenizer` over a str template: tokens are one code point, or a
/// backslash and the code point after it.
struct Tokenizer<'a> {
    string: &'a [u32],
    index: usize,
    /// `self.next` as a range into `string`.
    next: Option<(usize, usize)>,
}

impl<'a> Tokenizer<'a> {
    fn new(string: &'a [u32]) -> Result<Self> {
        let mut t = Self {
            string,
            index: 0,
            next: None,
        };
        t.advance()?;
        Ok(t)
    }

    /// `Tokenizer.__next`.
    fn advance(&mut self) -> Result<()> {
        let mut index = self.index;
        let Some(&c) = self.string.get(index) else {
            self.next = None;
            return Ok(());
        };
        let start = index;
        if c == BACKSLASH {
            index += 1;
            if index >= self.string.len() {
                return Err(ReError::new(
                    cps("bad escape (end of pattern)"),
                    self.string,
                    self.string.len().saturating_sub(1),
                )
                .into());
            }
        }
        self.index = index + 1;
        self.next = Some((start, self.index));
        Ok(())
    }

    fn peek(&self) -> Option<&'a [u32]> {
        self.next.and_then(|(a, b)| self.string.get(a..b))
    }

    /// `Tokenizer.get`.
    fn get(&mut self) -> Result<Option<&'a [u32]>> {
        let this = self.peek();
        self.advance()?;
        Ok(this)
    }

    /// `Tokenizer.match(char)` for a single code point.
    fn eat(&mut self, c: u32) -> Result<bool> {
        if self.peek() == Some(&[c][..]) {
            self.advance()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Whether `self.next` is a single code point in `lo..=hi`.
    fn next_in(&self, lo: u8, hi: u8) -> bool {
        matches!(self.peek(), Some(&[c]) if (u32::from(lo)..=u32::from(hi)).contains(&c))
    }

    /// `Tokenizer.tell`.
    fn tell(&self) -> usize {
        self.index - self.next.map_or(0, |(a, b)| b - a)
    }

    /// `Tokenizer.error(msg, offset)`.
    fn error(&self, msg: PyStr, offset: usize) -> Error {
        ReError::new(msg, self.string, self.tell().saturating_sub(offset)).into()
    }

    /// `Tokenizer.getuntil(terminator, name)`.
    fn getuntil(&mut self, terminator: u32, name: &str) -> Result<PyStr> {
        let mut result = PyStr::new();
        loop {
            let c = self.peek();
            self.advance()?;
            match c {
                None => {
                    if result.is_empty() {
                        return Err(self.error(cps(&format!("missing {name}")), 0));
                    }
                    let t = char::from_u32(terminator).unwrap_or('?');
                    return Err(self.error(
                        cps(&format!("missing {t}, unterminated name")),
                        result.len(),
                    ));
                }
                Some(&[c]) if c == terminator => {
                    if result.is_empty() {
                        return Err(self.error(cps(&format!("missing {name}")), 1));
                    }
                    return Ok(result);
                }
                Some(tok) => result.extend_from_slice(tok),
            }
        }
    }
}

fn invalid_group_reference(index: &str) -> PyStr {
    cps(&format!("invalid group reference {index}"))
}

/// `chr(c)` for a single-character ESCAPES entry.
fn simple_escape(c: u32) -> Option<u32> {
    let mapped = match char::from_u32(c)? {
        'a' => 0x07,
        'b' => 0x08,
        'f' => 0x0C,
        'n' => 0x0A,
        'r' => 0x0D,
        't' => 0x09,
        'v' => 0x0B,
        '\\' => BACKSLASH,
        _ => return None,
    };
    Some(mapped)
}

fn digits_value(digits: &[u32], radix: u32) -> u32 {
    digits
        .iter()
        .fold(0, |acc, &d| acc * radix + (d - u32::from(b'0')))
}

fn is_digit(c: u32) -> bool {
    (u32::from(b'0')..=u32::from(b'9')).contains(&c)
}

fn is_octdigit(c: u32) -> bool {
    (u32::from(b'0')..=u32::from(b'7')).contains(&c)
}

impl Template {
    /// `_parser.parse_template(source, pattern)`.
    ///
    /// # Errors
    /// `re.error` for malformed templates, `IndexError` for unknown group
    /// names, and `ValueError` for group numbers past Python's int digit limit.
    pub fn parse(source: &[u32], pattern: &Pattern) -> Result<Self> {
        let mut s = Tokenizer::new(source)?;
        let mut chunks = Vec::new();
        let mut literal = PyStr::new();
        let groups = pattern.groups;
        let mut add_group =
            |s: &Tokenizer<'_>, literal: &mut PyStr, index: usize, offset: usize| -> Result<()> {
                if index > groups {
                    return Err(s.error(invalid_group_reference(&index.to_string()), offset));
                }
                chunks.push(Chunk::Literal(std::mem::take(literal)));
                chunks.push(Chunk::Group(index));
                Ok(())
            };
        while let Some(this) = s.get()? {
            let [BACKSLASH, c] = *this else {
                literal.extend_from_slice(this);
                continue;
            };
            if c == u32::from(b'g') {
                if !s.eat(u32::from(b'<'))? {
                    return Err(s.error(cps("missing <"), 0));
                }
                let name = s.getuntil(u32::from(b'>'), "group name")?;
                let offset = name.len() + 1;
                let index = if name.iter().all(|&c| is_digit(c)) {
                    if name.len() > INT_MAX_STR_DIGITS {
                        return Err(Error::Value(format!(
                            "Exceeds the limit ({INT_MAX_STR_DIGITS} digits) for integer \
                             string conversion: value has {} digits; use \
                             sys.set_int_max_str_digits() to increase the limit",
                            name.len()
                        )));
                    }
                    let text: String = name.iter().filter_map(|&c| char::from_u32(c)).collect();
                    let value = text.parse::<BigInt>().unwrap_or_default();
                    match usize::try_from(&value) {
                        Ok(v) if v < MAXGROUPS => v,
                        _ => {
                            return Err(s.error(
                                invalid_group_reference(&pyrepr::int_text(&value)?),
                                offset,
                            ))
                        }
                    }
                } else {
                    if !unicode::is_identifier(&name) {
                        let mut msg = cps("bad character in group name ");
                        msg.extend(pyrepr::str_repr(&name));
                        return Err(s.error(msg, offset));
                    }
                    match pattern.groupindex.get(&name) {
                        Some(&g) => g,
                        None => {
                            let mut msg = cps("unknown group name ");
                            msg.extend(pyrepr::str_repr(&name));
                            return Err(Error::Index(crate::text::to_string_lossy(&msg)));
                        }
                    }
                };
                add_group(&s, &mut literal, index, offset)?;
            } else if c == u32::from(b'0') {
                let mut digits = vec![c];
                if s.next_in(b'0', b'7') {
                    digits.extend_from_slice(s.get()?.unwrap_or_default());
                    if s.next_in(b'0', b'7') {
                        digits.extend_from_slice(s.get()?.unwrap_or_default());
                    }
                }
                literal.push(digits_value(&digits, 8) & 0xFF);
            } else if is_digit(c) {
                let mut digits = vec![c];
                let mut isoctal = false;
                if s.next_in(b'0', b'9') {
                    digits.extend_from_slice(s.get()?.unwrap_or_default());
                    if is_octdigit(c)
                        && digits.get(1).is_some_and(|&d| is_octdigit(d))
                        && s.next_in(b'0', b'7')
                    {
                        digits.extend_from_slice(s.get()?.unwrap_or_default());
                        isoctal = true;
                        let value = digits_value(&digits, 8);
                        if value > 0o377 {
                            let mut msg = cps("octal escape value \\");
                            msg.extend_from_slice(&digits);
                            push_str(&mut msg, " outside of range 0-0o377");
                            return Err(s.error(msg, digits.len() + 1));
                        }
                        literal.push(value);
                    }
                }
                if !isoctal {
                    let index = usize::try_from(digits_value(&digits, 10)).unwrap_or(usize::MAX);
                    add_group(&s, &mut literal, index, digits.len())?;
                }
            } else if let Some(mapped) = simple_escape(c) {
                literal.push(mapped);
            } else if c < 0x80 && char::from_u32(c).is_some_and(|ch| ch.is_ascii_alphabetic()) {
                let mut msg = cps("bad escape ");
                msg.extend_from_slice(this);
                return Err(s.error(msg, this.len()));
            } else {
                literal.extend_from_slice(this);
            }
        }
        chunks.push(Chunk::Literal(literal));
        Ok(Self { chunks })
    }

    /// Whether the template is a single literal.
    pub fn literal(&self) -> Option<&[u32]> {
        match self.chunks.as_slice() {
            [Chunk::Literal(l)] => Some(l),
            _ => None,
        }
    }

    /// `expand_template`: appends the expansion to `out`, reading group `g`'s
    /// text through `group` (`None` for groups that didn't participate).
    pub fn expand_into<'s>(&self, out: &mut PyStr, group: impl Fn(usize) -> Option<&'s [u32]>) {
        for chunk in &self.chunks {
            match chunk {
                Chunk::Literal(l) => out.extend_from_slice(l),
                Chunk::Group(g) => {
                    if let Some(text) = group(*g) {
                        out.extend_from_slice(text);
                    }
                }
            }
        }
    }
}

/// `pattern_subx` with a template: replaces up to `count` matches (all when
/// `count == 0`, none when negative) and returns the result and the number
/// of replacements.
pub fn substitute(
    pattern: &Pattern,
    template: &Template,
    string: &[u32],
    count: isize,
) -> Result<(PyStr, usize)> {
    let len = string.len();
    let mut out = PyStr::with_capacity(len);
    let mut i = 0;
    let mut n: usize = 0;
    let mut scanner = Scanner::new(pattern, string, 0, len);
    let count = usize::try_from(count).ok();
    loop {
        match count {
            None => break,
            Some(c) if c != 0 && n >= c => break,
            _ => {}
        }
        if !scanner.next_match()? {
            break;
        }
        let state = &scanner.state;
        let (b, e) = (state.start, state.ptr);
        if i < b {
            out.extend_from_slice(string.get(i..b).unwrap_or_default());
        }
        match template.literal() {
            Some(l) => out.extend_from_slice(l),
            None => template.expand_into(&mut out, |g| {
                let span = if g == 0 {
                    Some((b, e))
                } else {
                    state.group_span(g)
                };
                span.and_then(|(x, y)| string.get(x..y))
            }),
        }
        i = e;
        n += 1;
        scanner.advance();
    }
    if i < len {
        out.extend_from_slice(string.get(i..).unwrap_or_default());
    }
    Ok((out, n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::to_string_lossy;

    fn sub(p: &str, t: &str, s: &str, count: isize) -> String {
        let pat = match Pattern::compile(&cps(p), &BigInt::from(0)) {
            Ok(p) => p,
            Err(e) => return format!("!{e}"),
        };
        match Template::parse(&cps(t), &pat) {
            Ok(tpl) => match substitute(&pat, &tpl, &cps(s), count) {
                Ok((out, n)) => format!("{} {n}", to_string_lossy(&out)),
                Err(e) => format!("!{e}"),
            },
            Err(e) => format!("!{e}"),
        }
    }

    #[test]
    fn basics() {
        assert_eq!(sub("x*", "-", "abxd", 0), "-a-b--d- 5");
        assert_eq!(sub("(a)|b", "[\\1]", "ab", 0), "[a][] 2");
        assert_eq!(sub("a", "\\n\\0\\101\\.", "a", 0), "\n\0A\\. 1");
        assert_eq!(sub("a", "x", "aaa", 2), "xxa 2");
        assert_eq!(sub("a", "x", "aaa", -1), "aaa 0");
        assert_eq!(sub("(?P<n>a)", "\\g<n>\\g<1>\\g<0>", "a", 0), "aaa 1");
        assert_eq!(sub("a", "\\g<_x>", "a", 0), "!unknown group name '_x'");
        assert_eq!(
            sub("a", "\\777", "a", 0),
            "!octal escape value \\777 outside of range 0-0o377 at position 0"
        );
        assert_eq!(
            sub("a", "\\8", "a", 0),
            "!invalid group reference 8 at position 1"
        );
        assert_eq!(
            sub("a", "a\\g<1>\\", "a", 0),
            "!bad escape (end of pattern) at position 6"
        );
        assert_eq!(
            sub("a", "\\g<a", "a", 0),
            "!missing >, unterminated name at position 3"
        );
    }

    #[test]
    fn edges() {
        let zeros = "0".repeat(4300);
        assert_eq!(
            sub("(a)", &format!("\\g<{zeros}1>"), "ab", 0),
            "!Exceeds the limit (4300 digits) for integer string conversion: value has \
             4301 digits; use sys.set_int_max_str_digits() to increase the limit"
        );
        assert_eq!(
            sub("(a)", &format!("\\g<{}1>", &zeros[1..]), "ab", 0),
            "ab 1"
        );
        assert_eq!(
            sub("(a)", "\\g<1073741823>", "ab", 0),
            "!invalid group reference 1073741823 at position 3"
        );
        assert_eq!(sub("(a)", "\\08", "ab", 0), "\u{0}8b 1");
        assert_eq!(
            sub("(a)", "\\128", "ab", 0),
            "!invalid group reference 12 at position 1"
        );
        assert_eq!(sub("(a)", "\\1a", "ab", 0), "aab 1");
        assert_eq!(sub("(a)", "\\0777", "ab", 0), "?7b 1");
        assert_eq!(
            sub("(a)", "\\g<a\\", "ab", 0),
            "!bad escape (end of pattern) at position 4"
        );
        assert_eq!(
            sub("(a)", "\\g<>x", "ab", 0),
            "!missing group name at position 3"
        );
        assert_eq!(sub("(a)", "\\\u{fc}", "ab", 0), "\\\u{fc}b 1");
        assert_eq!(
            sub("(a)", "\\g< 1>", "ab", 0),
            "!bad character in group name ' 1' at position 3"
        );
    }
}
