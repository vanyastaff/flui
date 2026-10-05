//! Explicit count capacity and expiration for a typed asset cache.

use std::num::NonZeroU64;
use std::time::Duration;

/// Retention policy for completed assets.
///
/// This bounds entries, not their decoded byte footprint or data retained by
/// consumer handles. Moka applies its capacity policy during maintenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheCapacity {
    /// Return loaded data without retaining completed entries.
    Disabled,
    /// Retain at most this many entries after pending maintenance settles.
    Entries(NonZeroU64),
}

impl CacheCapacity {
    pub(crate) const fn limit(self) -> u64 {
        match self {
            Self::Disabled => 0,
            Self::Entries(entries) => entries.get(),
        }
    }
}

impl Default for CacheCapacity {
    fn default() -> Self {
        Self::Entries(
            NonZeroU64::new(10_240).expect("BUG: default asset cache capacity is nonzero"),
        )
    }
}

/// A supported expiration interval, or explicit absence of expiration.
///
/// Construction excludes durations Moka would reject by panicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheExpiration(Option<Duration>);

impl CacheExpiration {
    /// Do not expire entries according to this lifetime or idle policy.
    pub const NEVER: Self = Self(None);

    /// Expire after a supported interval. Zero expires immediately.
    ///
    /// # Errors
    ///
    /// Returns [`ExpirationTooLong`] above Moka's 1,000-year maximum, using
    /// 365 days per year.
    pub fn after(duration: Duration) -> Result<Self, ExpirationTooLong> {
        if duration > Duration::from_hours(1_000 * 365 * 24) {
            Err(ExpirationTooLong)
        } else {
            Ok(Self(Some(duration)))
        }
    }

    pub(crate) const fn duration(self) -> Option<Duration> {
        self.0
    }
}

/// A requested expiration exceeds Moka's supported interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cache expiration must not exceed 1,000 years")]
pub struct ExpirationTooLong;

/// Capacity and expiration of one typed cache.
///
/// Expiration does not cancel in-flight initializers. Handles already returned
/// to consumers retain their data after expiration or eviction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetCacheConfig {
    /// Count bound, or explicit disabling of completed-entry retention.
    pub capacity: CacheCapacity,
    /// Maximum lifetime since insertion or replacement.
    pub time_to_live: CacheExpiration,
    /// Maximum idle interval since insertion or a retrieving cache read.
    pub time_to_idle: CacheExpiration,
}

impl Default for AssetCacheConfig {
    fn default() -> Self {
        Self {
            capacity: CacheCapacity::default(),
            time_to_live: CacheExpiration::after(Duration::from_mins(5))
                .expect("BUG: default asset lifetime is supported"),
            time_to_idle: CacheExpiration::after(Duration::from_mins(1))
                .expect("BUG: default asset idle interval is supported"),
        }
    }
}
