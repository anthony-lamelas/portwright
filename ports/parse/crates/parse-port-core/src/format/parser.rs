//! Upstream `Parser`: turns a format string into a Python-syntax regex plus
//! per-group conversions, and compiles it lazily with [`crate::pyre`].

use std::any::Any;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use indexmap::{Equivalent, IndexMap};

use super::percent::percent_format;
use super::spec::{extract_format, has_datetime_directive, FormatSpec};
use crate::error::{Error, Result};
use crate::pyre::cache::BoundedCache;
use crate::pyre::dt_format::get_regex_for_datetime_format;
use crate::pyre::unicode15 as pystr;
use crate::pyre::{Flags, Regex, DEFAULT_MAX_NESTED};

const DAYS_PAT: &str = r"(Mon|Tue|Wed|Thu|Fri|Sat|Sun)";
const MONTHS_PAT: &str = r"(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)";
const ALL_MONTHS_PAT: &str = "(Jan|January|Feb|February|Mar|March|Apr|April|May|Jun|June|Jul|July|Aug|August|Sep|September|Oct|October|Nov|November|Dec|December)";
const TIME_PAT: &str = r"(\d{1,2}:\d{1,2}(:\d{1,2}(\.\d+)?)?)";
const AM_PAT: &str = r"(\s+[AP]M)";
const TZ_PAT: &str = r"(\s+[-+]\d\d?:?\d\d)";

/// What a user-supplied converter contributes to the expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraType {
    /// `str(getattr(conv, "pattern", ".+?"))`
    pub pattern: String,
    /// `getattr(conv, "regex_group_count", 0)`, with `None` as 0.
    pub group_count: i64,
}

/// Access to the `extra_types` mapping while building the expression.
pub trait ExtraTypes {
    /// Error type; core errors convert into it.
    type Error: From<Error>;
    /// `name in extra_types`
    ///
    /// # Errors
    /// Whatever the mapping raises.
    fn contains(&mut self, name: &str) -> Result<bool, Self::Error>;
    /// Pattern and group count of `extra_types[name]`.
    ///
    /// # Errors
    /// Whatever the lookup raises.
    fn get(&mut self, name: &str) -> Result<ExtraType, Self::Error>;
}

/// A plain map of extra types.
impl ExtraTypes for IndexMap<String, ExtraType> {
    type Error = Error;
    fn contains(&mut self, name: &str) -> Result<bool> {
        Ok(self.contains_key(name))
    }
    fn get(&mut self, name: &str) -> Result<ExtraType> {
        IndexMap::get(self, name)
            .cloned()
            .ok_or_else(|| Error::Key(name.to_owned()))
    }
}

/// Key of `_type_conversions`: a fixed group index or a named group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupKey {
    /// Fixed field, keyed by its group index.
    Index(i64),
    /// Named field, keyed by its regex group name.
    Name(String),
}

// Written out so that [`NameKey`] hashes exactly like `GroupKey::Name`.
impl Hash for GroupKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            GroupKey::Index(i) => {
                0u8.hash(state);
                i.hash(state);
            }
            GroupKey::Name(n) => NameKey(n).hash(state),
        }
    }
}

/// A borrowed `GroupKey::Name`, to look one up without allocating.
struct NameKey<'a>(&'a str);

impl Hash for NameKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        1u8.hash(state);
        self.0.hash(state);
    }
}

impl Equivalent<GroupKey> for NameKey<'_> {
    fn equivalent(&self, key: &GroupKey) -> bool {
        matches!(key, GroupKey::Name(n) if n == self.0)
    }
}

/// Keyword arguments of the `date_convert` partial (group indexes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DateKw {
    /// `ymd=`
    pub ymd: Option<i64>,
    /// `mdy=`
    pub mdy: Option<i64>,
    /// `dmy=`
    pub dmy: Option<i64>,
    /// `d_m_y=(d, m, y)`
    pub d_m_y: Option<[i64; 3]>,
    /// `hms=`
    pub hms: Option<i64>,
    /// `am=`
    pub am: Option<i64>,
    /// `tz=`
    pub tz: Option<i64>,
    /// `mm=`
    pub mm: Option<i64>,
    /// `dd=`
    pub dd: Option<i64>,
}

