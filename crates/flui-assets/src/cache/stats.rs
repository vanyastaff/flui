//! Cache statistics tracking.

/// Statistics for cache performance.
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStats {
    /// Number of cache hits.
    pub hits: usize,

    /// Number of cache misses.
    pub misses: usize,

    /// Number of insertions.
    pub insertions: usize,

    /// Number of evictions.
    pub evictions: usize,
}

impl CacheStats {
    /// Returns the cache hit rate (0.0 to 1.0).
    ///
    /// Returns 0.0 if no requests have been made.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Returns the cache miss rate (0.0 to 1.0).
    pub fn miss_rate(&self) -> f64 {
        1.0 - self.hit_rate()
    }

    /// Returns the total number of requests (hits + misses).
    pub fn total_requests(&self) -> usize {
        self.hits + self.misses
    }
}
