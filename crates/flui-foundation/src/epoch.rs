//! Monotonic counters for the owner-affine UI runtime protocol.
//!
//! Two newtypes guard two kinds of staleness, each checked against its own
//! authority, so comparing a [`FrameEpoch`] with a [`SurfaceGeneration`] does
//! not compile:
//!
//! - [`FrameEpoch`] — a runtime's per-frame counter. Meaningful only within
//!   one `UiRuntime` lifetime; the raster side settles a frame by it.
//! - [`SurfaceGeneration`] — bumped every time the raster surface is
//!   (re)configured (resize, device-lost recovery); guards frames in flight
//!   against a surface that no longer exists.
//!
//! Channel identity — not epoch arithmetic — is the isolation boundary across
//! `UiRuntime` recreation: these counters are never compared
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

/// Defines a monotonic `u64` generation/version counter newtype.
///
/// Every generated type shares the same shape (construction, ordering,
/// `next()`, `Display`) but is a distinct type — the macro exists to keep
/// [`FrameEpoch`] and [`SurfaceGeneration`] from
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
                /// passively-set field with no self-increment method; ids a
                /// second thread mints go through `AtomicIdMint`'s
                /// `compare_exchange`, a different mechanism for a different,
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
    /// A `UiRuntime`'s monotonic per-frame counter.
    ///
    /// The UI runtime mints the next one for each frame any of its
    /// presentations hands a sink, so a frame that never reaches a sink takes
    /// none, and the raster side settles a frame by the
    /// [`FrameStamp`](crate::FrameStamp) that pairs it with the presentation's
    /// address (ADR-0305 §4). Meaningful only within one `UiRuntime`
    /// lifetime: recreating a UI runtime creates a new runtime with a new
    /// sequence starting at [`FrameEpoch::ZERO`] again, and the address's
    /// generational runtime id (not epoch comparison) is what keeps a stale
    /// runtime's results from being mistaken for the new one's.
    pub struct FrameEpoch;

    /// Generation counter bumped on every raster surface (re)configure:
    /// resize, device-lost recovery, or any other event that
    /// invalidates the previously-configured surface.
    ///
    /// Owned by the engine seam. A frame in flight declares the
    /// `SurfaceGeneration` it was produced for; the raster executor refuses
    /// (and settles `SurfaceStale`) a frame whose generation no longer matches the
    /// currently-configured surface instead of presenting into a torn-down
    /// swapchain.
    pub struct SurfaceGeneration;
}

/// The counters a recording writes and reads back: a presentation report
/// names its frame by the frame's epoch and surface generation.
macro_rules! recorded_counters {
    ($($name:ident),*) => {
        $(
            impl $name {
                /// The value whose raw counter is `value`, as
                /// [`get`](Self::get) read it: what a recording that wrote
                /// the counter reads back. Nothing checks that a counter ever
                /// reached it.
                #[must_use]
                pub const fn from_raw(value: u64) -> Self {
                    Self(value)
                }
            }
        )*
    };
}

recorded_counters!(FrameEpoch, SurfaceGeneration);
