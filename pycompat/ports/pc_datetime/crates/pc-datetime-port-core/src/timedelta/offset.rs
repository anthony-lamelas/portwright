//! `datetime.timezone` with a fixed offset and no name, as the shim's `_tz` builds it.

use crate::error::{PyError, PyResult};
use crate::timedelta::delta::{new_delta, TimeDelta, Unit};
use crate::value::Value;

/// A `datetime.timezone(offset)`: the offset lies strictly within ±24 hours.
/// A zero offset is the `timezone.utc` singleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct FixedOffset {
    offset: TimeDelta,
}

impl FixedOffset {
    /// `datetime.timezone(offset)`.
    ///
    /// # Errors
    /// ValueError when the offset isn't strictly between -24h and 24h.
    pub fn new(offset: TimeDelta) -> PyResult<Self> {
        let days = offset.days();
        let at_minus_day = days == -1 && offset.seconds() == 0 && offset.microseconds() < 1;
        if at_minus_day || !(-1..1).contains(&days) {
            return Err(PyError::value_error(format!(
                "offset must be a timedelta strictly between -timedelta(hours=24) and \
                 timedelta(hours=24), not {}.",
                offset.repr()
            )));
        }
        Ok(Self { offset })
    }

    /// `tz.utcoffset(None)`.
    pub fn utcoffset(self) -> TimeDelta {
        self.offset
    }

    /// Whether this is `timezone.utc`.
    pub fn is_utc(self) -> bool {
        self.offset.is_zero()
    }

    /// `tz.tzname(None)`: `UTC`, or `UTC±HH:MM[:SS[.ffffff]]`.
    pub fn tzname(self) -> String {
        if self.is_utc() {
            return "UTC".to_owned();
        }
        let (sign, total) = if self.offset.days() < 0 {
            ('-', -self.offset.total_microseconds())
        } else {
            ('+', self.offset.total_microseconds())
        };
        let microseconds = total % 1_000_000;
        let seconds = total / 1_000_000;
        let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
        if microseconds != 0 {
            format!("UTC{sign}{hours:02}:{minutes:02}:{seconds:02}.{microseconds:06}")
        } else if seconds != 0 {
            format!("UTC{sign}{hours:02}:{minutes:02}:{seconds:02}")
        } else {
            format!("UTC{sign}{hours:02}:{minutes:02}")
        }
    }

    /// `repr(tz)`: `datetime.timezone.utc` or `datetime.timezone(datetime.timedelta(...))`.
    pub fn repr(self) -> String {
        if self.is_utc() {
            "datetime.timezone.utc".to_owned()
        } else {
            format!("datetime.timezone({})", self.offset.repr())
        }
    }
}

/// The shim's `_tz(offset)`: `None`, or `timezone(timedelta(seconds=offset))`.
///
/// # Errors
/// The timedelta constructor's errors, then the timezone's ValueError.
pub fn offset_in(offset: &Value) -> PyResult<Option<FixedOffset>> {
    if *offset == Value::None {
        return Ok(None);
    }
    let delta = new_delta(&[(Unit::Seconds, offset)])?;
    FixedOffset::new(delta).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(v: Value) -> String {
        offset_in(&v)
            .ok()
            .flatten()
            .map_or_else(String::new, FixedOffset::tzname)
    }

    #[test]
    fn tznames() {
        assert_eq!(name(Value::int(0)), "UTC");
        assert_eq!(name(Value::int(19800)), "UTC+05:30");
        assert_eq!(name(Value::int(1)), "UTC+00:00:01");
        assert_eq!(name(Value::Float(0.5)), "UTC+00:00:00.500000");
        assert_eq!(name(Value::int(-1)), "UTC-00:00:01");
    }
}
