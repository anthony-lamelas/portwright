//! Expected values come from CPython 3.12's `re`.

use super::*;

const I: Flags = Flags {
    ignorecase: true,
    dotall: false,
};
const NO: Flags = Flags {
    ignorecase: false,
    dotall: false,
};

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn search(p: &str, f: Flags, t: &str) -> Option<Vec<Option<(usize, usize)>>> {
    let r = Regex::new(p, f).ok()?;
    let t = chars(t);
    let c = r.search(&t, 0, t.len())?;
    Some((0..c.len()).map(|i| c.span(i)).collect())
}

fn span(p: &str, f: Flags, t: &str) -> Option<(usize, usize)> {
    search(p, f, t).and_then(|v| v.first().copied().flatten())
}

fn err(p: &str) -> String {
    match Regex::compile(p, NO) {
        Err(e) => e.to_string(),
        Ok(_) => String::new(),
    }
}

#[test]
fn ignorecase_special_cases() {
    assert_eq!(span("σ", I, "xς"), Some((1, 2)));
    assert_eq!(span("[σ]", I, "xΣ"), Some((1, 2)));
    assert_eq!(span("k", I, "\u{212A}"), Some((0, 1)));
    assert_eq!(span("[\u{212A}]", I, "k"), Some((0, 1)));
    assert_eq!(span("s", I, "ſ"), Some((0, 1)));
    assert_eq!(span("ß", I, "ẞ"), Some((0, 1)));
    assert_eq!(span("ß", I, "SS"), None);
    assert_eq!(span("[^k]", I, "K"), None);
    assert_eq!(span("(?-i:Z)", I, "z"), None);
}

#[test]
fn backrefs_and_groups() {
    assert_eq!(
        search(r"(a)\1", I, "aA"),
        Some(vec![Some((0, 2)), Some((0, 1))])
    );
    assert_eq!(
        search(r"(?:(a)b|(a)c)", NO, "ac"),
        Some(vec![Some((0, 2)), None, Some((0, 1))])
    );
    assert_eq!(
        search(r"(?:(a)b|(a)c)*", NO, "acab"),
        Some(vec![Some((0, 4)), Some((2, 3)), Some((0, 1))])
    );
    assert_eq!(span(r"(?P<x>a)(?P=x)", NO, "baa"), Some((1, 3)));
    assert_eq!(span(r"(a)?(?(1)b|c)", NO, "c"), Some((0, 1)));
    let r = Regex::new("(?P<a>x)(y)(?P<b>z)", NO).ok();
    assert_eq!(r.as_ref().map(Regex::groups), Some(3));
    assert_eq!(r.and_then(|r| r.groupindex().get("b").copied()), Some(3));
}

#[test]
fn anchors_pos_endpos() {
    let t = chars("abc\n");
    let r = Regex::new("c$", NO).ok();
    assert_eq!(
        r.and_then(|r| r.search(&t, 0, 4)).and_then(|c| c.span(0)),
        Some((2, 3))
    );
    let r = Regex::new(r"c\Z", NO).ok();
    assert!(r.as_ref().and_then(|r| r.search(&t, 0, 4)).is_none());
    assert_eq!(
        r.and_then(|r| r.search(&t, 0, 3)).and_then(|c| c.span(0)),
        Some((2, 3))
    );
    let r = Regex::new(r"\Ab", NO).ok();
    assert!(r.and_then(|r| r.search(&t, 1, 4)).is_none());
    let r = Regex::new("", NO).ok();
    assert!(r.as_ref().and_then(|r| r.search(&t, 3, 1)).is_none());
    assert_eq!(
        r.and_then(|r| r.match_at(&t, 2, 1)).and_then(|c| c.span(0)),
        Some((2, 2))
    );
    let r = Regex::new("a*", NO).ok();
    assert!(r.and_then(|r| r.match_at(&t, 2, 1)).is_none());
    assert_eq!(clamp_pos(-3, 99, 4), (0, 4));
}

