//! The module's public functions, one per API. Each takes the call's
//! positional and keyword arguments as [`Value`]s and reproduces the shim's
//! order: `_check_builtin`, argument binding, `_text`, then `re`.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use indexmap::IndexMap;
use num_bigint::BigInt;
use num_traits::{FromPrimitive, Zero};

use crate::args::{self, ArgSource, Call, Signature};
use crate::error::{Error, Result};
use crate::flags;
use crate::matching::{self, Match, Mode};
use crate::pattern::Pattern;
use crate::pyrepr;
use crate::scan;
use crate::template::{self, Template};
use crate::text::{push_str, PyStr};
use crate::value::Value;

/// Characters `re.escape` prefixes with a backslash (`re._special_chars_map`).
const SPECIAL_CHARS: &str = "()[]{}?*+-|^$\\.&~# \t\n\r\x0b\x0c";

/// `re.escape(pattern)` for a str pattern.
pub fn escape_text(pattern: &[u32]) -> PyStr {
    // `SPECIAL_CHARS` as a bitmap over ASCII.
    const SPECIAL: u128 = {
        let bytes = SPECIAL_CHARS.as_bytes();
        let mut bits = 0u128;
        let mut i = 0;
        while i < bytes.len() {
            bits |= 1 << bytes[i];
            i += 1;
        }
        bits
    };
    let mut out = Vec::with_capacity(pattern.len() + pattern.len() / 4);
    for &c in pattern {
        if c < 0x80 && SPECIAL & (1u128 << c) != 0 {
            out.push(u32::from('\\'));
        }
        out.push(c);
    }
    out
}

/// `escape(pattern)`.
///
/// # Errors
/// `TypeError` for non-built-in arguments, bad arguments or a non-str pattern.
pub fn escape(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> = Signature::new("escape", &["pattern"], 1);
    let call = SIG.check_and_bind(args, kwargs)?;
    Ok(Value::Str(escape_text(args::text(
        call.required(0),
        "pattern",
    )?)))
}

/// The state of an `re.error` after `__init__(msg, pattern=None, pos=None)`.
///
/// Attributes that the original stores unchanged (`msg`, `pattern`, `pos`,
/// and `args[0]` when no formatting happens) are given as [`ArgSource`]s so
/// the bindings can hand back the very objects that were passed.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorInit {
    /// `self.msg`.
    pub msg: ArgSource,
    /// `self.pattern` (`None` when omitted).
    pub pattern: Option<ArgSource>,
    /// `self.pos` (`None` when omitted).
    pub pos: Option<ArgSource>,
    /// `(self.lineno, self.colno)`, set only when the message was formatted.
    pub location: Option<(BigInt, BigInt)>,
    /// `args[0]` when it was formatted; `None` means `args == (msg,)`.
    pub formatted: Option<PyStr>,
    /// `str(self)`; an error if `str(msg)` itself raises.
    pub text: Result<PyStr>,
}

/// `'%d' % value`.
fn percent_d(value: &Value) -> Result<String> {
    match value {
        Value::Bool(b) => Ok(if *b { "1" } else { "0" }.to_owned()),
        Value::Int(n) => pyrepr::int_text(n),
        Value::Float(f) if f.is_nan() => {
            Err(Error::Value("cannot convert float NaN to integer".into()))
        }
        Value::Float(f) if f.is_infinite() => Err(Error::Overflow(
            "cannot convert float infinity to integer".into(),
        )),
        Value::Float(f) => {
            let n = BigInt::from_f64(f.trunc()).unwrap_or_else(BigInt::zero);
            pyrepr::int_text(&n)
        }
        Value::Decimal(text) => {
            let lower = text.to_ascii_lowercase();
            let lower = lower.trim_start_matches(['-', '+']);
            if lower.contains("nan") {
                Err(Error::Value("cannot convert NaN to integer".into()))
            } else if lower.starts_with("inf") {
                Err(Error::Overflow("cannot convert Infinity to integer".into()))
            } else {
                // Only reached on the way to a TypeError from `count`.
                Ok(text.split(['.', 'e', 'E']).next().unwrap_or("0").to_owned())
            }
        }
        other => Err(Error::Type(format!(
            "%d format: a real number is required, not {}",
            other.type_name()
        ))),
    }
}

/// A Python slice bound `[0:pos]` clamped to `len`.
fn clamp_end(pos: &BigInt, len: usize) -> usize {
    let len_big = BigInt::from(len);
    let end = if pos.sign() == num_bigint::Sign::Minus {
        pos + &len_big
    } else {
        pos.clone()
    };
    if end.sign() == num_bigint::Sign::Minus {
        0
    } else if end > len_big {
        len
    } else {
        usize::try_from(&end).unwrap_or(len)
    }
}

