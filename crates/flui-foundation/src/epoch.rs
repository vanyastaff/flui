//! Monotonic generation/version counters for the owner-affine ui-realm
//! protocol.
//!
//! Three distinct newtypes guard three distinct kinds of staleness. They are
//! **not interchangeable** — each is checked against its own authority, and
//! mixing them up (e.g. comparing a [`SurfaceGeneration`] to a
//! [`ResourceGeneration`]) is a compile error, not a bug waiting to happen:
//!
//! - [`FrameEpoch`] — a runtime's per-frame counter. Meaningful only within
//!   one `UiRealm` lifetime; a worker result declares the epoch it was
//!   computed for, and the owner drops it at commit if the epoch is stale.
//! - [`SurfaceGeneration`] — bumped every time the raster surface is
//!   (re)configured (resize, device-lost recovery); guards frames in flight
//!   against a surface that no longer exists.
//! - [`ResourceGeneration`] — generational identity for worker-produced
//!   cache resources (image decode, tessellation, glyph shaping); guards a
//!   cache slot against being read after its producer moved on.
//!
//! [`GenerationGate`] is the shared, `Clone`-able commit-time check that
//! stamps and later validates a [`ResourceGeneration`]: a job stamps the
//! gate's current generation at dispatch, and the owner's `bump()` on
//! resource invalidation makes every outstanding stamp fail
//! `is_current()` in one step.
//!
//! Channel identity — not epoch arithmetic — is the isolation boundary across
//! `UiRealm` recreation: these counters are never compared
//! across two different runtimes' lifetimes, only within one.
//!
//! # Example
//!
//! ```rust
//! use flui_foundation::FrameEpoch;
//!
//! let mut epoch = FrameEpoch::ZERO;
//! assert_eq!(epoch.get(), 0);
//!
//! epoch = epoch.next();
//! assert_eq!(epoch.get(), 1);
//! assert!(epoch > FrameEpoch::ZERO);
//! ```

use core::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Defines a monotonic `u64` generation/version counter newtype.
///
/// Every generated type shares the same shape (construction, ordering,
/// `next()`, `Display`) but is a distinct type — the macro exists to keep
/// [`FrameEpoch`], [`SurfaceGeneration`], and [`ResourceGeneration`] from
/// drifting out of sync, not to make them convertible into one another.
macro_rules! epoch_counters {
    ($(
        $(#[$meta:meta])*
        pub struct $name:ident;
    )*) => {
        $(
            $(#[$meta])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
            pub struct $name(u64);

            impl $name {
                /// The initial value: a fresh runtime's first frame, a
                /// freshly-configured surface, a freshly-minted resource.
                pub const ZERO: Self = Self(0);

                /// The next value in this monotonic sequence.
                ///
                /// Plain `self.0 + 1`, not `wrapping_add`: wrapping back to 0
                /// would alias a live earlier value as current, which is the
                /// exact staleness bug this type exists to prevent. No
                /// existing counter in this codebase increments a bare `u64`
                /// newtype this way to crib from (`Scene::frame_number` is a
                /// passively-set field with no self-increment method;
                /// `AsyncDriver`/`IdGenerator` increment an `AtomicU64` via
                /// `fetch_add`, a different mechanism for a different,
                /// concurrently-shared counter). A per-runtime frame/surface/
                /// resource counter increments at most once per frame, so
                /// reaching `u64::MAX` is not a practical concern; debug
                /// builds still panic on the (unreachable) overflow rather
                /// than silently wrapping.
                #[must_use]
                pub const fn next(self) -> Self {
                    Self(self.0 + 1)
                }

                /// The raw counter value.
                #[must_use]
                pub const fn get(self) -> u64 {
                    self.0
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    fmt::Display::fmt(&self.0, f)
                }
            }
        )*
    };
}

epoch_counters! {
    /// A `UiRealm`'s monotonic per-frame counter.
    ///
    /// Minted by the runtime's scheduler; subsumes `Scene::frame_number`
    /// (one fact, one place — `Scene` keeps its field for now but the
    /// ui-realm protocol threads `FrameEpoch` instead). Meaningful only
    /// within one `UiRealm` lifetime: recreating a realm creates a new
    /// runtime with a new `FrameEpoch` sequence starting at
    /// [`FrameEpoch::ZERO`] again, and channel identity (not epoch
    /// comparison) is what keeps a stale runtime's results from being
    /// mistaken for the new one's.
    ///
    /// A worker job carries the `FrameEpoch` it was issued under; at commit
    /// (Idle drain) the owner applies the result only if that epoch is still
    /// current, otherwise it is dropped with a trace event, never a panic.
    pub struct FrameEpoch;

    /// Generation counter bumped on every raster surface (re)configure:
    /// resize, device-lost recovery, or any other event that
    /// invalidates the previously-configured surface.
    ///
    /// Owned by the engine seam. A frame in flight declares the
    /// `SurfaceGeneration` it was produced for; the raster owner rejects (and
    /// acks `SurfaceOutdated`) a frame whose generation no longer matches the
    /// currently-configured surface instead of presenting into a torn-down
    /// swapchain.
    pub struct SurfaceGeneration;

    /// Generational identity for worker-produced cache resources — image
    /// decode, tessellation, and text-shaping cache slots — following the
    /// same generational pattern as [`GenId`](crate::GenId).
    ///
    /// Guards a cache slot against being read after its producer moved past
    /// it: a worker result is applied at commit only if its declared
    /// `ResourceGeneration` still matches the slot's current generation.
    pub struct ResourceGeneration;
}

/// The canonical commit-time freshness gate for worker results.
///
/// A worker result is applied only if its declared [`ResourceGeneration`] is
/// still current on the gate it was stamped against; otherwise the owner
/// drops it with a trace event, never a panic. Same shape as the `AsyncSlot`
/// stale-result pattern (`flui-view/src/element/async_slot.rs`'s `Slot::generation`:
/// bumped on every resubscription, and a write whose generation no longer
/// matches is discarded) generalized to `ResourceGeneration` and shared via a
/// `Clone`-able handle instead of a private field on one slot.
///
/// # Sharing semantics
///
/// `Clone` shares the *same* gate — every clone reads and invalidates the one
/// underlying counter. That is the point: a job stamps
/// [`GenerationGate::current`] at dispatch time by cloning the gate into its
/// stamp, and the owner's later [`GenerationGate::bump`] on the original
/// handle invalidates every outstanding clone's stamp in one step, with no
/// coordination between them beyond sharing the `Arc`.
#[derive(Debug, Clone)]
pub struct GenerationGate(Arc<AtomicU64>);

impl GenerationGate {
    /// A gate whose current generation is [`ResourceGeneration::ZERO`].
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(AtomicU64::new(ResourceGeneration::ZERO.get())))
    }

    /// The generation currently accepted as fresh.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::GenerationGate;
    ///
    /// let gate = GenerationGate::new();
    ///
    /// // A job stamps the gate's current generation at dispatch time.
    /// let stamp = gate.current();
    /// assert!(gate.is_current(stamp));
    ///
    /// // The resource the job depends on is invalidated (e.g. a repaint
    /// // boundary re-images): bump the gate.
    /// let fresh = gate.bump();
    /// assert!(gate.is_current(fresh));
    ///
    /// // The job's stamp — captured before the bump — is now stale.
    /// assert!(!gate.is_current(stamp));
    /// ```
    #[must_use]
    pub fn current(&self) -> ResourceGeneration {
        ResourceGeneration(self.0.load(Ordering::Relaxed))
    }

    /// Invalidates every outstanding generation and mints the next one.
    ///
    /// Every stamp captured via [`Self::current`] before this call now fails
    /// [`Self::is_current`], including stamps held by other clones of this
    /// gate (they share the same counter).
    #[must_use]
    pub fn bump(&self) -> ResourceGeneration {
        // `Relaxed` throughout this type: the gate has a single logical
        // writer (the owner thread) and readers only ever
        // compare a previously-read `ResourceGeneration` for equality
        // against the live one — no other memory is published through this
        // atomic that a reader must observe as a consequence of seeing a
        // particular value, so `Acquire`/`Release` would buy nothing here.
        let previous = self.0.fetch_add(1, Ordering::Relaxed);
        ResourceGeneration(previous + 1)
    }

    /// Whether `generation` still matches this gate's current generation.
    #[must_use]
    pub fn is_current(&self, generation: ResourceGeneration) -> bool {
        generation == self.current()
    }
}

