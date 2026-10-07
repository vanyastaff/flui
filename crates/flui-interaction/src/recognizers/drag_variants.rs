//! Pre-built drag recognisers with fixed axis.
//!
//! `VerticalDragGestureRecognizer`, `HorizontalDragGestureRecognizer`, and
//! `PanGestureRecognizer` are type aliases over
//! [`DragGestureRecognizer`] for call-site readability, paired with the
//! [`vertical_drag`] / [`horizontal_drag`] / [`pan`] constructors that set the
//! axis. Note: aliases are the *same* type — they do NOT enforce the axis at
//! compile time; the axis is a runtime field set by the constructor.
//!
//! Because a type alias shares the underlying type's methods, the
//! constructors and per-axis builders all live on
//! [`DragGestureRecognizer`] itself; this module only adds the alias-flavoured
//! *fluent* builders ([`PanGestureRecognizer::on_start`] /
//! [`PanGestureRecognizer::on_update`] / [`PanGestureRecognizer::on_end`])
//! and free constructor helpers ([`vertical_drag`], [`horizontal_drag`],
//! [`pan`]) that hide the axis argument at the call site.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::recognizers::drag_variants::{pan, PanGestureRecognizer};
//! use flui_interaction::recognizers::drag::DragGestureRecognizer;
//!
//! let arena = GestureArena::new();
//! // Free fn — sets the Free axis at construction.
//! let recognizer: PanGestureRecognizer = pan(arena);
//! // Standard builders on the underlying recogniser remain reachable.
//! let _ = recognizer.clone()
//!     .with_on_start(|d| { let _ = d; });
//! ```

use std::rc::Rc;

use crate::arena::GestureArena;
use crate::traits::DragAxis;

use super::drag::{DragEndCallback, DragGestureRecognizer, DragStartCallback, DragUpdateCallback};

/// A drag recogniser constrained to the vertical axis.
///
/// Construct via [`vertical_drag`] to bind
/// [`DragAxis::Vertical`]; the alias is for call-site readability, not
/// compile-time axis enforcement.
pub type VerticalDragGestureRecognizer = DragGestureRecognizer;

/// A drag recogniser constrained to the horizontal axis.
///
/// Construct via [`horizontal_drag`] to bind [`DragAxis::Horizontal`].
pub type HorizontalDragGestureRecognizer = DragGestureRecognizer;

/// A free-direction pan recogniser.
///
/// A pan is a drag that can
/// move in any direction — the default axis is [`DragAxis::Free`].
pub type PanGestureRecognizer = DragGestureRecognizer;

// ============================================================================
// Free fn constructors (one per axis)
// ============================================================================
//
// Inherent `new`/`with_settings` methods on a type alias collide with the
// underlying type's identical-shaped methods, so we use free fns here. The
// axis becomes implicit at the call site.

/// Construct a vertical-only drag recogniser.
///
/// Equivalent to `DragGestureRecognizer::new(arena, DragAxis::Vertical)`
/// but reads more naturally at the call site.
#[must_use]
pub fn vertical_drag(arena: GestureArena) -> Rc<VerticalDragGestureRecognizer> {
    DragGestureRecognizer::new(arena, DragAxis::Vertical)
}

/// Construct a vertical-only drag recogniser with custom settings.
#[must_use]
pub fn vertical_drag_with_settings(
    arena: GestureArena,
    settings: crate::settings::GestureSettings,
) -> Rc<VerticalDragGestureRecognizer> {
    DragGestureRecognizer::with_settings(arena, DragAxis::Vertical, settings)
}

/// Construct a horizontal-only drag recogniser.
#[must_use]
pub fn horizontal_drag(arena: GestureArena) -> Rc<HorizontalDragGestureRecognizer> {
    DragGestureRecognizer::new(arena, DragAxis::Horizontal)
}

/// Construct a horizontal-only drag recogniser with custom settings.
#[must_use]
pub fn horizontal_drag_with_settings(
    arena: GestureArena,
    settings: crate::settings::GestureSettings,
) -> Rc<HorizontalDragGestureRecognizer> {
    DragGestureRecognizer::with_settings(arena, DragAxis::Horizontal, settings)
}

/// Construct a free-direction pan recogniser.
#[must_use]
pub fn pan(arena: GestureArena) -> Rc<PanGestureRecognizer> {
    DragGestureRecognizer::new(arena, DragAxis::Free)
}

/// Construct a free-direction pan recogniser with custom settings.
#[must_use]
pub fn pan_with_settings(
    arena: GestureArena,
    settings: crate::settings::GestureSettings,
) -> Rc<PanGestureRecognizer> {
    DragGestureRecognizer::with_settings(arena, DragAxis::Free, settings)
}

// ============================================================================
// Pan-only fluent builders
// ============================================================================
//
// `on_start` / `on_update` / `on_end` are inherently tied to a recogniser
// type, so pan-flavoured fluent builders live here. Vertical / Horizontal
// recognisers can still use the standard `with_on_*` chain — the type alias
// exposes those methods unchanged.

impl PanGestureRecognizer {
    /// Convenience builder equivalent to
    /// [`DragGestureRecognizer::with_on_start`] but returning the alias
    /// type for fluent chaining.
    pub fn on_start(self: Rc<Self>, cb: DragStartCallback) -> Rc<Self> {
        // The aliased method already returns Arc<Self>; the closure is
        // forwarded as-is.
        self.with_on_start(move |d| cb(d))
    }

    /// Convenience builder equivalent to
    /// [`DragGestureRecognizer::with_on_update`].
    pub fn on_update(self: Rc<Self>, cb: DragUpdateCallback) -> Rc<Self> {
        self.with_on_update(move |d| cb(d))
    }

    /// Convenience builder equivalent to
    /// [`DragGestureRecognizer::with_on_end`].
    pub fn on_end(self: Rc<Self>, cb: DragEndCallback) -> Rc<Self> {
        self.with_on_end(move |d| cb(d))
    }
}