/// `pattern.count(newline, 0, pos) + 1` and `pos - pattern.rfind(newline, 0, pos)`,
/// plus whether `newline in pattern`.
fn locate(pattern: &Value, pos: &Value) -> Result<(BigInt, BigInt, bool)> {
    let newlines: Vec<bool> = match pattern {
        Value::Str(s) => s.iter().map(|&c| c == 0x0A).collect(),
        Value::Bytes(b) | Value::ByteArray(b) => b.iter().map(|&c| c == b'\n').collect(),
        Value::List(_) | Value::Tuple(_) => {
            return Err(Error::Type(format!(
                "{}.count() takes exactly one argument (3 given)",
                pattern.type_name()
            )))
        }
        other => {
            return Err(Error::Attribute(format!(
                "'{}' object has no attribute 'count'",
                other.tp_name()
            )))
        }
    };
    let pos = match pos {
        Value::Int(n) => n.clone(),
        Value::Bool(b) => BigInt::from(u8::from(*b)),
        _ => {
            return Err(Error::Type(
                "slice indices must be integers or None or have an __index__ method".into(),
            ))
        }
    };
    let head = newlines
        .get(..clamp_end(&pos, newlines.len()))
        .unwrap_or(&[]);
    let lineno = BigInt::from(head.iter().filter(|&&nl| nl).count()) + 1;
    let rfind = head
        .iter()
        .rposition(|&nl| nl)
        .map_or_else(|| BigInt::from(-1), BigInt::from);
    let colno = pos - rfind;
    Ok((lineno, colno, newlines.contains(&true)))
}

/// `re.error(*args, **kwargs)`: `error.__init__(self, msg, pattern=None, pos=None)`.
/// The class isn't wrapped by `_builtins_only`, so any value is accepted.
///
/// # Errors
/// `TypeError` for bad arguments, and whatever the message formatting raises
/// (`%d` conversion, `count` on a non-text pattern, int digit limits).
pub fn error_init(args: &[Value], kwargs: &[(String, Value)]) -> Result<ErrorInit> {
    const SIG: Signature<'static> = Signature {
        name: "error.__init__",
        params: &["msg", "pattern", "pos"],
        required: 1,
        varargs: false,
        method: true,
    };
    let bound = SIG.bind(args, kwargs)?;
    let source = |i: usize| bound.slots.get(i).copied().flatten();
    let msg_source = source(0).unwrap_or(ArgSource::Positional(0));
    let msg = msg_source.get(args, kwargs).unwrap_or(&Value::None);
    let pattern = source(1)
        .and_then(|s| s.get(args, kwargs))
        .filter(|v| !matches!(v, Value::None));
    let pos = source(2)
        .and_then(|s| s.get(args, kwargs))
        .filter(|v| !matches!(v, Value::None));
    let mut init = ErrorInit {
        msg: msg_source,
        pattern: source(1),
        pos: source(2),
        location: None,
        formatted: None,
        text: Ok(PyStr::new()),
    };
    let (Some(pattern), Some(pos)) = (pattern, pos) else {
        init.text = pyrepr::str(msg);
        return Ok(init);
    };
    let mut text = pyrepr::str(msg)?;
    push_str(&mut text, &format!(" at position {}", percent_d(pos)?));
    let (lineno, colno, has_newline) = locate(pattern, pos)?;
    if has_newline {
        push_str(
            &mut text,
            &format!(
                " (line {}, column {})",
                pyrepr::int_text(&lineno)?,
                pyrepr::int_text(&colno)?
            ),
        );
    }
    init.location = Some((lineno, colno));
    init.formatted = Some(text.clone());
    init.text = Ok(text);
    Ok(init)
}

// ---------------------------------------------------------------------------
// compile and the compiled-pattern accessors

/// `re._MAXCACHE`: size of the LRU cache.
const MAXCACHE: usize = 512;
/// `re._MAXCACHE2`: size of the FIFO cache.
const MAXCACHE2: usize = 256;

/// A cache key: the pattern and the flags as a number (`2 == 2.0 == True`).
type CacheKey = (PyStr, BigInt);

/// A borrowed [`CacheKey`] for lookups; hashes exactly as the owned tuple.
#[derive(Hash)]
struct KeyRef<'a>(&'a [u32], &'a BigInt);

impl indexmap::Equivalent<CacheKey> for KeyRef<'_> {
    fn equivalent(&self, key: &CacheKey) -> bool {
        self.0 == key.0.as_slice() && *self.1 == key.1
    }
}

/// A multiply-rotate hasher (FxHash) for the pattern cache. Every
/// module-level call hashes its whole pattern, and SipHash dominated short
/// calls. Hash order never shows: `IndexMap` iterates in insertion order.
#[derive(Debug, Default, Clone, Copy)]
struct FxHasher(u64);

impl FxHasher {
    const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(Self::SEED);
    }
}

