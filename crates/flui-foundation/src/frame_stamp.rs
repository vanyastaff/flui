//! [`FrameStamp`] — the frame-identity group threaded through the raster
//! boundary: which presentation produced a frame, at what per-realm epoch,
//! against which raster surface configuration.
//!
//! # Why a bundled type instead of positional constructor arguments
//!
//! `flui_layer::SceneSnapshot` used to accept these three values as three
//! positional [`SceneSnapshot::new`](../../flui_layer/struct.SceneSnapshot.html)
//! arguments. `#[non_exhaustive]` on that struct made *matching* on it
//! additive, but not *construction*: every field the type gained broke every
//! external call site, because a positional constructor has no room to grow.
//! Bundling the identity fields into one value here — constructed only
//! through [`FrameStamp::builder`] — is what makes the *next* identity axis
//! additive instead of breaking.
//!
//! # The next axis, named so this type is shaped for it now
//!
//! ADR-0045 decision 4 adds a second identity axis once GPU-services
//! generation gating lands: `ResourceGeneration`, checked alongside
//! [`SurfaceGeneration`] before a frame renders. Adding that field here
//! widens [`FrameStampBuilder`] with one more typestate slot and one more
//! setter; it touches no existing call site, because every existing call
//! site already goes through the builder rather than a positional
//! constructor.

use crate::epoch::{FrameEpoch, SurfaceGeneration};
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
/// frame counting.
///
/// # Construction
///
/// Build through [`FrameStamp::builder`]. `#[non_exhaustive]` blocks
/// struct-literal construction from outside this crate deliberately, so a
/// future field here is additive rather than breaking every caller — see the
/// module doc for the specific axis this is already shaped for.
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
}

impl FrameStamp {
    /// Starts building a [`FrameStamp`]. Every field is required; the
    /// returned builder only exposes [`FrameStampBuilder::build`] once
    /// [`FrameStampBuilder::address`], [`FrameStampBuilder::epoch`], and
    /// [`FrameStampBuilder::surface_generation`] have all been called —
    /// enforced at compile time via the builder's typestate, not by a
    /// runtime check or a panicking default.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::{
    ///     FrameEpoch, FrameStamp, PresentationAddress, PresentationId, RealmId,
    ///     SurfaceGeneration,
    /// };
    ///
    /// let stamp = FrameStamp::builder()
    ///     .address(PresentationAddress {
    ///         realm_id: RealmId::new(1),
    ///         presentation_id: PresentationId::new(1),
    ///     })
    ///     .epoch(FrameEpoch::ZERO)
    ///     .surface_generation(SurfaceGeneration::ZERO)
    ///     .build();
    ///
    /// assert_eq!(stamp.epoch, FrameEpoch::ZERO);
    /// ```
    #[must_use]
    pub fn builder() -> FrameStampBuilder {
        FrameStampBuilder::new()
    }
}

/// Builder for [`FrameStamp`].
///
/// A typestate builder: each of the three setters is offered only while its
/// own slot is still the unit type `()` (unfilled), and [`Self::build`] is
/// offered only once every slot holds its real value. Calling the setters in
/// any order reaches the same buildable state — each setter constrains only
/// its own slot. Forgetting a field is a compile error, not a runtime panic:
/// there is no sensible default for a frame's own identity to fall back on.
#[derive(Debug, Clone, Copy)]
pub struct FrameStampBuilder<Address = (), Epoch = (), SurfaceGen = ()> {
    address: Address,
    epoch: Epoch,
    surface_generation: SurfaceGen,
}

impl FrameStampBuilder {
    fn new() -> Self {
        Self {
            address: (),
            epoch: (),
            surface_generation: (),
        }
    }
}

impl Default for FrameStampBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl<Epoch, SurfaceGen> FrameStampBuilder<(), Epoch, SurfaceGen> {
    /// Sets the frame's realm+presentation address.
    #[must_use]
    pub fn address(
        self,
        address: PresentationAddress,
    ) -> FrameStampBuilder<PresentationAddress, Epoch, SurfaceGen> {
        FrameStampBuilder {
            address,
            epoch: self.epoch,
            surface_generation: self.surface_generation,
        }
    }
}

impl<Address, SurfaceGen> FrameStampBuilder<Address, (), SurfaceGen> {
    /// Sets the frame's per-realm epoch.
    #[must_use]
    pub fn epoch(self, epoch: FrameEpoch) -> FrameStampBuilder<Address, FrameEpoch, SurfaceGen> {
        FrameStampBuilder {
            address: self.address,
            epoch,
            surface_generation: self.surface_generation,
        }
    }
}

impl<Address, Epoch> FrameStampBuilder<Address, Epoch, ()> {
    /// Sets the raster surface generation this frame was produced against.
    #[must_use]
    pub fn surface_generation(
        self,
        surface_generation: SurfaceGeneration,
    ) -> FrameStampBuilder<Address, Epoch, SurfaceGeneration> {
        FrameStampBuilder {
            address: self.address,
            epoch: self.epoch,
            surface_generation,
        }
    }
}

impl FrameStampBuilder<PresentationAddress, FrameEpoch, SurfaceGeneration> {
    /// Builds the [`FrameStamp`]. Only reachable once every field has been
    /// set — see the type's own doc.
    #[must_use]
    pub fn build(self) -> FrameStamp {
        FrameStamp {
            address: self.address,
            epoch: self.epoch,
            surface_generation: self.surface_generation,
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
    fn builder_round_trips_every_field() {
        let stamp = FrameStamp::builder()
            .address(test_address())
            .epoch(FrameEpoch::ZERO.next())
            .surface_generation(SurfaceGeneration::ZERO.next())
            .build();

        assert_eq!(stamp.address, test_address());
        assert_eq!(stamp.epoch, FrameEpoch::ZERO.next());
        assert_eq!(stamp.surface_generation, SurfaceGeneration::ZERO.next());
    }

    #[test]
    fn setters_are_order_independent() {
        let built_address_first = FrameStamp::builder()
            .address(test_address())
            .epoch(FrameEpoch::ZERO)
            .surface_generation(SurfaceGeneration::ZERO)
            .build();
        let built_surface_generation_first = FrameStamp::builder()
            .surface_generation(SurfaceGeneration::ZERO)
            .epoch(FrameEpoch::ZERO)
            .address(test_address())
            .build();

        assert_eq!(built_address_first, built_surface_generation_first);
    }

    #[test]
    fn frame_stamp_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FrameStamp>();
    }
}
