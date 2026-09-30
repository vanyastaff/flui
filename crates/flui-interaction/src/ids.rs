//! Type-safe identifiers used by the gesture/interaction subsystem.
//!
//! # `PointerId` re-export
//!
//! The canonical [`PointerId`] is **re-exported** from the `ui-events` crate
//! (W3C-compliant pointer event types). This crate previously carried a local
//! `PointerId(i32)` newtype which:
//!
//! - Used `0` as the "mouse / primary pointer" sentinel.
//! - Duplicated a per-event `DefaultHasher` allocation on every event in
//!   `extract_pointer_id` to fit `ui_events::pointer::PointerId(NonZeroU64)`
//!   back into a 32-bit `i32`.
//! - Caused HashMap key collisions between two pointers that hashed to the
//!   same 31-bit truncated value.
//!
//! Widening the local type to [`ui_events::pointer::PointerId`] (i.e.
//! `NonZeroU64`) removes the lossy hash and aligns the gesture layer with
//! the platform layer ([`flui-platform`](crate)) which already speaks
//! `ui_events` directly.
//!
//! ## Constructor migration
//!
//! `ui_events::pointer::PointerId::new` is **fallible** — it returns
//! `Option<PointerId>` because `0` is not a valid id (the inner type is
//! `NonZeroU64`). Callers that previously wrote `PointerId::new(0)` must use
//! [`PointerId::PRIMARY`] instead (the canonical primary pointer id, value
//! `1`). Callers that previously wrote `PointerId::new(N)` for `N >= 1`
//! should use `PointerId::new((N as u64) + 1).expect("nonzero pointer id")`
//! — adding `1` keeps test pointers distinct from `PRIMARY`.
//!
//! Flutter parity: `gestures/events.dart::PointerEvent.pointer` is an
//! unbounded `int`; `PointerId::PRIMARY` corresponds to Flutter's
//! mouse/primary pointer convention.
//!
//! # Local IDs
//!
//! [`FocusNodeId`] and [`HandlerId`] remain local — they back their own
//! crate-private slab/registry indexing and do not touch platform layers.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::ids::{PointerId, FocusNodeId};
//!
//! // Primary pointer (was: `PointerId::new(0)`).
//! let mouse = PointerId::PRIMARY;
//! // Second pointer in a multi-touch gesture.
//! let touch1 = PointerId::new(2).expect("nonzero pointer id");
//!
//! assert_ne!(mouse, touch1);
//!
//! let focus = FocusNodeId::new(42);
//! // PointerId and FocusNodeId are distinct types — cannot be mixed.
//! ```

use std::{fmt, num::NonZeroU64};

// ============================================================================
// PointerId — re-exported from ui-events (canonical W3C-compliant type)
// ============================================================================

/// Unique identifier for a pointer device (mouse, touch, stylus).
///
/// Re-exported from [`ui_events::pointer::PointerId`]. See [the module
/// documentation](self) for migration notes (the local `i32` newtype
/// was widened to this `NonZeroU64`-backed type).
pub use ui_events::pointer::PointerId;

// ============================================================================
// FocusNodeId - Identifier for focusable UI elements
// ============================================================================

/// Unique identifier for a focusable UI element.
///
/// Uses `NonZeroU64` for niche optimization: `Option<FocusNodeId>` is same
/// size.
///
/// # Example
///
/// ```rust
/// use flui_interaction::ids::FocusNodeId;
///
/// let text_field = FocusNodeId::new(1);
/// let button = FocusNodeId::new(2);
///
/// // Option<FocusNodeId> is still 8 bytes due to niche optimization
/// assert_eq!(
///     std::mem::size_of::<Option<FocusNodeId>>(),
///     std::mem::size_of::<FocusNodeId>()
/// );
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct FocusNodeId(NonZeroU64);

impl FocusNodeId {
    /// Creates a new focus node ID.
    ///
    /// # Panics
    ///
    /// Panics if `id` is 0. Use `try_new` for fallible construction.
    #[inline]
    pub fn new(id: u64) -> Self {
        Self(NonZeroU64::new(id).expect("FocusNodeId cannot be 0"))
    }

    /// Creates a new focus node ID, returning `None` if `id` is 0.
    #[inline]
    pub const fn try_new(id: u64) -> Option<Self> {
        match NonZeroU64::new(id) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }

    /// Returns the raw ID value.
    #[inline]
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// Creates a FocusNodeId from a NonZeroU64.
    #[inline]
    pub const fn from_non_zero(nz: NonZeroU64) -> Self {
        Self(nz)
    }
}

impl fmt::Debug for FocusNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FocusNodeId({})", self.0)
    }
}

impl fmt::Display for FocusNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "focus:{}", self.0)
    }
}

impl From<NonZeroU64> for FocusNodeId {
    #[inline]
    fn from(nz: NonZeroU64) -> Self {
        Self(nz)
    }
}

impl From<FocusNodeId> for NonZeroU64 {
    #[inline]
    fn from(id: FocusNodeId) -> Self {
        id.0
    }
}

// ============================================================================
// HandlerId - Identifier for registered handlers
// ============================================================================

/// Unique identifier for a registered event handler.
///
/// Used by signal resolver and other registration systems.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct HandlerId(NonZeroU64);

impl HandlerId {
    /// Creates a new handler ID.
    ///
    /// # Panics
    ///
    /// Panics if `id` is 0. Use [`try_new`](Self::try_new) for fallible
    /// construction from an untrusted source.
    #[inline]
    pub fn new(id: u64) -> Self {
        Self(NonZeroU64::new(id).expect("HandlerId cannot be 0"))
    }

    /// Creates a new handler ID, returning `None` if `id` is 0.
    #[inline]
    pub const fn try_new(id: u64) -> Option<Self> {
        match NonZeroU64::new(id) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }

    /// Returns the raw ID value.
    #[inline]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for HandlerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HandlerId({})", self.0)
    }
}

impl fmt::Display for HandlerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "handler:{}", self.0)
    }
}

impl From<NonZeroU64> for HandlerId {
    #[inline]
    fn from(nz: NonZeroU64) -> Self {
        Self(nz)
    }
}

impl From<HandlerId> for NonZeroU64 {
    #[inline]
    fn from(id: HandlerId) -> Self {
        id.0
    }
}

// ============================================================================
// DeviceId - Identifier for input devices (mouse tracker)
// ============================================================================

/// Unique identifier for an input device.
///
/// Alias for mouse tracker compatibility.
pub type DeviceId = i32;

// ============================================================================
// RegionId - Identifier for mouse regions
// ============================================================================

/// Unique identifier for a mouse-sensitive region.
///
/// Re-exported from `flui_foundation::RenderId` since regions correspond to
/// render objects (hit-testable visual elements).
pub use flui_foundation::RenderId as RegionId;