impl std::hash::Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (chunks, rest) = bytes.as_chunks::<8>();
        for chunk in chunks {
            self.add(u64::from_le_bytes(*chunk));
        }
        let mut tail = [0u8; 8];
        if let Some(dst) = tail.get_mut(..rest.len()) {
            dst.copy_from_slice(rest);
        }
        self.add(u64::from_le_bytes(tail) ^ ((rest.len() as u64) << 56));
    }

    fn write_u32(&mut self, n: u32) {
        self.add(u64::from(n));
    }

    fn write_u64(&mut self, n: u64) {
        self.add(n);
    }

    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

type FxBuild = std::hash::BuildHasherDefault<FxHasher>;

/// `re._cache` (LRU) and `re._cache2` (FIFO).
#[derive(Debug, Default)]
struct Cache {
    lru: IndexMap<CacheKey, Arc<Pattern>, FxBuild>,
    fifo: IndexMap<CacheKey, Arc<Pattern>, FxBuild>,
}

fn cache() -> MutexGuard<'static, Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE
        .get_or_init(|| Mutex::new(Cache::default()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `hash(value)`: `TypeError` for unhashable values.
pub(crate) fn check_hashable(value: &Value) -> Result<()> {
    match value {
        Value::List(_) | Value::Dict(_) | Value::Set(_) | Value::ByteArray(_) => Err(Error::Type(
            format!("unhashable type: '{}'", value.type_name()),
        )),
        Value::Tuple(items) => items.iter().try_for_each(check_hashable),
        _ => Ok(()),
    }
}

/// The number a flags value equals for dict lookups, if any.
fn numeric_key(flags: &Value) -> Option<BigInt> {
    match flags {
        Value::Int(n) => Some(n.clone()),
        Value::Bool(b) => Some(BigInt::from(u8::from(*b))),
        Value::Float(f) if f.is_finite() && f.fract() == 0.0 => BigInt::from_f64(*f),
        _ => None,
    }
}

/// `re._compile(pattern, flags)` for a str pattern: the cache replica, the
/// flags check (`flags & re.TEMPLATE`), and compilation. The same cached
/// [`Pattern`] is returned for equal keys, so callers can keep identity.
///
/// # Errors
/// `TypeError` for unhashable or non-int flags, then whatever compiling raises.
pub fn compile_cached(pattern: &[u32], flags_value: &Value) -> Result<Arc<Pattern>> {
    check_hashable(flags_value)?;
    let number = numeric_key(flags_value);
    if let Some(n) = &number {
        let lookup = KeyRef(pattern, n);
        let mut c = cache();
        if let Some(p) = c.fifo.get(&lookup) {
            return Ok(Arc::clone(p));
        }
        if let Some(p) = c.lru.shift_remove(&lookup) {
            insert(&mut c, (pattern.to_vec(), n.clone()), &p);
            return Ok(p);
        }
    }
    let int_flags = match flags_value {
        Value::Int(n) => n.clone(),
        Value::Bool(b) => BigInt::from(u8::from(*b)),
        other => {
            return Err(Error::Type(format!(
                "unsupported operand type(s) for &: '{}' and 'RegexFlag'",
                other.tp_name()
            )))
        }
    };
    let p = Arc::new(Pattern::compile(pattern, &int_flags)?);
    // `re._compile` doesn't cache DEBUG patterns.
    let debug = (&int_flags & BigInt::from(flags::DEBUG)) != BigInt::zero();
    if !debug {
        let mut c = cache();
        if c.lru.len() >= MAXCACHE {
            c.lru.shift_remove_index(0);
        }
        insert(&mut c, (pattern.to_vec(), int_flags), &p);
    }
    Ok(p)
}

fn insert(c: &mut Cache, key: CacheKey, p: &Arc<Pattern>) {
    c.lru.insert(key.clone(), Arc::clone(p));
    if c.fifo.len() >= MAXCACHE2 {
        c.fifo.shift_remove_index(0);
    }
    c.fifo.insert(key, Arc::clone(p));
}

/// `compile(pattern, flags=0)`.
///
/// # Errors
/// As `re.compile` behind the shim's checks.
pub fn compile(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> = Signature::new(stringify!(compile), &["pattern", "flags"], 1);
    let call = SIG.check_and_bind(args, kwargs)?;
    let zero = Value::Int(BigInt::zero());
    let flags_value = call.get(1).unwrap_or(&zero);
    let text = args::text(call.required(0), "pattern")?;
    Ok(Value::Pattern(compile_cached(text, flags_value)?))
}

/// Binds the single argument `p` of a `pattern_*` accessor.
fn pattern_arg<'v>(
    name: &'static str,
    attr: &str,
    args: &'v [Value],
    kwargs: &'v [(String, Value)],
) -> Result<&'v Arc<Pattern>> {
    let call = Signature::new(name, &["p"], 1).check_and_bind(args, kwargs)?;
    pattern_with(call.required(0), attr)
}

