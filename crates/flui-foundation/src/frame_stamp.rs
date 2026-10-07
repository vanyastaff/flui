//! [`FrameStamp`] bundles the typed presentation address, per-realm frame
//! epoch, raster surface generation and GPU resource generation that cross
//! the raster boundary together.
//!
//! Every axis is required. In particular, [`GpuResourceGeneration`] and
//! [`SurfaceGeneration`] protect different failure boundaries and must both
//! be checked before rendering (ADR-0045 decision 4).
//!
//! [`FrameStamp::new`] takes these four values directly. A builder would not
//! make adding another required value compatible with existing callers:
//! each caller must still supply it. The plain constructor exposes that
//! requirement without another construction protocol.

use crate::epoch::{FrameEpoch, GpuResourceGeneration, SurfaceGeneration};
use crate::id::PresentationAddress;

/// The identity group that stamps one frame: which presentation produced it,
/// at what per-realm epoch, against which raster surface configuration.
///
/// # Frame identity
///
/// A frame's full identity is `(address, epoch)`, never `epoch` alone:
/// [`FrameEpoch`] is only per-*realm* monotonic, so two presentations
/// belonging to the same realm's forest may composite in the same epoch —
/// `address` disambiguates them. `address` is the full `(realm_id,
/// presentation_id)` pair — never `presentation_id` alone, since two
/// different realm incarnations can mint an identical `PresentationId` and
/// only the full pair safely distinguishes them. `surface_generation` is a
/// separate axis, scoped *per presentation*: it is minted by that
/// presentation's own raster seam (ADR-0037 §8), never by the realm or by
/// frame counting. `gpu_resource_generation` is a third, independent axis
/// (ADR-0045 decision 4): `surface_generation` guards against a torn-down
/// swapchain, `gpu_resource_generation` guards against a torn-down
/// `wgpu::Device` — a shared device loss invalidates every surface on its
/// owner thread at once, which is a materially different failure than one
/// surface being resized, so the two are checked as separate clauses rather
/// than folded into one counter.
///
/// # `#[non_exhaustive]`, and what it does and does not do here
///
/// As specified by the [Rust Reference], `#[non_exhaustive]` prevents
/// external struct-literal construction and requires `..` in external
/// struct patterns. Callers construct a stamp through [`FrameStamp::new`].
/// The attribute does not make a required addition to that constructor's
/// arguments compatible with existing calls.
///
/// [Rust Reference]: https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameStamp {
    /// The full realm+presentation address that composited this frame.
    pub address: PresentationAddress,
    /// The runtime's per-frame counter at the time this frame was
    /// composited.
    pub epoch: FrameEpoch,
    /// The raster surface generation this frame was produced against.
    pub surface_generation: SurfaceGeneration,
    /// The GPU-resource generation this frame was produced
    /// against (ADR-0045 decision 4). See the type doc above for why this
    /// is a separate axis from `surface_generation` rather than folded into
    /// it.
    pub gpu_resource_generation: GpuResourceGeneration,
}

impl FrameStamp {
    /// Packages the four identity/versioning fields the raster boundary
    /// needs to accept, reject, or reconcile a frame.
    ///
    /// All four fields are distinct newtypes (never the same underlying
    /// type as one another), so transposing two arguments is a compile-time
    /// type error, not a silent bug — see the second example below.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::{
    ///     FrameEpoch, FrameStamp, GpuResourceGeneration, PresentationAddress, PresentationId,
    ///     RealmId, SurfaceGeneration,
    /// };
    ///
    /// let address = PresentationAddress {
    ///     realm_id: RealmId::new(1),
    ///     presentation_id: PresentationId::new(1),
    /// };
    /// let stamp = FrameStamp::new(address, FrameEpoch::ZERO, SurfaceGeneration::ZERO, GpuResourceGeneration::ZERO);
    ///
    /// assert_eq!(stamp.epoch, FrameEpoch::ZERO);
    /// ```
    ///
    /// Swapping `epoch` and `surface_generation` — the two fields whose
    /// underlying representation looks alike (both wrap a `u64` counter) —
    /// does not compile:
    ///
    /// ```compile_fail
    /// use flui_foundation::{
    ///     FrameEpoch, FrameStamp, GpuResourceGeneration, PresentationAddress, PresentationId,
    ///     RealmId, SurfaceGeneration,
    /// };
    ///
    /// let address = PresentationAddress {
    ///     realm_id: RealmId::new(1),
    ///     presentation_id: PresentationId::new(1),
    /// };
    /// let stamp = FrameStamp::new(address, SurfaceGeneration::ZERO, FrameEpoch::ZERO, GpuResourceGeneration::ZERO);
    ///
    /// assert_eq!(stamp.epoch, FrameEpoch::ZERO);
    /// ```
    #[must_use]
    pub fn new(
        address: PresentationAddress,
        epoch: FrameEpoch,
        surface_generation: SurfaceGeneration,
        gpu_resource_generation: GpuResourceGeneration,
    ) -> Self {
        Self {
            address,
            epoch,
            surface_generation,
            gpu_resource_generation,
        }
    }
}