impl DateKw {
    /// Whether `date_convert` may read `groups[i]` of a `len`-tuple: some
    /// keyword names `i` directly or as a negative index. Other groups are
    /// never read.
    pub fn reads_group(&self, i: usize, len: usize) -> bool {
        let hits = |k: i64| {
            let k = if k < 0 {
                i64::try_from(len).map_or(k, |len| k.saturating_add(len))
            } else {
                k
            };
            usize::try_from(k) == Ok(i)
        };
        [
            self.ymd, self.mdy, self.dmy, self.hms, self.am, self.tz, self.mm, self.dd,
        ]
        .into_iter()
        .flatten()
        .chain(self.d_m_y.into_iter().flatten())
        .any(hits)
    }
}

/// The converter upstream stores for a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conversion {
    /// `int_convert(base)`; `None` detects the base.
    Int(Option<i64>),
    /// `percentage`
    Percentage,
    /// `convert_first(float)`
    Float,
    /// `convert_first(Decimal)`
    Decimal,
    /// `partial(date_convert, **kw)`
    Date(DateKw),
    /// `partial(strf_date_convert, type=...)`
    Strf(String),
    /// `convert_first(extra_types[name])`
    Custom(String),
}

/// A compiled format (upstream `Parser`, minus Python-object handling).
#[derive(Debug)]
pub struct Parser {
    format: String,
    expression: String,
    flags: Flags,
    group_to_name: IndexMap<String, String>,
    name_to_group: IndexMap<String, String>,
    name_types: IndexMap<String, String>,
    fixed_fields: Vec<i64>,
    named_fields: Vec<String>,
    group_index: i64,
    conversions: IndexMap<GroupKey, Conversion>,
    search_re: OnceLock<Arc<Regex>>,
    match_re: OnceLock<Arc<Regex>>,
    binding: BindingData,
}

/// Data a language binding derives once per parser (see
/// [`Parser::binding_data`]). Type-erased so the core stays binding-free.
#[derive(Default)]
struct BindingData(OnceLock<Box<dyn Any + Send + Sync>>);

impl fmt::Debug for BindingData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BindingData")
    }
}

/// One token of `PARSE_RE.split(format)`.
#[derive(Debug, PartialEq, Eq)]
enum Part<'a> {
    Text(&'a [char]),
    Field(&'a [char]),
}

/// End (exclusive) of a `\[[^]]+\]` match starting at `i`, if any.
fn bracket_end(c: &[char], i: usize) -> Option<usize> {
    if c.get(i) != Some(&'[') || c.get(i + 1).is_none_or(|&x| x == ']') {
        return None;
    }
    let rest = c.get(i + 1..)?;
    rest.iter().position(|&x| x == ']').map(|k| i + k + 2)
}

/// `[s[1:-1] for s in re.findall(r"\[[^]]+]", subkeys)]`: the keys of a
/// field name's `[a][b]` suffix.
pub fn bracket_keys(subkeys: &str) -> Vec<String> {
    let c: Vec<char> = subkeys.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        match bracket_end(&c, i) {
            Some(end) => {
                out.extend(c.get(i + 1..end - 1).map(|k| k.iter().collect::<String>()));
                i = end;
            }
            None => i += 1,
        }
    }
    out
}

/// Length of a `PARSE_RE` match at `i`, if any.
fn parse_re_at(c: &[char], i: usize) -> Option<usize> {
    let at = |k: usize| c.get(k).copied();
    match (at(i)?, at(i + 1)) {
        ('{', Some('{')) | ('}', Some('}')) => return Some(2),
        ('{', _) => {}
        _ => return None,
    }
    let word = |ch: char| ch == '-' || pystr::is_word(ch);
    let mut j = i + 1;
    while at(j).is_some_and(word) {
        j += 1;
    }
    loop {
        match at(j) {
            Some('.') if at(j + 1).is_some_and(word) => {
                j += 1;
                while at(j).is_some_and(word) {
                    j += 1;
                }
            }
            Some('[') => match bracket_end(c, j) {
                Some(end) => j = end,
                None => break,
            },
            _ => break,
        }
    }
    if at(j) == Some(':') && at(j + 1).is_some_and(|x| x != '}') {
        if let Some(k) = (j + 1..c.len()).find(|&k| c[k] == '}') {
            return Some(k + 1 - i);
        }
    }
    (at(j) == Some('}')).then_some(j + 1 - i)
}

