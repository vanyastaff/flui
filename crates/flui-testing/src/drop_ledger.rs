//! Proof that the values a test handed the framework were dropped
//! (ADR-0261 §2).
//!
//! A test plants a [`DropSentinel`] in each value it hands the framework: a
//! field of a view's state, a capture of a listener or of a future. The
//! sentinel's own `Drop` reports to the [`DropLedger`] it came from, so
//! [`DropLedger::live`] at zero proves every destructor ran. A framework value
//! that leaks keeps its sentinel alive. One ledger per kind of value, so a
//! failure names the kind by the ledger that kept a live sentinel.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Counts the sentinels planted from it and the ones that were dropped.
#[derive(Clone, Debug, Default)]
pub struct DropLedger {
    counts: Arc<Counts>,
}

#[derive(Debug, Default)]
struct Counts {
    planted: AtomicUsize,
    dropped: AtomicUsize,
}

impl DropLedger {
    /// A ledger with nothing planted.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A sentinel to put where the value under test keeps it.
    pub fn plant(&self) -> DropSentinel {
        self.counts.planted.fetch_add(1, Ordering::SeqCst);
        DropSentinel {
            counts: Arc::clone(&self.counts),
        }
    }

    /// How many sentinels were planted.
    #[must_use]
    pub fn planted(&self) -> usize {
        self.counts.planted.load(Ordering::SeqCst)
    }

    /// How many planted sentinels are not dropped yet.
    #[must_use]
    pub fn live(&self) -> usize {
        // Read dropped first: a drop between the two reads can only make the
        // difference larger, never wrap it.
        let dropped = self.counts.dropped.load(Ordering::SeqCst);
        self.planted().saturating_sub(dropped)
    }
}

/// Reports its drop, once, to the [`DropLedger`] that planted it.
#[must_use = "a sentinel dropped at once proves nothing"]
#[derive(Debug)]
pub struct DropSentinel {
    counts: Arc<Counts>,
}

impl Drop for DropSentinel {
    fn drop(&mut self) {
        self.counts.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
