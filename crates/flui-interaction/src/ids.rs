//! Type-safe identifiers used by the gesture/interaction subsystem.
//!
//! [`PointerId`] and [`DeviceId`] are the exact owned contract identities from
//! `flui-platform-api` (ADR-0143). Pointer identity names a contact; device
//! identity names connected hardware. Neither is a truncated hash or a signed
//! compatibility label. Primary contact status lives in `PointerInfo`, not an ID.
//!
//! # Local IDs
//!
//! [`FocusNodeId`] is issued by the focus-node allocator.
//! Callers retain these identities rather than constructing them.
//!
//! # Example
//!
//! ```rust
//! use flui_interaction::{FocusNode, PointerId};
//!
//! let mouse = PointerId::try_from(1_u64).expect("nonzero pointer id");
//! let touch1 = PointerId::try_from(2_u64).expect("nonzero pointer id");
//!
//! assert_ne!(mouse, touch1);
//!
//! let focus = FocusNode::new().id();
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
/// Issued when a [`crate::FocusNode`] is created.
///
/// # Example
///
/// ```rust
/// use flui_interaction::FocusNode;
///
/// let text_field = FocusNode::new();
/// let button = FocusNode::new();
///
/// assert_ne!(text_field.id(), button.id());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct FocusNodeId(NonZeroU64);

impl FocusNodeId {
    #[inline]
    pub(crate) const fn new(id: NonZeroU64) -> Self {
        Self(id)
    }

    /// Returns the raw ID value.
    #[inline]
    pub const fn get(self) -> u64 {
        self.0.get()
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

impl From<FocusNodeId> for NonZeroU64 {
    #[inline]
    fn from(id: FocusNodeId) -> Self {
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