fn split_format(c: &[char]) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    let mut text_start = 0;
    let mut i = 0;
    while i < c.len() {
        if let Some(n) = parse_re_at(c, i) {
            if text_start < i {
                parts.push(Part::Text(&c[text_start..i]));
            }
            parts.push(Part::Field(&c[i..i + n]));
            i += n;
            text_start = i;
        } else {
            i += 1;
        }
    }
    if text_start < c.len() {
        parts.push(Part::Text(&c[text_start..]));
    }
    parts
}

fn overflow() -> Error {
    Error::Overflow("Python int too large to convert to C ssize_t".into())
}

impl Parser {
    /// `Parser(format, extra_types, case_sensitive)`: builds the expression
    /// eagerly; regexes compile on first use.
    ///
    /// # Errors
    /// Upstream's eager errors (`ValueError`, `RepeatedNameError`, `KeyError`,
    /// `re.error` for strftime types, `TypeError` from `%`-formatting) or
    /// whatever `extra` raises.
    pub fn new<X: ExtraTypes>(
        format: &str,
        extra: &mut X,
        case_sensitive: bool,
    ) -> Result<Parser, X::Error> {
        let mut p = Parser {
            format: format.to_owned(),
            expression: String::new(),
            flags: Flags {
                ignorecase: !case_sensitive,
                dotall: true,
            },
            group_to_name: IndexMap::new(),
            name_to_group: IndexMap::new(),
            name_types: IndexMap::new(),
            fixed_fields: Vec::new(),
            named_fields: Vec::new(),
            group_index: 0,
            conversions: IndexMap::new(),
            search_re: OnceLock::new(),
            match_re: OnceLock::new(),
            binding: BindingData::default(),
        };
        let chars: Vec<char> = format.chars().collect();
        let mut e = String::new();
        for part in split_format(&chars) {
            match part {
                Part::Field(['{', '{']) => e.push_str(r"\{"),
                Part::Field(['}', '}']) => e.push_str(r"\}"),
                // Upstream treats *any* split part that starts with `{` and
                // ends with `}` as a field, including text such as `{:}`.
                Part::Field(f) | Part::Text(f)
                    if f.len() >= 2 && f.first() == Some(&'{') && f.last() == Some(&'}') =>
                {
                    let inner: String = f[1..f.len() - 1].iter().collect();
                    let s = p.handle_field(&inner, extra)?;
                    e.push_str(&s);
                }
                Part::Field(t) | Part::Text(t) => {
                    for &ch in t {
                        if "?\\.[]()*+^$!|".contains(ch) {
                            e.push('\\');
                        }
                        e.push(ch);
                    }
                }
            }
        }
        p.expression = e;
        Ok(p)
    }

    /// `Parser(format, None, case_sensitive)` through a process-wide cache.
    /// With no extra types the build runs no user code, so it depends only
    /// on its arguments and a shared result is indistinguishable from a new
    /// one. Only successes are cached.
    ///
    /// # Errors
    /// The same as [`Parser::new`] with an empty `extra_types`.
    pub fn new_cached(format: &str, case_sensitive: bool) -> Result<Arc<Parser>> {
        static CACHE: [BoundedCache<Parser>; 2] = [const { BoundedCache::new(256) }; 2];
        CACHE[usize::from(case_sensitive)].get_or_try_insert(format, || {
            let mut none: IndexMap<String, ExtraType> = IndexMap::new();
            Parser::new(format, &mut none, case_sensitive)
        })
    }

