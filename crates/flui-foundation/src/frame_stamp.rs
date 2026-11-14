//! [`FrameStamp`] bundles the typed presentation address, per-presentation frame
//! epoch and raster surface generation that cross the raster boundary
//! together.
//!
//! A frame carries CPU data only: every GPU object it needs is derived inside
//! the raster executor, which derives them again after a device rebuild, so no
//! axis of the stamp names a device (ADR-0305 §6).
//!
//! [`FrameStamp::new`] takes these three values directly. A builder would not
//! make adding another required value compatible with existing callers:
//! each caller must still supply it. The plain constructor exposes that
//! requirement without another construction protocol.

use crate::epoch::{FrameEpoch, SurfaceGeneration};
use crate::id::PresentationAddress;

/// The identity group that stamps one frame: which presentation produced it,
/// at what epoch of that presentation, against which raster surface configuration.
///
/// # Frame identity
///
/// A frame's full identity is `(address, epoch)`, never `epoch` alone:
/// [`FrameEpoch`] counts each presentation's frames on its own, so two presentations
/// of the same UI runtime's forest stamp the same epoch on different frames —
/// `address` disambiguates them. `address` is the full `(ui_runtime_id,
/// presentation_id)` pair — never `presentation_id` alone, since two
/// different UI runtime incarnations can mint an identical `PresentationId` and
/// only the full pair safely distinguishes them. `surface_generation` is a
/// separate axis, scoped *per presentation*: it is minted by that
/// presentation's own raster seam (ADR-0037 §8), never by the UI runtime or by
/// frame counting. A device rebuild advances the generation of every slot on
/// the rebuilt device, so a frame stamped before it is refused by the same
/// comparison (ADR-0305 §8).
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
    /// The full UI runtime+presentation address that composited this frame.
    pub address: PresentationAddress,
    /// The presentation's per-frame counter at the time this frame was
    /// composited.
    pub epoch: FrameEpoch,
    /// The raster surface generation this frame was produced against.
    pub surface_generation: SurfaceGeneration,
}

impl FrameStamp {
    /// Packages the three identity/versioning fields the raster boundary
    /// needs to accept, reject, or reconcile a frame.
    ///
    /// The fields are distinct newtypes (never the same underlying type as
    /// one another), so transposing two arguments is a compile-time type
    /// error, not a silent bug — see the second example below.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::{
    ///     FrameEpoch, FrameStamp, PresentationAddress, PresentationId, UiRuntimeId,
    ///     SurfaceGeneration,
    /// };
    ///
    /// let address = PresentationAddress {
    ///     ui_runtime_id: UiRuntimeId::new(1),
    ///     presentation_id: PresentationId::new(1),
    /// };
    /// let stamp = FrameStamp::new(address, FrameEpoch::ZERO, SurfaceGeneration::ZERO);
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
    ///     FrameEpoch, FrameStamp, PresentationAddress, PresentationId, UiRuntimeId,
    ///     SurfaceGeneration,
    /// };
    ///
    /// let address = PresentationAddress {
    ///     ui_runtime_id: UiRuntimeId::new(1),
    ///     presentation_id: PresentationId::new(1),
    /// };
    /// let stamp = FrameStamp::new(address, SurfaceGeneration::ZERO, FrameEpoch::ZERO);
    ///
    /// assert_eq!(stamp.epoch, FrameEpoch::ZERO);
    /// ```
    #[must_use]
    pub fn new(
        address: PresentationAddress,
        epoch: FrameEpoch,
        surface_generation: SurfaceGeneration,
    ) -> Self {
        Self {
            address,
            epoch,
            surface_generation,
        }
    }
}
