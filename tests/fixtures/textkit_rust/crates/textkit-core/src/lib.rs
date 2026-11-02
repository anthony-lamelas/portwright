//! Hand-written port of the textkit fixture. It contains no Python.
//!
//! Python strings can hold lone surrogates, which a Rust `String` can't, so text
//! is a slice of code points here.
#![forbid(unsafe_code)]

use indexmap::IndexMap;
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{Signed, ToPrimitive, Zero};

const SPACE: u32 = ' ' as u32;
const EQUALS: u32 = '=' as u32;

fn ascii_alphanumeric(point: u32) -> Option<u8> {
    u8::try_from(point).ok().filter(u8::is_ascii_alphanumeric)
}

/// Join runs of ASCII letters and digits with `sep`, in lowercase.
pub fn slug(text: &[u32], sep: &[u32]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut pending = false;
    for &point in text {
        match ascii_alphanumeric(point) {
            Some(byte) => {
                if pending && !out.is_empty() {
                    out.extend_from_slice(sep);
                }
                out.push(u32::from(byte.to_ascii_lowercase()));
                pending = false;
            }
            None => pending = true,
        }
    }
    out
}

/// Apply Python's `text[:n]`, where a negative `n` counts from the end.
pub fn prefix<'a>(text: &'a [u32], n: &BigInt) -> &'a [u32] {
    let end = if n.is_negative() {
        let from_end = n.abs().to_usize().unwrap_or(usize::MAX);
        text.len().saturating_sub(from_end)
    } else {
        n.to_usize().unwrap_or(usize::MAX).min(text.len())
    };
    &text[..end]
}

/// Apply Python's `text.rstrip(chars)`, which treats `chars` as a set.
pub fn rstrip<'a>(text: &'a [u32], chars: &[u32]) -> &'a [u32] {
    let keep = text.iter().rposition(|point| !chars.contains(point));
    &text[..keep.map_or(0, |index| index + 1)]
}

/// Floor division and modulo, or `None` when `size` is zero.
pub fn floor_bucket(n: &BigInt, size: &BigInt) -> Option<(BigInt, BigInt)> {
    if size.is_zero() {
        return None;
    }
    Some(n.div_mod_floor(size))
}

/// Add up the decimal digits of `n`, ignoring its sign.
pub fn sum_digits(n: &BigInt) -> BigInt {
    let ten = BigInt::from(10);
    let mut remaining = n.abs();
    let mut total = BigInt::zero();
    while !remaining.is_zero() {
        let (quotient, digit) = remaining.div_rem(&ten);
        total += digit;
        remaining = quotient;
    }
    total
}

/// Count space-separated words, in first-seen order.
pub fn word_count(text: &[u32]) -> IndexMap<Vec<u32>, u64> {
    let mut counts = IndexMap::new();
    for word in text
        .split(|&point| point == SPACE)
        .filter(|word| !word.is_empty())
    {
        *counts.entry(word.to_vec()).or_insert(0) += 1;
    }
    counts
}

/// Why a `key=value` line couldn't be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseKvError {
    MissingEquals,
    EmptyKey,
}

fn strip_spaces(text: &[u32]) -> &[u32] {
    let start = text
        .iter()
        .position(|&point| point != SPACE)
        .unwrap_or(text.len());
    let trimmed = &text[start..];
    rstrip(trimmed, &[SPACE])
}

/// Split a `key=value` line and trim spaces from both parts.
pub fn parse_kv(line: &[u32]) -> Result<(&[u32], &[u32]), ParseKvError> {
    let split = line
        .iter()
        .position(|&point| point == EQUALS)
        .ok_or(ParseKvError::MissingEquals)?;
    let key = strip_spaces(&line[..split]);
    if key.is_empty() {
        return Err(ParseKvError::EmptyKey);
    }
    Ok((key, strip_spaces(&line[split + 1..])))
}

/// Sort and remove duplicates in place, and return how many were removed.
pub fn normalize(values: &mut Vec<BigInt>) -> usize {
    let before = values.len();
    values.sort();
    values.dedup();
    before - values.len()
}

/// Why a stack operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StackError {
    Full,
    Empty,
}

/// A stack with an optional capacity.
#[derive(Debug)]
pub struct Stack<T> {
    capacity: Option<i64>,
    items: Vec<T>,
}

impl<T> Stack<T> {
    #[must_use]
    pub fn new(capacity: Option<i64>) -> Self {
        Self {
            capacity,
            items: Vec::new(),
        }
    }

    #[must_use]
    pub fn capacity(&self) -> Option<i64> {
        self.capacity
    }

    pub fn push(&mut self, item: T) -> Result<(), StackError> {
        if self
            .capacity
            .is_some_and(|limit| i64::try_from(self.items.len()).is_ok_and(|len| len >= limit))
        {
            return Err(StackError::Full);
        }
        self.items.push(item);
        Ok(())
    }

    pub fn pop(&mut self) -> Result<T, StackError> {
        self.items.pop().ok_or(StackError::Empty)
    }

    #[must_use]
    pub fn peek(&self) -> Option<&T> {
        self.items.last()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