impl Default for GenerationGate {
    /// Equivalent to [`GenerationGate::new`].
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameEpoch, GenerationGate, ResourceGeneration, SurfaceGeneration};

    #[test]
    fn zero_is_default() {
        assert_eq!(FrameEpoch::default(), FrameEpoch::ZERO);
        assert_eq!(FrameEpoch::ZERO.get(), 0);
    }

    #[test]
    fn next_increments_by_one() {
        let first = FrameEpoch::ZERO;
        let second = first.next();
        assert_eq!(second.get(), 1);
        assert_eq!(second.next().get(), 2);
    }

    #[test]
    fn ordering_tracks_recency() {
        let older = SurfaceGeneration::ZERO;
        let newer = older.next();
        assert!(newer > older);
        assert_eq!(older, SurfaceGeneration::ZERO);
    }

    #[test]
    fn distinct_types_do_not_mix() {
        // This is a compile-time property, not a runtime assertion: the
        // following would not compile if uncommented, because the three
        // counters are distinct types despite identical internal shape.
        // let _: FrameEpoch = SurfaceGeneration::ZERO; // ERROR
        let frame = FrameEpoch::ZERO;
        let resource = ResourceGeneration::ZERO;
        assert_eq!(frame.get(), resource.get());
    }

    #[test]
    fn display_shows_raw_value() {
        assert_eq!(FrameEpoch::ZERO.next().next().to_string(), "2");
    }

    // -----------------------------------------------------------------------
    // GenerationGate tests
    // -----------------------------------------------------------------------

    #[test]
    fn generation_gate_starts_at_zero() {
        let gate = GenerationGate::new();
        assert_eq!(gate.current(), ResourceGeneration::ZERO);
        assert_eq!(
            GenerationGate::default().current(),
            ResourceGeneration::ZERO
        );
    }

    #[test]
    fn stale_after_bump() {
        let gate = GenerationGate::new();
        let stamp = gate.current();
        assert!(gate.is_current(stamp));

        let fresh = gate.bump();
        assert_ne!(fresh, stamp);
        assert!(gate.is_current(fresh));
        assert!(!gate.is_current(stamp), "pre-bump stamp must go stale");
    }

    #[test]
    fn clone_shares_state() {
        let gate = GenerationGate::new();
        let clone = gate.clone();

        // A bump through one handle is visible through every clone: they
        // share one underlying counter, not independent copies.
        let bumped = gate.bump();
        assert_eq!(clone.current(), bumped);

        let stamp_via_clone = clone.current();
        let bumped_again = clone.bump();
        assert!(!gate.is_current(stamp_via_clone));
        assert!(gate.is_current(bumped_again));
    }

    #[test]
    fn generation_gate_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<GenerationGate>();
    }
}
