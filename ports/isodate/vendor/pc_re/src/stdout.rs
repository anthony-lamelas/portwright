//! Kept only because the frozen fuzz harness calls [`take`] after each call.
//! The core doesn't reproduce `re.DEBUG` output, so nothing is ever pending.

use crate::text::PyStr;

/// What the last call printed: always empty.
#[must_use]
pub fn take() -> PyStr {
    PyStr::new()
}
