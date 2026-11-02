//! A process-wide, string-keyed memo for pure, immutable build results.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// Shared successes of a pure build, keyed by its string input.
///
/// When full, the whole map is cleared (as `re._cache` once did). Only
/// successes are stored, and values are immutable, so a hit is
/// indistinguishable from building again. A poisoned lock is recovered:
/// the map is only ever read or inserted into, so it can't be half-updated.
pub(crate) struct BoundedCache<V> {
    map: OnceLock<Mutex<HashMap<String, Arc<V>>>>,
    capacity: usize,
}

impl<V> BoundedCache<V> {
    /// An empty cache holding at most `capacity` entries.
    pub(crate) const fn new(capacity: usize) -> Self {
        Self {
            map: OnceLock::new(),
            capacity,
        }
    }

    /// The cached value for `key`, or the result of `build` (cached on success).
    /// `build` runs without the lock held.
    pub(crate) fn get_or_try_insert<E>(
        &self,
        key: &str,
        build: impl FnOnce() -> Result<V, E>,
    ) -> Result<Arc<V>, E> {
        let map = self.map.get_or_init(Mutex::default);
        if let Some(v) = map.lock().unwrap_or_else(PoisonError::into_inner).get(key) {
            return Ok(Arc::clone(v));
        }
        let v = Arc::new(build()?);
        let mut guard = map.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.len() >= self.capacity {
            guard.clear();
        }
        guard.insert(key.to_owned(), Arc::clone(&v));
        Ok(v)
    }
}