    /// The original format string.
    pub fn format(&self) -> &str {
        &self.format
    }

    /// The generated regex (upstream `_expression`).
    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// Group indexes of the fixed fields.
    pub fn fixed_fields(&self) -> &[i64] {
        &self.fixed_fields
    }

    /// Regex group names of the named fields.
    pub fn named_fields(&self) -> &[String] {
        &self.named_fields
    }

    /// Original field name of a regex group name.
    pub fn group_name(&self, group: &str) -> Option<&str> {
        self.group_to_name.get(group).map(String::as_str)
    }

    /// The conversion for a group, if any.
    pub fn conversion(&self, key: &GroupKey) -> Option<&Conversion> {
        self.conversions.get(key)
    }

    /// The conversion for the named group `name`, if any (the same as
    /// `conversion(&GroupKey::Name(name.into()))`).
    pub fn named_conversion(&self, name: &str) -> Option<&Conversion> {
        self.conversions.get(&NameKey(name))
    }

    /// All conversions in insertion order.
    pub fn conversions(&self) -> impl Iterator<Item = (&GroupKey, &Conversion)> {
        self.conversions.iter()
    }

    /// A binding's per-parser derived data, if [`Parser::set_binding_data`]
    /// stored a `T`. Shared parsers ([`Parser::new_cached`]) share it, so it
    /// must depend only on the parser.
    pub fn binding_data<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.binding.0.get()?.downcast_ref()
    }

    /// Store `value` as the binding data unless some is already stored, and
    /// return the stored data if it is a `T`.
    pub fn set_binding_data<T: Any + Send + Sync>(&self, value: T) -> Option<&T> {
        self.binding
            .0
            .get_or_init(|| Box::new(value))
            .downcast_ref()
    }

    /// Upstream `_search_re`, compiled on first success.
    ///
    /// # Errors
    /// The raw compile error (`re.error`, `OverflowError`, ...).
    pub fn search_regex(&self) -> Result<Arc<Regex>> {
        self.search_regex_nested(|| DEFAULT_MAX_NESTED)
    }

    /// [`Parser::search_regex`], compiling with
    /// [`Regex::compile_nested`]'s recursion limit (computed only when the
    /// pattern isn't cached).
    ///
    /// # Errors
    /// As for [`Parser::search_regex`], plus the recursion error.
    pub fn search_regex_nested(&self, max_nested: impl FnOnce() -> usize) -> Result<Arc<Regex>> {
        if let Some(r) = self.search_re.get() {
            return Ok(r.clone());
        }
        let r = Regex::compile_cached_nested(&self.expression, self.flags, max_nested)?;
        Ok(self.search_re.get_or_init(|| r).clone())
    }

    /// Upstream `_match_re` (`\A...\Z`), compiled on first success.
    ///
    /// # Errors
    /// `re.error` becomes `NotImplementedError`; other errors pass through.
    pub fn match_regex(&self) -> Result<Arc<Regex>> {
        self.match_regex_nested(|| DEFAULT_MAX_NESTED)
    }

    /// [`Parser::match_regex`], compiling with
    /// [`Regex::compile_nested`]'s recursion limit (computed only when the
    /// pattern isn't cached).
    ///
    /// # Errors
    /// As for [`Parser::match_regex`], plus the recursion error.
    pub fn match_regex_nested(&self, max_nested: impl FnOnce() -> usize) -> Result<Arc<Regex>> {
        if let Some(r) = self.match_re.get() {
            return Ok(r.clone());
        }
        let expression = format!(r"\A{}\Z", self.expression);
        let r = match Regex::compile_cached_nested(&expression, self.flags, max_nested) {
            Ok(r) => r,
            Err(Error::Regex(_)) => {
                return Err(Error::NotImplemented(format!(
                    "Group names (e.g. (?P<name>) can cause failure, as they are not escaped properly: '{expression}'"
                )))
            }
            Err(e) => return Err(e),
        };
        Ok(self.match_re.get_or_init(|| r).clone())
    }

    fn bump(&mut self, n: i64) -> Result<()> {
        self.group_index = self.group_index.checked_add(n).ok_or_else(overflow)?;
        Ok(())
    }

    /// Upstream `_to_group_name`.
    fn make_group_name(&mut self, field: &str) -> Result<String> {
        let mut group = field.replace(['.', '[', ']', '-'], "_");
        let mut n = 1;
        while self.group_to_name.contains_key(&group) {
            n += 1;
            let rep = "_".repeat(n);
            group = if field.contains('.') {
                field.replace('.', &rep)
            } else if field.contains('_') {
                field.replace('_', &rep)
            } else if field.contains('-') {
                field.replace('-', &rep)
            } else {
                return Err(Error::Key(format!(
                    "duplicated group name {}",
                    pystr::repr(field)
                )));
            };
        }
        self.group_to_name.insert(group.clone(), field.to_owned());
        self.name_to_group.insert(field.to_owned(), group.clone());
        Ok(group)
    }

    /// Upstream `_handle_field` (braces already removed).
    fn handle_field<X: ExtraTypes>(
        &mut self,
        field: &str,
        extra: &mut X,
    ) -> Result<String, X::Error> {
        let (name, spec) = match field.split_once(':') {
            Some((n, f)) => (n, f),
            None => (field, ""),
        };
        let wrap: String;
        let group: GroupKey;
        if name.chars().next().is_some_and(pystr::is_alpha) {
            if let Some(g) = self.name_to_group.get(name) {
                let seen = self.name_types.get(name).map_or("", String::as_str);
                if seen != spec {
                    return Err(Error::RepeatedName(format!(
                        "field type {} for field \"{}\" does not match previous seen type {}",
                        pystr::repr(spec),
                        name,
                        pystr::repr(seen)
                    ))
                    .into());
                }
                return Ok(format!("(?P={g})"));
            }
            let g = self.make_group_name(name)?;
            self.name_types.insert(name.to_owned(), spec.to_owned());
            self.named_fields.push(g.clone());
            wrap = format!("(?P<{g}>%s)");
            group = GroupKey::Name(g);
        } else {
            self.fixed_fields.push(self.group_index);
            wrap = "(%s)".to_owned();
            group = GroupKey::Index(self.group_index);
        }

        if spec.is_empty() {
            self.bump(1)?;
            return Ok(percent_format(&wrap, ".+?")?);
        }

        let format = extract_format(spec, |t| extra.contains(t))?;
        let ty = format.type_.as_str();
        let is_numeric = !ty.is_empty() && "n%fFegdobxEGX".contains(ty);
        let n = self.group_index;
        let mut conv: Option<Conversion> = None;
        let mut s: String;
        if extra.contains(ty)? {
            let info = extra.get(ty)?;
            s = info.pattern;
            self.bump(info.group_count)?;
            conv = Some(Conversion::Custom(ty.to_owned()));
        } else {
            s = self.builtin_type(&format, n, &mut conv)?;
        }
        if let Some(c) = conv {
            self.conversions.insert(group, c);
        }

        let align = format.align;
        let mut fill: Option<String> = format.fill.map(String::from);
        if is_numeric {
            if align == Some('=') {
                let f = fill.get_or_insert_with(|| "0".to_owned());
                s = format!("{f}*{s}");
            }
            s = format!("[-+ ]?{s}");
        }
        let mut fill = fill.unwrap_or_else(|| " ".to_owned());

        s = percent_format(&wrap, &s)?;
        self.bump(1)?;

        let mut align = align;
        if !format.width.is_empty() && align.is_none() {
            align = Some('>');
        }
        if fill.chars().all(|c| r".\+?*[](){}^$".contains(c)) {
            fill = format!("\\{fill}");
        }
        Ok(match align {
            Some('<') => format!("{s}{fill}*"),
            Some('>') => format!("{fill}*{s}"),
            Some('^') => format!("{fill}*{s}{fill}*"),
            _ => s,
        })
    }

    /// The pattern for a non-extra type; records the conversion.
    fn builtin_type(
        &mut self,
        format: &FormatSpec,
        n: i64,
        conv: &mut Option<Conversion>,
    ) -> Result<String> {
        let ty = format.type_.as_str();
        let width = format.width.as_str();
        let precision = format.precision.as_deref().unwrap_or("");
        let date = |kw: DateKw| Some(Conversion::Date(kw));
        let at = |k: i64| n.checked_add(k).ok_or_else(overflow);
        let s = match ty {
            "n" => {
                self.bump(1)?;
                *conv = Some(Conversion::Int(Some(10)));
                r"\d{1,3}([,.]\d{3})*".to_owned()
            }
            "b" => {
                self.bump(1)?;
                *conv = Some(Conversion::Int(Some(2)));
                r"(0[bB])?[01]+".to_owned()
            }
            "o" => {
                self.bump(1)?;
                *conv = Some(Conversion::Int(Some(8)));
                r"(0[oO])?[0-7]+".to_owned()
            }
            "x" | "X" => {
                self.bump(1)?;
                *conv = Some(Conversion::Int(Some(16)));
                r"(0[xX])?[0-9a-fA-F]+".to_owned()
            }
            "%" => {
                self.bump(1)?;
                *conv = Some(Conversion::Percentage);
                r"\d+(\.\d+)?%".to_owned()
            }
            "f" | "F" => {
                *conv = Some(if ty == "f" {
                    Conversion::Float
                } else {
                    Conversion::Decimal
                });
                if format.precision.as_deref() == Some("0") {
                    r"(?:\d+|nan|NAN|inf|INF)".to_owned()
                } else {
                    r"(?:\d*\.\d+|nan|NAN|inf|INF)".to_owned()
                }
            }
            "e" | "E" => {
                *conv = Some(Conversion::Float);
                r"\d*\.\d+[eE][-+]?\d+|nan|NAN|[-+]?inf|[-+]?INF".to_owned()
            }
            "g" | "G" => {
                self.bump(2)?;
                *conv = Some(Conversion::Float);
                r"\d+(\.\d+)?([eE][-+]?\d+)?|nan|NAN|[-+]?inf|[-+]?INF".to_owned()
            }
            "d" => {
                let w = if width.is_empty() {
                    "+".to_owned()
                } else {
                    format!("{{1,{}}}", pystr::py_int(width, 10)?)
                };
                let g = format.grouping.map(String::from).unwrap_or_default();
                *conv = Some(Conversion::Int(None));
                format!(
                    "[-+ ]?[0-9{g}]{w}|[-+ ]?0[xX][0-9a-fA-F{g}]{w}|[-+ ]?0[bB][01{g}]{w}|[-+ ]?0[oO][0-7{g}]{w}"
                )
            }
            _ if has_datetime_directive(ty) => {
                let s = get_regex_for_datetime_format(ty);
                *conv = Some(Conversion::Strf(ty.to_owned()));
                let groups = Regex::compile_cached(&s, Flags::default())?.groups();
                self.bump(i64::try_from(groups).map_err(|_| overflow())?)?;
                s
            }
            "ti" => {
                *conv = date(DateKw {
                    ymd: Some(at(1)?),
                    hms: Some(at(4)?),
                    tz: Some(at(7)?),
                    ..DateKw::default()
                });
                self.bump(7)?;
                format!(r"(\d{{4}}-\d\d-\d\d)((\s+|T){TIME_PAT})?(Z|\s*[-+]\d\d:?\d\d)?")
            }
            "tg" => {
                *conv = date(DateKw {
                    dmy: Some(at(1)?),
                    hms: Some(at(5)?),
                    am: Some(at(8)?),
                    tz: Some(at(9)?),
                    ..DateKw::default()
                });
                self.bump(9)?;
                format!(
                    r"(\d{{1,2}}[-/](\d{{1,2}}|{ALL_MONTHS_PAT})[-/]\d{{4}})(\s+{TIME_PAT})?{AM_PAT}?{TZ_PAT}?"
                )
            }
            "ta" => {
                *conv = date(DateKw {
                    mdy: Some(at(1)?),
                    hms: Some(at(5)?),
                    am: Some(at(8)?),
                    tz: Some(at(9)?),
                    ..DateKw::default()
                });
                self.bump(9)?;
                format!(
                    r"((\d{{1,2}}|{ALL_MONTHS_PAT})[-/]\d{{1,2}}[-/]\d{{4}})(\s+{TIME_PAT})?{AM_PAT}?{TZ_PAT}?"
                )
            }
            "te" => {
                *conv = date(DateKw {
                    dmy: Some(at(3)?),
                    hms: Some(at(5)?),
                    tz: Some(at(8)?),
                    ..DateKw::default()
                });
                self.bump(8)?;
                format!(
                    r"({DAYS_PAT},\s+)?(\d{{1,2}}\s+{MONTHS_PAT}\s+\d{{4}})\s+{TIME_PAT}{TZ_PAT}"
                )
            }
            "th" => {
                *conv = date(DateKw {
                    dmy: Some(at(1)?),
                    hms: Some(at(3)?),
                    tz: Some(at(6)?),
                    ..DateKw::default()
                });
                self.bump(6)?;
                format!(r"(\d{{1,2}}[-/]{MONTHS_PAT}[-/]\d{{4}}):{TIME_PAT}{TZ_PAT}")
            }
            "tc" => {
                *conv = date(DateKw {
                    d_m_y: Some([at(4)?, at(3)?, at(8)?]),
                    hms: Some(at(5)?),
                    ..DateKw::default()
                });
                self.bump(8)?;
                format!(r"({DAYS_PAT})\s+{MONTHS_PAT}\s+(\d{{1,2}})\s+{TIME_PAT}\s+(\d{{4}})")
            }
            "tt" => {
                *conv = date(DateKw {
                    hms: Some(at(1)?),
                    am: Some(at(4)?),
                    tz: Some(at(5)?),
                    ..DateKw::default()
                });
                self.bump(5)?;
                format!("{TIME_PAT}{AM_PAT}?{TZ_PAT}?")
            }
            "ts" => {
                *conv = date(DateKw {
                    mm: Some(at(1)?),
                    dd: Some(at(3)?),
                    hms: Some(at(5)?),
                    ..DateKw::default()
                });
                self.bump(5)?;
                format!(r"{MONTHS_PAT}(\s+)(\d+)(\s+)(\d{{1,2}}:\d{{1,2}}:\d{{1,2}})?")
            }
            "l" => r"[A-Za-z]+".to_owned(),
            _ if !ty.is_empty() => format!(r"\{ty}+"),
            _ if !precision.is_empty() => {
                if width.is_empty() {
                    format!(".{{1,{precision}}}?")
                } else {
                    format!(".{{{width},{precision}}}?")
                }
            }
            _ if !width.is_empty() => format!(".{{{width},}}?"),
            _ => ".+?".to_owned(),
        };
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expr(f: &str) -> Result<String> {
        let mut x: IndexMap<String, ExtraType> = IndexMap::new();
        Ok(Parser::new(f, &mut x, false)?.expression().to_owned())
    }

    #[test]
    fn expressions() -> Result<()> {
        assert_eq!(expr("{} {}")?, "(.+?) (.+?)");
        assert_eq!(
            expr("{a:d}-{b}")?,
            r"(?P<a>[-+ ]?[-+ ]?[0-9]+|[-+ ]?0[xX][0-9a-fA-F]+|[-+ ]?0[bB][01]+|[-+ ]?0[oO][0-7]+)-(?P<b>.+?)"
        );
        assert_eq!(expr("{{{}}}")?, r"\{(.+?)\}");
        assert_eq!(expr("a.c{a}{a}")?, r"a\.c(?P<a>.+?)(?P=a)");
        assert_eq!(expr("{:|<4}")?, r"(.{4,}?)|*");
        assert_eq!(expr("{:*^9d}")?.chars().take(3).collect::<String>(), r"\**");
        assert!(expr("{a_b_}{a[b]}").is_err());
        assert!(expr("{a[%d]}").is_err());
        Ok(())
    }
}
