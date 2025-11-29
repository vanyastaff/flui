//! Viewport-related types
//!
//! Types for configuring viewport caching behavior.

/// How to interpret the cache extent value for viewport.
///
/// Determines how the `cacheExtent` property is interpreted for
/// pre-rendering content outside the visible viewport area.
///
/// # Examples
///
/// ```
/// use flui_types::layout::CacheExtentStyle;
///
/// // Default is pixel-based caching
/// let style = CacheExtentStyle::default();
/// assert_eq!(style, CacheExtentStyle::Pixel);
///
/// // Viewport-based for responsive caching
/// let viewport_style = CacheExtentStyle::Viewport;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CacheExtentStyle {
    /// Cache extent is specified in logical pixels.
    ///
    /// For example, a cache extent of 100.0 means 100 logical pixels
    /// of content will be pre-rendered beyond the visible viewport.
    #[default]
    Pixel,

    /// Cache extent is a fraction of the viewport extent.
    ///
    /// For example, a cache extent of 0.5 means half the viewport's
    /// size will be pre-rendered beyond the visible area.
    Viewport,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_extent_style_default() {
        assert_eq!(CacheExtentStyle::default(), CacheExtentStyle::Pixel);
    }

    #[test]
    fn test_cache_extent_style_variants() {
        assert_ne!(CacheExtentStyle::Pixel, CacheExtentStyle::Viewport);
    }
}
