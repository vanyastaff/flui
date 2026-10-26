//! What a feature started watching for a loop: an operating-system observer,
//! a set of browser listeners. The loop holds it as long as the watch should
//! last and names no feature to do so.

use std::any::Any;

/// A watch a feature started, which stops when this is dropped.
///
/// The loop decides when a watch ends (when it stops, or when it is dropped)
/// without knowing what is watched: the feature's hook hands the observer back
/// as a `Watch`, and the loop keeps it. An unwind drops it too, so no stop is
/// skipped.
#[must_use = "a watch stops when dropped: keep it for as long as the observation should last"]
pub(crate) struct Watch {
    /// Held for its `Drop`, which ends the watch.
    _watch: Box<dyn Any>,
}

impl Watch {
    /// A watch that `observer` keeps alive and its `Drop` ends.
    pub(crate) fn new(observer: impl Any) -> Self {
        Self {
            _watch: Box::new(observer),
        }
    }
}

impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watch").finish_non_exhaustive()
    }
}
