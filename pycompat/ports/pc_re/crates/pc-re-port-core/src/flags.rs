//! `re` flag values (`re.RegexFlag`), exported by the module as plain ints.

/// `re.NOFLAG`
pub const NOFLAG: u32 = 0;
/// `re.TEMPLATE` (undocumented, still accepted by 3.12)
pub const TEMPLATE: u32 = 1;
/// `re.IGNORECASE`
pub const IGNORECASE: u32 = 2;
/// `re.LOCALE`
pub const LOCALE: u32 = 4;
/// `re.MULTILINE`
pub const MULTILINE: u32 = 8;
/// `re.DOTALL`
pub const DOTALL: u32 = 16;
/// `re.UNICODE`
pub const UNICODE: u32 = 32;
/// `re.VERBOSE`
pub const VERBOSE: u32 = 64;
/// `re.DEBUG`
pub const DEBUG: u32 = 128;
/// `re.ASCII`
pub const ASCII: u32 = 256;
