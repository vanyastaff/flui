//! [`FrameStamp`] — the frame-identity group threaded through the raster
//! boundary: which presentation produced a frame, at what per-realm epoch,
//! against which raster surface configuration.
//!
//! # Why a bundled type
//!
//! `flui_layer::SceneSnapshot` used to hold these three values as three flat
//! fields, each threaded separately through its own positional constructor
//! argument. Bundling them into one value here is a real simplification for
//! `SceneSnapshot`: its own identity collapses from three fields to one
//! (`stamp: FrameStamp`).
//!
//! # What bundling does *not* buy: construction is not additive
//!
//! An earlier revision of this type used a typestate builder on the claim
//! that it would make a *future* field addition to `FrameStamp` non-
//! breaking. That claim was tested — by literally adding a field — and was
//! false: no value-type construction shape in Rust, positional constructor
//! or typestate builder alike, makes adding a *required* field additive.
//! Additivity is available only for *optional* fields, and ADR-0045
//! decision 4's [`GpuResourceGeneration`] axis cannot be optional (the
//! decision requires both generations to be checked before a frame
//! renders). That field landed for real in the same change as this
//! paragraph's correction (not merely tested and reverted): adding it
//! changed [`FrameStamp::new`]'s signature and broke every call site that
//! called it — **six, across three crates**, each named directly by the
//! compiler as an arity mismatch: this module's own unit test and its
//! positive doctest (`flui-foundation`); `scene_snapshot.rs`'s stamp helper
//! (`flui-layer`); and the `raster_owner.rs` test helper, the
//! `raster_backpressure` bench and the `raster_backpressure_allocation`
//! integration test (`flui-engine`). The `compile_fail` doctest below was a
//! seventh site that needed different handling — see its own note, updated
//! in the same change rather than left describing a field that had not
//! landed yet. An earlier revision of this paragraph said "three, all in
//! `flui-engine`"; the compiler named six, and a paragraph whose whole
//! purpose is to be the trustworthy version of a claim that was previously
//! untrustworthy has to survive being checked.
//! That is a real, compiler-guided, small-blast-radius breaking change —
//! narrowed from five positional arguments (`SceneSnapshot`'s own former
//! shape) to four, not eliminated — and no amount of builder ceremony
//! changes that, so this type uses the plain constructor the honest version
//! of that claim implies.

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
/// This struct is deliberately `#[non_exhaustive]`. That constrains
/// *matching* from outside this crate — an external struct-literal pattern
/// without `..` fails to compile — never construction; see
/// [`FrameStamp::new`]'s own doc for why no annotation makes a future
/// required field additive to construction. The benefit is real but
/// narrow: this workspace has no consumer of `FrameStamp` outside this
/// crate that destructures it today, so the guard is not yet load-bearing,
/// but it costs nothing (no destructuring pattern exists anywhere in this
/// workspace for it to complicate) and it is scoped honestly to the one
/// thing `#[non_exhaustive]` actually buys.
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
    /// The `flui-engine` `GpuServices` generation this frame was produced
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
    /// let stamp = FrameStamp::new(
    ///     PresentationAddress {
    ///         realm_id: RealmId::new(1),
    ///         presentation_id: PresentationId::new(1),
    ///     },
    ///     FrameEpoch::ZERO,
    ///     SurfaceGeneration::ZERO,
    ///     GpuResourceGeneration::ZERO,
    /// );
    ///
    /// assert_eq!(stamp.epoch, FrameEpoch::ZERO);
    /// ```
    ///
    /// Swapping `epoch` and `surface_generation` — the two fields whose
    /// underlying representation looks alike (both wrap a `u64` counter) —
    /// does not compile:
    ///
    /// **This block went vacuous once, and was fixed in the same change**
    /// that added `gpu_resource_generation` as the fourth field: it would
    /// otherwise fail on arity rather than on the type transposition its
    /// comment names, reporting success while testing nothing. It is kept
    /// at four arguments (transposing the same two as before, with the new
    /// fourth argument supplied correctly) precisely so the failure stays
    /// `E0308` on the transposed pair rather than becoming `E0061` on the
    /// count — re-confirmed by extraction against this crate's own source
    /// after the field landed, not assumed from the shape of the diff.
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
    /// // ERROR[E0308]: expected `FrameEpoch`, found `SurfaceGeneration` —
    /// // the two arguments are transposed, and each is a distinct newtype.
    /// let _ = FrameStamp::new(
    ///     address,
    ///     SurfaceGeneration::ZERO,
    ///     FrameEpoch::ZERO,
    ///     GpuResourceGeneration::ZERO,
    /// );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{PresentationId, RealmId};

    fn test_address() -> PresentationAddress {
        PresentationAddress {
            realm_id: RealmId::new(1),
            presentation_id: PresentationId::new(1),
        }
    }

    #[test]
    fn new_packages_all_fields() {
        let address = test_address();
        let epoch = FrameEpoch::ZERO.next();
        let surface_generation = SurfaceGeneration::ZERO.next();
        let gpu_resource_generation = GpuResourceGeneration::mint();

        let stamp = FrameStamp::new(address, epoch, surface_generation, gpu_resource_generation);

        assert_eq!(stamp.address, address);
        assert_eq!(stamp.epoch, epoch);
        assert_eq!(stamp.surface_generation, surface_generation);
        assert_eq!(stamp.gpu_resource_generation, gpu_resource_generation);
    }

    #[test]
    fn frame_stamp_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FrameStamp>();
    }
}
