//! Type-safe identifiers used by the gesture/interaction subsystem.
//!
//! [`PointerId`] and [`DeviceId`] are the exact owned contract identities from
//! `flui-platform` (ADR-0143). Pointer identity names a contact; device
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
/// Re-exported from [`flui_platform::pointer::PointerId`].
pub use flui_platform::pointer::PointerId;

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
/// Re-exported from [`flui_platform::pointer::DeviceId`].
pub use flui_platform::pointer::DeviceId;

// ============================================================================
// InputSurface - the routing root of an input stream
// ============================================================================

/// The routing root an input stream arrives on: a presentation's window, or a
/// surface embedded in it (ADR-0207 §5).
///
/// Contact sequences, hover, move coalescing and wheel and pan-zoom latches are
/// keyed by the surface, never by a process-wide "current pointer". A contact
/// stays with the surface that admitted its Down: the rest of the sequence is
/// delivered there wherever the platform reports it.
///
/// ```rust
/// use flui_interaction::{GestureBinding, InputSurface};
///
/// let binding = GestureBinding::new();
/// let embedded = binding.embed_surface()?;
/// assert_ne!(embedded, InputSurface::Window);
/// # Ok::<(), flui_interaction::SurfaceIdsExhausted>(())
/// ```
///
/// The set is closed, as ADR-0207 §5 states it: a new kind of routing root
/// would change every owner keyed by the surface and the recording format, so
/// matches over it stay exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputSurface {
    /// The window of the presentation that owns the binding.
    Window,
    /// A surface embedded in that window, minted by
    /// [`GestureBinding::embed_surface`](crate::GestureBinding::embed_surface).
    Embedded(EmbeddedSurfaceId),
}

/// Identity of a surface embedded in a presentation's window.
///
/// Minted only by [`GestureBinding::embed_surface`](crate::GestureBinding::embed_surface),
/// never reused within that binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EmbeddedSurfaceId(NonZeroU64);

impl EmbeddedSurfaceId {
    #[inline]
    pub(crate) const fn new(id: NonZeroU64) -> Self {
        Self(id)
    }

    /// The raw value, for diagnostics and recording rows; there is no
    /// conversion back.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// A binding minted every embedded-surface identity it can represent; it
/// refuses permanently instead of reissuing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("embedded input surface identities are exhausted")]
pub struct SurfaceIdsExhausted;

// ============================================================================
// RegionId - Identifier for mouse regions
// ============================================================================

/// Unique identifier for a mouse-sensitive region.
///
/// Re-exported from `flui_foundation::RenderId` since regions correspond to
/// render objects (hit-testable visual elements).
pub use flui_foundation::RenderId as RegionId;