/// The shim's `p.METHOD` lookup: only a Pattern has the attribute.
fn pattern_with<'v>(p: &'v Value, method: &str) -> Result<&'v Arc<Pattern>> {
    match p {
        Value::Pattern(p) => Ok(p),
        other => Err(Error::Attribute(format!(
            "'{}' object has no attribute '{method}'",
            other.tp_name()
        ))),
    }
}

/// `pattern_text(p)`: `p.pattern`.
///
/// # Errors
/// `TypeError` for bad arguments, `AttributeError` for a non-Pattern.
pub fn pattern_text(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    let p = pattern_arg("pattern_text", "pattern", args, kwargs)?;
    Ok(Value::Str(p.pattern.clone()))
}

/// `pattern_flags(p)`: `p.flags`.
///
/// # Errors
/// `TypeError` for bad arguments, `AttributeError` for a non-Pattern.
pub fn pattern_flags(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    let p = pattern_arg("pattern_flags", "flags", args, kwargs)?;
    Ok(Value::Int(BigInt::from(p.flags)))
}

/// `pattern_groups(p)`: `p.groups`.
///
/// # Errors
/// `TypeError` for bad arguments, `AttributeError` for a non-Pattern.
pub fn pattern_groups(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    let p = pattern_arg("pattern_groups", "groups", args, kwargs)?;
    Ok(Value::Int(BigInt::from(p.groups)))
}

/// `pattern_groupindex(p)`: `dict(p.groupindex)`.
///
/// # Errors
/// `TypeError` for bad arguments, `AttributeError` for a non-Pattern.
pub fn pattern_groupindex(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    let p = pattern_arg("pattern_groupindex", "groupindex", args, kwargs)?;
    Ok(Value::Dict(
        p.groupindex
            .iter()
            .map(|(k, &v)| (Value::Str(k.clone()), Value::Int(BigInt::from(v))))
            .collect(),
    ))
}

// ---------------------------------------------------------------------------
// matching: module functions, pattern methods and match accessors

/// `PyNumber_AsSsize_t(value, OverflowError)` after `__index__`, as argument
/// clinic converts `pos` / `endpos`.
fn ssize_arg(value: &Value) -> Result<isize> {
    match value {
        Value::Bool(b) => Ok(isize::from(*b)),
        Value::Int(n) => isize::try_from(n)
            .map_err(|_| Error::Overflow("Python int too large to convert to C ssize_t".into())),
        other => Err(Error::Type(format!(
            "'{}' object cannot be interpreted as an integer",
            other.tp_name()
        ))),
    }
}

/// `re.search` / `re.match` / `re.fullmatch`.
fn module_match(
    name: &'static str,
    mode: Mode,
    args: &[Value],
    kwargs: &[(String, Value)],
) -> Result<Value> {
    let (p, string) = module_scan(name, args, kwargs)?;
    let len = string.len();
    Ok(Match::run(&p, &string, 0, len, mode)?.map_or(Value::None, |m| Value::Match(Arc::new(m))))
}

/// `search(pattern, string, flags=0)`.
///
/// # Errors
/// As `re.search` behind the shim's checks.
pub fn search(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    module_match("search", Mode::Search, args, kwargs)
}

/// `match(pattern, string, flags=0)`.
///
/// # Errors
/// As `re.match` behind the shim's checks.
pub fn r#match(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    module_match("match", Mode::Match, args, kwargs)
}

/// `fullmatch(pattern, string, flags=0)`.
///
/// # Errors
/// As `re.fullmatch` behind the shim's checks.
pub fn fullmatch(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    module_match("fullmatch", Mode::FullMatch, args, kwargs)
}

/// A pattern, its subject and the clamped `pos` / `endpos`.
pub type Window = (Arc<Pattern>, Arc<[u32]>, usize, usize);

/// The shim's `_window(p, method, string, pos, endpos)` after binding
/// `(p, string, pos=0, endpos=None)`: the pattern, the subject and the
/// clamped window.
///
/// # Errors
/// `TypeError` for bad arguments or a non-str subject, `AttributeError` for a
/// non-Pattern, and the `Py_ssize_t` conversion errors of `pos` / `endpos`.
pub fn window(
    name: &'static str,
    method: &str,
    args: &[Value],
    kwargs: &[(String, Value)],
) -> Result<Window> {
    let call =
        Signature::new(name, &["p", "string", "pos", "endpos"], 2).check_and_bind(args, kwargs)?;
    let string: Arc<[u32]> = args::text(call.required(1), "string")?.into();
    let p = pattern_with(call.required(0), method)?;
    let pos = call.get(2).map_or(Ok(0), ssize_arg)?;
    let endpos = match call.get(3) {
        None | Some(Value::None) => isize::MAX,
        Some(v) => ssize_arg(v)?,
    };
    let len = string.len();
    Ok((
        Arc::clone(p),
        string,
        matching::clamp(pos, len),
        matching::clamp(endpos, len),
    ))
}

