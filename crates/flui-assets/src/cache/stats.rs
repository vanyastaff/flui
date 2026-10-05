//! Cache statistics tracking.

/// Best-effort operation counters shared by cache clones.
///
/// Initializer hit/miss counts use an initial presence probe, which can race
/// concurrent writes. Resetting counters can overlap operations. These are
/// diagnostics, not a snapshot of resident entries or eviction events.
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStats {
    /// Number of cache hits.
    pub hits: usize,

    /// Number of cache misses.
    pub misses: usize,

    /// Completed explicit insert calls and fresh initializer results returned
    /// to callers, including calls with retention disabled. Cancellation after
    /// backend publication may leave an entry without a completed result count.
    pub insertions: usize,

    /// Explicit invalidation requests, including requests for absent keys.
    /// Automatic capacity and expiration removals are not counted here.
    pub invalidations: usize,
}

impl CacheStats {
    /// Returns the cache hit rate (0.0 to 1.0).
    ///
    /// Returns 0.0 if no requests have been made.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits as f64 + self.misses as f64;
        if total == 0.0 {
            0.0
        } else {
            self.hits as f64 / total
        }
    }

    /// Returns the cache miss rate (0.0 to 1.0).
    pub fn miss_rate(&self) -> f64 {
        let total = self.hits as f64 + self.misses as f64;
        if total == 0.0 {
            0.0
        } else {
            self.misses as f64 / total
        }
    }

    /// Returns the total number of requests (hits + misses).
    pub fn total_requests(&self) -> usize {
        self.hits.saturating_add(self.misses)
    }
}
