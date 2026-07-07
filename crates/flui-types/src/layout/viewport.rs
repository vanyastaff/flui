//! Viewport-related types
//!
//! Types for configuring viewport caching behavior.

/// How a viewport's cache extent value should be interpreted.
///
/// Mirrors Flutter's `CacheExtentStyle`. The cache extent is the area
/// beyond the visible bounds where children are still laid out (and
/// pre-rendered) so scrolling reveals them without jank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CacheExtentStyle {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Cache extent is an absolute value in logical pixels (the default).
    #[default]
    Pixel,

    /// Cache extent is a fraction of the viewport extent.
    ///
    /// For example, a cache extent of 0.5 means half the viewport's
    /// size will be pre-rendered beyond the visible area.
    Viewport,
}