fn pattern_window_match(
    name: &'static str,
    method: &str,
    mode: Mode,
    args: &[Value],
    kwargs: &[(String, Value)],
) -> Result<Value> {
    let (p, string, pos, endpos) = window(name, method, args, kwargs)?;
    Ok(Match::run(&p, &string, pos, endpos, mode)?
        .map_or(Value::None, |m| Value::Match(Arc::new(m))))
}

/// `pattern_search(p, string, pos=0, endpos=None)`.
///
/// # Errors
/// As [`window`].
pub fn pattern_search(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    pattern_window_match("pattern_search", "search", Mode::Search, args, kwargs)
}

/// `pattern_match(p, string, pos=0, endpos=None)`.
///
/// # Errors
/// As [`window`].
pub fn pattern_match(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    pattern_window_match("pattern_match", "match", Mode::Match, args, kwargs)
}

/// `pattern_fullmatch(p, string, pos=0, endpos=None)`.
///
/// # Errors
/// As [`window`].
pub fn pattern_fullmatch(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    pattern_window_match(
        "pattern_fullmatch",
        "fullmatch",
        Mode::FullMatch,
        args,
        kwargs,
    )
}

// ---------------------------------------------------------------------------
// findall, finditer and split

/// The shim's `findall` / `finditer`: binds `(pattern, string, flags=0)`,
/// checks both texts and compiles through the cache.
fn module_scan(
    name: &'static str,
    args: &[Value],
    kwargs: &[(String, Value)],
) -> Result<(Arc<Pattern>, Arc<[u32]>)> {
    let call =
        Signature::new(name, &["pattern", "string", "flags"], 2).check_and_bind(args, kwargs)?;
    let zero = Value::Int(BigInt::zero());
    let flags_value = call.get(2).unwrap_or(&zero);
    let pattern = args::text(call.required(0), "pattern")?;
    let string: Arc<[u32]> = args::text(call.required(1), "string")?.into();
    Ok((compile_cached(pattern, flags_value)?, string))
}

fn match_list(matches: Vec<Match>) -> Value {
    Value::List(
        matches
            .into_iter()
            .map(|m| Value::Match(Arc::new(m)))
            .collect(),
    )
}

/// `findall(pattern, string, flags=0)`.
///
/// # Errors
/// As `re.findall` behind the shim's checks.
pub fn findall(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    let (p, string) = module_scan("findall", args, kwargs)?;
    let len = string.len();
    Ok(Value::List(scan::findall(&p, &string, 0, len)?))
}

/// `finditer(pattern, string, flags=0)`: the matches as a list.
///
/// # Errors
/// As `re.finditer` behind the shim's checks.
pub fn finditer(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    let (p, string) = module_scan("finditer", args, kwargs)?;
    let len = string.len();
    Ok(match_list(scan::finditer(&p, &string, 0, len)?))
}

/// `pattern_findall(p, string, pos=0, endpos=None)`.
///
/// # Errors
/// As [`window`].
pub fn pattern_findall(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    let (p, string, pos, endpos) = window("pattern_findall", "findall", args, kwargs)?;
    Ok(Value::List(scan::findall(&p, &string, pos, endpos)?))
}

/// `pattern_finditer(p, string, pos=0, endpos=None)`: the matches as a list.
///
/// # Errors
/// As [`window`].
pub fn pattern_finditer(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    let (p, string, pos, endpos) = window("pattern_finditer", "finditer", args, kwargs)?;
    Ok(match_list(scan::finditer(&p, &string, pos, endpos)?))
}

/// `split(pattern, string, maxsplit=0, flags=0)`.
///
/// # Errors
/// As `re.split` behind the shim's checks: compile errors come before the
/// `maxsplit` conversion.
pub fn split(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> =
        Signature::new("split", &["pattern", "string", "maxsplit", "flags"], 2);
    let call = SIG.check_and_bind(args, kwargs)?;
    let zero = Value::Int(BigInt::zero());
    let maxsplit = call.get(2).unwrap_or(&zero);
    let flags_value = call.get(3).unwrap_or(&zero);
    let pattern = args::text(call.required(0), "pattern")?;
    let string = args::text(call.required(1), "string")?;
    let p = compile_cached(pattern, flags_value)?;
    let maxsplit = ssize_arg(maxsplit)?;
    Ok(Value::List(scan::split(&p, string, maxsplit)?))
}

