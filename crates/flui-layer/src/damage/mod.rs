//! Damage: which pixels of a frame changed since the image the raster side
//! holds.
//!
//! The scene owner produces it as the footprint of each patch (ADR-0304 §4),
//! closed over what filters read ([`close_over`]); a [`RasterBase`] decides
//! whether a publication's damage applies to the image the raster side
//! holds, under the [`DamageMode`] it repaints by. Damage cannot come from
//! which render objects repainted, because the objects that always repaint
//! cover the screen (ADR-0061); it comes from which scene nodes a patch
//! changed.
//!
//! The comparison of two consecutive layer trees that came before the
//! retained scene, `LayerDiffer` over `BoundaryStamp`s, lives on behind the
//! `testing` feature for the readback suites that still build trees by hand,
//! until the engine draws versions (`docs/gaps.md`).

mod base;
pub(crate) mod closure;
#[cfg(any(test, feature = "testing"))]
mod differ;
#[cfg(test)]
mod tests;

pub use base::RasterBase;
pub use closure::{Closure, MAX_CLOSURE_ROUNDS, close_over, read_closure};
#[cfg(any(test, feature = "testing"))]
pub use differ::LayerDiffer;

/// Whether the raster side draws damage, and when a region is large
/// enough that a full repaint is cheaper.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum DamageMode {
    /// Retain nothing and report every frame as
    /// [`DamageRegion::Full`](crate::DamageRegion::Full) for
    /// [`FullReason::DamageOff`](crate::FullReason::DamageOff), without
    /// reading the region: every frame pays for a full render.
    Off,
    /// Draw each frame's damage.
    On {
        /// The fraction of the surface above which a damaged rectangle is
        /// reported as `Full` instead: a partial repaint of most of the
        /// surface saves little and pays for a retained target and a blit.
        full_above: f64,
    },
}

impl Default for DamageMode {
    /// On, with `full_above` at one half. The threshold is a starting
    /// hypothesis the `damage_retained_target` bench group measures
    /// (ADR-0087 §4), not a tuned value.
    fn default() -> Self {
        Self::On { full_above: 0.5 }
    }
}
