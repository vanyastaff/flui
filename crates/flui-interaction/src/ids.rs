//! Type-safe identifiers used by the gesture/interaction subsystem.
//!
//! [`PointerId`] and [`DeviceId`] are the exact owned contract identities from
//! `flui-platform-api` (ADR-0143). Pointer identity names a contact; device
//! identity names connected hardware. Neither is a truncated hash or a signed
//! compatibility label. Primary contact status lives in `PointerInfo`, not an ID.
//!
//! # Local IDs
//!
//! [`FocusNodeId`] and [`HandlerId`] remain local — they back their own
//! crate-private slab/registry indexing and do not touch platform layers.
//!
//! # Example
//!
//! ```rust
//! use flui_interaction::ids::{PointerId, FocusNodeId};
//!
//! let mouse = PointerId::try_from(1_u64).expect("nonzero pointer id");
//! let touch1 = PointerId::try_from(2_u64).expect("nonzero pointer id");
//!
//! assert_ne!(mouse, touch1);
//!
//! let focus = FocusNodeId::new(42);
//! // PointerId and FocusNodeId are distinct types — cannot be mixed.
//! ```

use std::{fmt, num::NonZeroU64};

// ============================================================================
// PointerId — the owned platform contract identity
// ============================================================================

/// Unique identifier for a pointer device (mouse, touch, stylus).
///
/// Re-exported from [`flui_platform_api::pointer::PointerId`].
pub use flui_platform_api::pointer::PointerId;

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
/// Re-exported from [`flui_platform_api::pointer::DeviceId`].
pub use flui_platform_api::pointer::DeviceId;

// ============================================================================
// RegionId - Identifier for mouse regions
// ============================================================================

/// Unique identifier for a mouse-sensitive region.
///
/// Re-exported from `flui_foundation::RenderId` since regions correspond to
/// render objects (hit-testable visual elements).
pub use flui_foundation::RenderId as RegionId;
