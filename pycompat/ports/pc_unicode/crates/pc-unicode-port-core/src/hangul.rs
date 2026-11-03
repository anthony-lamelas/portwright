//! Hangul syllable constants from Unicode 15.0 §3.12 (conjoining jamo behavior).

pub(crate) const S_BASE: u32 = 0xAC00;
pub(crate) const L_BASE: u32 = 0x1100;
pub(crate) const V_BASE: u32 = 0x1161;
pub(crate) const T_BASE: u32 = 0x11A7;
pub(crate) const L_COUNT: u32 = 19;
pub(crate) const V_COUNT: u32 = 21;
pub(crate) const T_COUNT: u32 = 28;
/// Syllables per leading consonant.
pub(crate) const N_COUNT: u32 = V_COUNT * T_COUNT;
pub(crate) const S_COUNT: u32 = L_COUNT * N_COUNT;
