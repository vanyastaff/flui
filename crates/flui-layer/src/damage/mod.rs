//! The damage producer: which pixels of a frame changed since the previous
//! one, from comparing the two frames' repaint-boundary stamps (ADR-0087 §3).
//!
//! Damage cannot come from which render objects repainted, because the
//! objects that always repaint cover the screen (ADR-0061); it has to come
//! from comparing consecutive layer trees. [`LayerDiffer`] pairs the layers a
//! repaint boundary stamped ([`BoundaryStamp`](crate::BoundaryStamp)) by their
//! `RenderId` and damages a boundary's *own region* — its subtree minus the
//! subtrees of boundaries nested inside it — when its content token or its
//! placement changed.
//!
//! The walk runs on the host side of the raster boundary, over the frozen
//! [`Scene`](crate::Scene) the raster lane is about to submit, and costs one
//! pass over the tree. [`DamageMode::Off`] removes that cost entirely.

mod differ;
#[cfg(test)]
mod tests;

pub use differ::LayerDiffer;

/// Whether a [`LayerDiffer`] produces damage, and when a region is large
/// enough that a full repaint is cheaper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DamageMode {
    /// Retain nothing and report every frame as
    /// [`DamageRegion::Full`](crate::DamageRegion::Full), without walking
    /// the tree: the cost per frame is the cost of not having a differ.
    Off,
    /// Diff every frame against the previous one.
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
