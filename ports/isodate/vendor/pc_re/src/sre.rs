//! sre's opcode, AT and CATEGORY numbering (CPython 3.12 `re._constants`).

/// sre opcodes, numbered as `_constants.OPCODES`.
pub mod op {
    pub const FAILURE: u32 = 0;
    pub const SUCCESS: u32 = 1;
    pub const ANY: u32 = 2;
    pub const ANY_ALL: u32 = 3;
    pub const ASSERT: u32 = 4;
    pub const ASSERT_NOT: u32 = 5;
    pub const AT: u32 = 6;
    pub const BRANCH: u32 = 7;
    pub const CATEGORY: u32 = 8;
    pub const CHARSET: u32 = 9;
    pub const BIGCHARSET: u32 = 10;
    pub const GROUPREF: u32 = 11;
    pub const GROUPREF_EXISTS: u32 = 12;
    pub const IN: u32 = 13;
    pub const INFO: u32 = 14;
    pub const JUMP: u32 = 15;
    pub const LITERAL: u32 = 16;
    pub const MARK: u32 = 17;
    pub const MAX_UNTIL: u32 = 18;
    pub const MIN_UNTIL: u32 = 19;
    pub const NOT_LITERAL: u32 = 20;
    pub const NEGATE: u32 = 21;
    pub const RANGE: u32 = 22;
    pub const REPEAT: u32 = 23;
    pub const REPEAT_ONE: u32 = 24;
    pub const SUBPATTERN: u32 = 25;
    pub const MIN_REPEAT_ONE: u32 = 26;
    pub const ATOMIC_GROUP: u32 = 27;
    pub const POSSESSIVE_REPEAT: u32 = 28;
    pub const POSSESSIVE_REPEAT_ONE: u32 = 29;
    pub const GROUPREF_IGNORE: u32 = 30;
    pub const IN_IGNORE: u32 = 31;
    pub const LITERAL_IGNORE: u32 = 32;
    pub const NOT_LITERAL_IGNORE: u32 = 33;
    pub const GROUPREF_LOC_IGNORE: u32 = 34;
    pub const IN_LOC_IGNORE: u32 = 35;
    pub const LITERAL_LOC_IGNORE: u32 = 36;
    pub const NOT_LITERAL_LOC_IGNORE: u32 = 37;
    pub const GROUPREF_UNI_IGNORE: u32 = 38;
    pub const IN_UNI_IGNORE: u32 = 39;
    pub const LITERAL_UNI_IGNORE: u32 = 40;
    pub const NOT_LITERAL_UNI_IGNORE: u32 = 41;
    pub const RANGE_UNI_IGNORE: u32 = 42;
    /// Parser-only (never in compiled code).
    pub const MIN_REPEAT: u32 = 43;
    /// Parser-only (never in compiled code).
    pub const MAX_REPEAT: u32 = 44;
}

/// Names of the opcodes, indexed by number.
pub const OPCODE_NAMES: [&str; 45] = [
    "FAILURE",
    "SUCCESS",
    "ANY",
    "ANY_ALL",
    "ASSERT",
    "ASSERT_NOT",
    "AT",
    "BRANCH",
    "CATEGORY",
    "CHARSET",
    "BIGCHARSET",
    "GROUPREF",
    "GROUPREF_EXISTS",
    "IN",
    "INFO",
    "JUMP",
    "LITERAL",
    "MARK",
    "MAX_UNTIL",
    "MIN_UNTIL",
    "NOT_LITERAL",
    "NEGATE",
    "RANGE",
    "REPEAT",
    "REPEAT_ONE",
    "SUBPATTERN",
    "MIN_REPEAT_ONE",
    "ATOMIC_GROUP",
    "POSSESSIVE_REPEAT",
    "POSSESSIVE_REPEAT_ONE",
    "GROUPREF_IGNORE",
    "IN_IGNORE",
    "LITERAL_IGNORE",
    "NOT_LITERAL_IGNORE",
    "GROUPREF_LOC_IGNORE",
    "IN_LOC_IGNORE",
    "LITERAL_LOC_IGNORE",
    "NOT_LITERAL_LOC_IGNORE",
    "GROUPREF_UNI_IGNORE",
    "IN_UNI_IGNORE",
    "LITERAL_UNI_IGNORE",
    "NOT_LITERAL_UNI_IGNORE",
    "RANGE_UNI_IGNORE",
    "MIN_REPEAT",
    "MAX_REPEAT",
];

