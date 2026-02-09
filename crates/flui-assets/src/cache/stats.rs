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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hit_rate() {
        let mut stats = CacheStats::default();
        stats.hits = 80;
        stats.misses = 20;

        assert!((stats.hit_rate() - 0.8).abs() < 1e-10);
        assert!((stats.miss_rate() - 0.2).abs() < 1e-10);
        assert_eq!(stats.total_requests(), 100);
    }

    #[test]
    fn test_zero_requests() {
        let stats = CacheStats::default();
        assert_eq!(stats.hit_rate(), 0.0);
        assert_eq!(stats.miss_rate(), 1.0);
        assert_eq!(stats.total_requests(), 0);
    }
}
