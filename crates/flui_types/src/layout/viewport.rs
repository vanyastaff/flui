//! Viewport-related types
//!
//! Types for configuring viewport caching behavior.

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CacheExtentStyle {
    #[default]
    Pixel,

    /// Cache extent is a fraction of the viewport extent.
    ///
    /// For example, a cache extent of 0.5 means half the viewport's
    /// size will be pre-rendered beyond the visible area.
    Viewport,
}
