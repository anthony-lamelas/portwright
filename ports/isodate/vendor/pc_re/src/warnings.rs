//! Warnings CPython's `re` raises while it parses a pattern, such as
//! `FutureWarning: Possible nested set at position 1` for `[[:alpha:]]`.
//!
//! The parser records them here, in order, and the Python layer raises each one
//! as a `FutureWarning` once the call returns. A warning raised before a parse
//! error is still raised, as in CPython. A pattern served from the cache is
//! never parsed again, so it doesn't warn again, also as in CPython.

use std::cell::RefCell;

thread_local! {
    static PENDING: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Records a `FutureWarning` with this message.
pub fn future(message: String) {
    PENDING.with(|pending| pending.borrow_mut().push(message));
}

/// The warnings recorded on this thread since the last call, oldest first.
#[must_use]
pub fn take() -> Vec<String> {
    PENDING.with(|pending| std::mem::take(&mut *pending.borrow_mut()))
}
