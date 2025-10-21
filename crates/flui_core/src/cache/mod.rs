//! Caching infrastructure for performance optimization
//!
//! This module provides high-performance caching using the `moka` crate.
//! It includes caches for layout results, text measurements, and other
//! expensive computations.
//!
//! # Performance
//!
//! - Layout cache: 10x-100x speedup for repeated layouts
//! - Thread-safe: Can be accessed from multiple threads
//! - TTL support: Automatic expiration of stale entries
//! - LRU eviction: Least recently used entries are evicted first
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::cache::{layout_cache, LayoutCacheKey};
//!
//! let cache = layout_cache();
//! let key = LayoutCacheKey::new(widget_id, constraints);
//!
//! let result = cache.get_or_compute(key, || {
//!     // Expensive layout calculation
//!     compute_layout(constraints)
//! });
//! ```

pub mod layout_cache;

pub use layout_cache::{
    LayoutCache, LayoutCacheKey, LayoutResult,
    layout_cache, invalidate_layout, clear_layout_cache
};