/// `pattern_split(p, string, maxsplit=0)`.
///
/// # Errors
/// `TypeError` for bad arguments or a non-str subject, `AttributeError` for a
/// non-Pattern, then the `Py_ssize_t` conversion errors of `maxsplit`.
pub fn pattern_split(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> =
        Signature::new("pattern_split", &["p", "string", "maxsplit"], 2);
    let call = SIG.check_and_bind(args, kwargs)?;
    let string = args::text(call.required(1), "string")?;
    let p = pattern_with(call.required(0), "split")?;
    let maxsplit = call.get(2).map_or(Ok(0), ssize_arg)?;
    Ok(Value::List(scan::split(p, string, maxsplit)?))
}

/// A bound accessor call: the match and the bound arguments.
struct MatchCall<'v> {
    m: &'v Match,
    call: Call<'v>,
}

/// Binds `(m, *rest)` and checks `m` is a Match with attribute `attr`.
fn match_arg<'v>(
    sig: &Signature<'_>,
    attr: &str,
    args: &'v [Value],
    kwargs: &'v [(String, Value)],
) -> Result<MatchCall<'v>> {
    let call = sig.check_and_bind(args, kwargs)?;
    match call.required(0) {
        Value::Match(m) => Ok(MatchCall { m, call }),
        other => Err(missing_method(other, attr)),
    }
}

/// The error of `obj.attr(...)` on a non-Match: Pattern's data attributes exist
/// but aren't callable (`TypeError`); anything else lacks the attribute.
fn missing_method(obj: &Value, attr: &str) -> Error {
    let data_attr = match (obj, attr) {
        (Value::Pattern(_), "groups" | "flags") => Some("int"),
        (Value::Pattern(_), "pattern") => Some("str"),
        (Value::Pattern(_), "groupindex") => Some("mappingproxy"),
        _ => None,
    };
    match data_attr {
        Some(tp) => Error::Type(format!("'{tp}' object is not callable")),
        None => Error::Attribute(format!(
            "'{}' object has no attribute '{attr}'",
            obj.tp_name()
        )),
    }
}