/// AT codes (`_constants.ATCODES`).
pub mod at {
    pub const BEGINNING: u32 = 0;
    pub const BEGINNING_LINE: u32 = 1;
    pub const BEGINNING_STRING: u32 = 2;
    pub const BOUNDARY: u32 = 3;
    pub const NON_BOUNDARY: u32 = 4;
    pub const END: u32 = 5;
    pub const END_LINE: u32 = 6;
    pub const END_STRING: u32 = 7;
    pub const LOC_BOUNDARY: u32 = 8;
    pub const LOC_NON_BOUNDARY: u32 = 9;
    pub const UNI_BOUNDARY: u32 = 10;
    pub const UNI_NON_BOUNDARY: u32 = 11;
}

/// Names of the AT codes, indexed by number.
pub const AT_NAMES: [&str; 12] = [
    "AT_BEGINNING",
    "AT_BEGINNING_LINE",
    "AT_BEGINNING_STRING",
    "AT_BOUNDARY",
    "AT_NON_BOUNDARY",
    "AT_END",
    "AT_END_LINE",
    "AT_END_STRING",
    "AT_LOC_BOUNDARY",
    "AT_LOC_NON_BOUNDARY",
    "AT_UNI_BOUNDARY",
    "AT_UNI_NON_BOUNDARY",
];

/// Category codes (`_constants.CHCODES`).
pub mod ch {
    pub const DIGIT: u32 = 0;
    pub const NOT_DIGIT: u32 = 1;
    pub const SPACE: u32 = 2;
    pub const NOT_SPACE: u32 = 3;
    pub const WORD: u32 = 4;
    pub const NOT_WORD: u32 = 5;
    pub const LINEBREAK: u32 = 6;
    pub const NOT_LINEBREAK: u32 = 7;
    pub const LOC_WORD: u32 = 8;
    pub const LOC_NOT_WORD: u32 = 9;
    pub const UNI_DIGIT: u32 = 10;
    pub const UNI_NOT_DIGIT: u32 = 11;
    pub const UNI_SPACE: u32 = 12;
    pub const UNI_NOT_SPACE: u32 = 13;
    pub const UNI_WORD: u32 = 14;
    pub const UNI_NOT_WORD: u32 = 15;
    pub const UNI_LINEBREAK: u32 = 16;
    pub const UNI_NOT_LINEBREAK: u32 = 17;
}

/// Names of the category codes, indexed by number.
pub const CH_NAMES: [&str; 18] = [
    "CATEGORY_DIGIT",
    "CATEGORY_NOT_DIGIT",
    "CATEGORY_SPACE",
    "CATEGORY_NOT_SPACE",
    "CATEGORY_WORD",
    "CATEGORY_NOT_WORD",
    "CATEGORY_LINEBREAK",
    "CATEGORY_NOT_LINEBREAK",
    "CATEGORY_LOC_WORD",
    "CATEGORY_LOC_NOT_WORD",
    "CATEGORY_UNI_DIGIT",
    "CATEGORY_UNI_NOT_DIGIT",
    "CATEGORY_UNI_SPACE",
    "CATEGORY_UNI_NOT_SPACE",
    "CATEGORY_UNI_WORD",
    "CATEGORY_UNI_NOT_WORD",
    "CATEGORY_UNI_LINEBREAK",
    "CATEGORY_UNI_NOT_LINEBREAK",
];

/// `CH_LOCALE[code]`.
pub fn ch_locale(code: u32) -> u32 {
    match code {
        ch::WORD => ch::LOC_WORD,
        ch::NOT_WORD => ch::LOC_NOT_WORD,
        other => other,
    }
}

/// `CH_UNICODE[code]`.
pub fn ch_unicode(code: u32) -> u32 {
    match code {
        0..=7 => code + 10,
        other => other,
    }
}

/// `_sre.MAXREPEAT`.
pub const MAXREPEAT: u32 = u32::MAX;
/// `_sre.MAXGROUPS`.
pub const MAXGROUPS: usize = 1_073_741_823;
/// `_compiler.MAXCODE`.
pub const MAXCODE: u32 = u32::MAX;

/// `SRE_INFO_PREFIX`
pub const INFO_PREFIX: u32 = 1;
/// `SRE_INFO_LITERAL`
pub const INFO_LITERAL: u32 = 2;
/// `SRE_INFO_CHARSET`
pub const INFO_CHARSET: u32 = 4;

/// Name of a code-table entry, or `?` if out of range.
pub fn name(table: &[&'static str], code: u32) -> &'static str {
    usize::try_from(code)
        .ok()
        .and_then(|i| table.get(i))
        .copied()
        .unwrap_or("?")
}