#[test]
fn repeats_and_lookaround() {
    assert_eq!(span("x{", NO, "ax{"), Some((1, 3)));
    assert_eq!(span("a{2,3}", NO, "aaaa"), Some((0, 3)));
    assert_eq!(span("a{2,3}?", NO, "aaaa"), Some((0, 2)));
    assert_eq!(span("a*+a", NO, "aaa"), None);
    assert_eq!(span("(?>a*)b", NO, "aab"), Some((0, 3)));
    assert_eq!(span(r"(?<=\d{2})x", NO, "1x12x"), Some((4, 5)));
    assert_eq!(span(r"(?<!a)b", NO, "abcb"), Some((3, 4)));
    assert_eq!(span(r"(.*?)c(.*)", NO, "abcbc"), Some((0, 5)));
    assert_eq!(span(r"\d+", NO, "x١٢3"), Some((1, 4)));
    let r = Regex::new(r"(?:(a)|b)*", NO).ok();
    let t = chars("ab");
    assert_eq!(
        r.and_then(|r| r.fullmatch(&t, 0, 2))
            .and_then(|c| c.span(1)),
        Some((0, 1))
    );
}

#[test]
fn error_messages() {
    assert_eq!(err("("), "missing ), unterminated subpattern at position 0");
    assert_eq!(err(r"\q"), r"bad escape \q at position 0");
    assert_eq!(err("a**"), "multiple repeat at position 2");
    assert_eq!(err("*"), "nothing to repeat at position 0");
    assert_eq!(err("[a"), "unterminated character set at position 0");
    assert_eq!(
        err("(?P<a²>x)"),
        "bad character in group name 'a²' at position 4"
    );
    assert_eq!(err("(?P=b)"), "unknown group name 'b' at position 4");
    assert_eq!(
        err("(?P<B>x)(?P<B>y)"),
        "redefinition of group name 'B' as group 2; was group 1 at position 12"
    );
    assert_eq!(err("(?<=a*)b"), "look-behind requires fixed-width pattern");
    assert_eq!(
        err("ab\ncd("),
        "missing ), unterminated subpattern at position 5 (line 2, column 3)"
    );
    assert!(matches!(
        Regex::compile("a{4294967295}", NO),
        Err(crate::Error::Overflow(_))
    ));
    assert!(matches!(
        Regex::compile("(?a)(?u)", NO),
        Err(crate::Error::Value(_))
    ));
}

#[test]
fn exponential_backtracking_is_memoised() {
    // sre is exponential here (0.5s on 8 chars); failure memoisation keeps
    // the same answer in polynomial time.
    let long = "a".repeat(40);
    let dotall = Flags {
        ignorecase: false,
        dotall: true,
    };
    assert_eq!(
        span("(|.\u{fffd}|(|.)+()?+)+()?+\u{670}", dotall, &long),
        None
    );
    assert_eq!(span(r"(a|aa)+\1?b", NO, &long), None);
    assert_eq!(span(r"(a*)*(?:x|\1)y", NO, &long), None);
    let mut ok = long.clone();
    ok.push('b');
    assert_eq!(span(r"(a|aa)+\1?b", NO, &ok), Some((0, 41)));
}

#[test]
fn deep_nesting_is_recursion_error() {
    let nest = |n: usize| format!("{}a{}", "(".repeat(n), ")".repeat(n));
    // `nested` is 2 per group level; the limit is the deepest allowed.
    assert!(Regex::compile_nested(&nest(10), NO, 20).is_ok());
    let e = Regex::compile_nested(&nest(11), NO, 20).err();
    assert!(e.as_ref().is_some_and(super::is_recursion));
    // Deep enough to overflow the Rust stack without the limit. The default
    // limit (493 group levels, plus the one `parse` wraps around a custom pattern) needs about 1 MB of stack in release builds
    // and more in debug ones, so run it on a thread sized like Python's main.
    let deep = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let ok = |p: &str| Regex::compile(p, NO).is_ok();
            let rec = |p: &str| {
                Regex::compile(p, NO)
                    .err()
                    .is_some_and(|e| super::is_recursion(&e))
            };
            ok(&nest(493))
                && rec(&nest(494))
                && rec(&nest(20000))
                && rec(&nest(20000).replace('(', "(?:"))
        });
    assert!(deep.is_ok_and(|h| h.join().is_ok_and(|b| b)));
}

#[test]
fn surrogate_standins_round_trip() {
    use super::unicode15 as u;
    for cp in [0xD800, 0xDBFF, 0xDC00, 0xDFFF] {
        let c = u::surrogate_standin(cp);
        assert_eq!(c.and_then(u::standin_surrogate), Some(cp));
    }
    assert_eq!(u::surrogate_standin(0xE000), None);
    assert_eq!(u::standin_surrogate('a'), None);
    let s: String = u::surrogate_standin(0xD800).into_iter().collect();
    assert_eq!(u::repr(&format!("x{s}")), "'x\\ud800'");
}