const fn accessor(name: &'static str) -> Signature<'static> {
    Signature::new(name, &["m"], 1)
}

/// `match_group(m, *groups)`.
///
/// # Errors
/// `IndexError: no such group`, `TypeError` for unhashable names.
pub fn match_group(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> = Signature {
        name: "match_group",
        params: &["m"],
        required: 1,
        varargs: true,
        method: false,
    };
    let call = match_arg(&SIG, "group", args, kwargs)?;
    let groups = call.call.extra();
    match groups {
        [] => call.m.get_slice(&Value::Int(BigInt::zero()), &Value::None),
        [one] => call.m.get_slice(one, &Value::None),
        many => Ok(Value::Tuple(
            many.iter()
                .map(|g| call.m.get_slice(g, &Value::None))
                .collect::<Result<Vec<_>>>()?,
        )),
    }
}

/// `match_groups(m, default=None)`.
///
/// # Errors
/// `TypeError` for bad arguments, `AttributeError` for a non-Match.
pub fn match_groups(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> = Signature::new("match_groups", &["m", "default"], 1);
    let call = match_arg(&SIG, "groups", args, kwargs)?;
    let default = call.call.get(1).unwrap_or(&Value::None);
    Ok(Value::Tuple(
        (1..call.m.spans.len())
            .map(|g| {
                call.m
                    .group_text(g)
                    .map_or_else(|| default.clone(), Value::Str)
            })
            .collect(),
    ))
}

/// `match_groupdict(m, default=None)`.
///
/// # Errors
/// `TypeError` for bad arguments, `AttributeError` for a non-Match.
pub fn match_groupdict(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> = Signature::new("match_groupdict", &["m", "default"], 1);
    let call = match_arg(&SIG, "groupdict", args, kwargs)?;
    let default = call.call.get(1).unwrap_or(&Value::None);
    Ok(Value::Dict(
        call.m
            .pattern
            .groupindex
            .iter()
            .map(|(name, &g)| {
                let v = call
                    .m
                    .group_text(g)
                    .map_or_else(|| default.clone(), Value::Str);
                (Value::Str(name.clone()), v)
            })
            .collect(),
    ))
}

fn span_call(
    name: &'static str,
    attr: &str,
    args: &[Value],
    kwargs: &[(String, Value)],
) -> Result<(isize, isize)> {
    let sig = Signature::new(name, &["m", "group"], 1);
    let call = match_arg(&sig, attr, args, kwargs)?;
    let zero = Value::Int(BigInt::zero());
    let group = call.call.get(1).unwrap_or(&zero);
    let g = call.m.group_index(group)?;
    Ok(call.m.span_of(g))
}

/// `match_start(m, group=0)`.
///
/// # Errors
/// `IndexError: no such group`, `TypeError` for unhashable names.
pub fn match_start(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    let (a, _) = span_call("match_start", "start", args, kwargs)?;
    Ok(Value::Int(BigInt::from(a)))
}

/// `match_end(m, group=0)`.
///
/// # Errors
/// `IndexError: no such group`, `TypeError` for unhashable names.
pub fn match_end(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    let (_, b) = span_call("match_end", "end", args, kwargs)?;
    Ok(Value::Int(BigInt::from(b)))
}

/// `match_span(m, group=0)`.
///
/// # Errors
/// `IndexError: no such group`, `TypeError` for unhashable names.
pub fn match_span(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    let (a, b) = span_call("match_span", "span", args, kwargs)?;
    Ok(Value::Tuple(vec![
        Value::Int(BigInt::from(a)),
        Value::Int(BigInt::from(b)),
    ]))
}

/// `match_lastindex(m)`.
///
/// # Errors
/// `TypeError` for bad arguments, `AttributeError` for a non-Match.
pub fn match_lastindex(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> = accessor("match_lastindex");
    let call = match_arg(&SIG, "lastindex", args, kwargs)?;
    Ok(call
        .m
        .lastindex
        .map_or(Value::None, |i| Value::Int(BigInt::from(i))))
}

/// `match_lastgroup(m)`.
///
/// # Errors
/// `TypeError` for bad arguments, `AttributeError` for a non-Match.
pub fn match_lastgroup(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> = accessor("match_lastgroup");
    let call = match_arg(&SIG, "lastgroup", args, kwargs)?;
    Ok(call
        .m
        .lastgroup()
        .map_or(Value::None, |n| Value::Str(n.clone())))
}

/// `match_pos(m)`.
///
/// # Errors
/// `TypeError` for bad arguments, `AttributeError` for a non-Match.
pub fn match_pos(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> = accessor("match_pos");
    let call = match_arg(&SIG, "pos", args, kwargs)?;
    Ok(Value::Int(BigInt::from(call.m.pos)))
}

/// `match_endpos(m)`.
///
/// # Errors
/// `TypeError` for bad arguments, `AttributeError` for a non-Match.
pub fn match_endpos(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> = accessor("match_endpos");
    let call = match_arg(&SIG, "endpos", args, kwargs)?;
    Ok(Value::Int(BigInt::from(call.m.endpos)))
}

/// `match_string(m)`.
///
/// # Errors
/// `TypeError` for bad arguments, `AttributeError` for a non-Match.
pub fn match_string(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> = accessor("match_string");
    let call = match_arg(&SIG, "string", args, kwargs)?;
    Ok(Value::Str(call.m.string.to_vec()))
}

// ---------------------------------------------------------------------------
// substitution: sub / subn / pattern_sub / pattern_subn / match_expand

/// `pattern.sub` / `pattern.subn` after the shim's checks: the `count`
/// conversion, the template parse (before any matching), then the loop.
fn run_sub(
    p: &Pattern,
    repl: &[u32],
    string: &[u32],
    count: Option<&Value>,
    with_count: bool,
) -> Result<Value> {
    let count = match count {
        Some(v) => ssize_arg(v)?,
        None => 0,
    };
    let template = Template::parse(repl, p)?;
    let (out, n) = template::substitute(p, &template, string, count)?;
    Ok(if with_count {
        Value::Tuple(vec![Value::Str(out), Value::Int(BigInt::from(n))])
    } else {
        Value::Str(out)
    })
}

/// `re.sub` / `re.subn` behind the shim.
fn module_sub(
    name: &'static str,
    with_count: bool,
    args: &[Value],
    kwargs: &[(String, Value)],
) -> Result<Value> {
    let call = Signature::new(name, &["pattern", "repl", "string", "count", "flags"], 3)
        .check_and_bind(args, kwargs)?;
    let pattern = args::text(call.required(0), "pattern")?;
    let repl = args::text(call.required(1), "repl")?;
    let string = args::text(call.required(2), "string")?;
    let zero = Value::Int(BigInt::zero());
    let p = compile_cached(pattern, call.get(4).unwrap_or(&zero))?;
    run_sub(&p, repl, string, call.get(3), with_count)
}

/// `sub(pattern, repl, string, count=0, flags=0)`.
///
/// # Errors
/// As `re.sub` behind the shim's checks.
pub fn sub(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    module_sub("sub", false, args, kwargs)
}

/// `subn(pattern, repl, string, count=0, flags=0)`.
///
/// # Errors
/// As `re.subn` behind the shim's checks.
pub fn subn(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    module_sub("subn", true, args, kwargs)
}

/// `p.sub(_text(repl), _text(string), count)`: the attribute lookup on `p`
/// happens before the arguments are checked.
fn method_sub(
    name: &'static str,
    method: &str,
    with_count: bool,
    args: &[Value],
    kwargs: &[(String, Value)],
) -> Result<Value> {
    let call =
        Signature::new(name, &["p", "repl", "string", "count"], 3).check_and_bind(args, kwargs)?;
    let p = pattern_with(call.required(0), method)?;
    let repl = args::text(call.required(1), "repl")?;
    let string = args::text(call.required(2), "string")?;
    run_sub(p, repl, string, call.get(3), with_count)
}

/// `pattern_sub(p, repl, string, count=0)`.
///
/// # Errors
/// As `Pattern.sub` behind the shim's checks.
pub fn pattern_sub(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    method_sub("pattern_sub", "sub", false, args, kwargs)
}

/// `pattern_subn(p, repl, string, count=0)`.
///
/// # Errors
/// As `Pattern.subn` behind the shim's checks.
pub fn pattern_subn(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    method_sub("pattern_subn", "subn", true, args, kwargs)
}

/// `match_expand(m, template)`: `m.expand(_text(template, "template"))`.
///
/// # Errors
/// `AttributeError` for a non-Match, `TypeError` for a non-str template, and
/// the template's parse errors.
pub fn match_expand(args: &[Value], kwargs: &[(String, Value)]) -> Result<Value> {
    const SIG: Signature<'static> = Signature::new("match_expand", &["m", "template"], 2);
    let call = match_arg(&SIG, "expand", args, kwargs)?;
    let source = args::text(call.call.required(1), "template")?;
    let m = call.m;
    let template = Template::parse(source, &m.pattern)?;
    let mut out = PyStr::new();
    template.expand_into(&mut out, |g| {
        m.spans
            .get(g)
            .copied()
            .flatten()
            .and_then(|(a, b)| m.string.get(a..b))
    });
    Ok(Value::Str(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{cps, to_string_lossy};

    fn s(t: &str) -> Value {
        Value::Str(cps(t))
    }
    fn int(i: i64) -> Value {
        Value::Int(BigInt::from(i))
    }

    #[test]
    fn groups_on_pattern_is_not_callable() -> Result<()> {
        let p = compile_cached(&cps("(a)"), &int(0))?;
        let err = match_groups(&[Value::Pattern(p)], &[]);
        assert!(matches!(err, Err(Error::Type(ref m)) if m == "'int' object is not callable"));
        let err = match_groups(&[int(1)], &[]);
        assert!(matches!(err, Err(Error::Attribute(_))));
        Ok(())
    }
    fn text(args: &[Value]) -> String {
        match error_init(args, &[]) {
            Ok(init) => init.text.map(|t| to_string_lossy(&t)).unwrap_or_default(),
            Err(e) => format!("!{e}"),
        }
    }

    #[test]
    fn cache_returns_the_same_pattern() -> Result<()> {
        let pattern = cps("cache(d)+");
        let first = compile_cached(&pattern, &int(0))?;
        assert!(Arc::ptr_eq(&first, &compile_cached(&pattern, &int(0))?));
        assert!(Arc::ptr_eq(
            &first,
            &compile_cached(&pattern, &Value::Bool(false))?
        ));
        Ok(())
    }

    #[test]
    fn escapes() {
        assert_eq!(
            to_string_lossy(&escape_text(&cps("a.b c\u{e9}_"))),
            "a\\.b\\ c\u{e9}_"
        );
        for c in 0..0x200 {
            let special = SPECIAL_CHARS.chars().any(|s| u32::from(s) == c);
            let expected = if special {
                vec![u32::from('\\'), c]
            } else {
                vec![c]
            };
            assert_eq!(escape_text(&[c]), expected, "{c}");
        }
    }

    #[test]
    fn error_messages() {
        assert_eq!(
            text(&[s("m"), s("ab\ncd"), Value::Bool(true)]),
            "m at position 1 (line 1, column 2)"
        );
        assert_eq!(
            text(&[s("m"), s("a\nbc\n"), int(-3)]),
            "m at position -3 (line 2, column -4)"
        );
        assert_eq!(
            text(&[s("m"), Value::Bytes(b"a\nb".to_vec()), int(2)]),
            "m at position 2 (line 2, column 1)"
        );
        assert_eq!(
            text(&[s("m"), s("a"), Value::Float(1.5)]),
            "!slice indices must be integers or None or have an __index__ method"
        );
        assert_eq!(
            text(&[s("m"), s("a"), s("2")]),
            "!%d format: a real number is required, not str"
        );
        assert_eq!(
            text(&[s("m"), Value::List(vec![]), int(2)]),
            "!list.count() takes exactly one argument (3 given)"
        );
        assert_eq!(
            text(&[s("m"), int(1), int(2)]),
            "!'int' object has no attribute 'count'"
        );
        assert_eq!(
            text(&[Value::Tuple(vec![s("a")]), s("x"), int(1)]),
            "('a',) at position 1"
        );
        assert_eq!(
            text(&[s("m"), s("a"), Value::Float(f64::INFINITY)]),
            "!cannot convert float infinity to integer"
        );
        assert_eq!(text(&[Value::Set(vec![])]), "set()");
        assert_eq!(text(&[s("m"), s("a\nb"), Value::None]), "m");
    }
}
