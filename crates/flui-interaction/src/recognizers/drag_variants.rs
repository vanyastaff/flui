//! Pre-built drag recognisers with fixed axis.
//!
//! Flutter parity: `gestures/monodrag.dart` exposes
//! `VerticalDragGestureRecognizer`, `HorizontalDragGestureRecognizer`, and
//! `PanGestureRecognizer` as thin subclasses of `DragGestureRecognizer` that
//! hard-code the axis. We expose the same three as type aliases over
//! [`DragGestureRecognizer`] so a recogniser's axis is statically fixed at
//! the type level (no runtime axis mismatch, no extra newtype overhead).
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
//! // Free fn — axis fixed at Free via the type.
//! let recognizer: PanGestureRecognizer = pan(arena);
//! // Standard builders on the underlying recogniser remain reachable.
//! let _ = recognizer.clone()
//!     .with_on_start(|d| { let _ = d; });
//! ```

use std::sync::Arc;

use crate::arena::GestureArena;

use super::drag::{
    DragAxis, DragEndCallback, DragGestureRecognizer, DragStartCallback, DragUpdateCallback,
};

/// A drag recogniser constrained to the vertical axis.
///
/// Mirrors Flutter's `VerticalDragGestureRecognizer` (which itself extends
/// `DragGestureRecognizer`). Internally a [`DragGestureRecognizer`] with
/// [`DragAxis::Vertical`]; the axis is fixed at the type level for
/// clarity at the call site.
pub type VerticalDragGestureRecognizer = DragGestureRecognizer;

/// A drag recogniser constrained to the horizontal axis.
///
/// Mirrors Flutter's `HorizontalDragGestureRecognizer`.
pub type HorizontalDragGestureRecognizer = DragGestureRecognizer;

/// A free-direction pan recogniser.
///
/// Mirrors Flutter's `PanGestureRecognizer`. A pan is a drag that can
/// move in any direction — the default axis is [`DragAxis::Free`].
pub type PanGestureRecognizer = DragGestureRecognizer;

// ============================================================================
// Free fn constructors (Flutter parity for per-axis subclass constructors)
// ============================================================================
//
// Inherent `new`/`with_settings` methods on a type alias collide with the
// underlying type's identical-shaped methods, so we use free fns here. The
// axis becomes implicit in the recogniser type — matching Flutter's
// per-axis subclasses.

/// Construct a vertical-only drag recogniser.
///
/// Equivalent to `DragGestureRecognizer::new(arena, DragAxis::Vertical)`
/// but reads more naturally at the call site.
#[must_use]
pub fn vertical_drag(arena: GestureArena) -> Arc<VerticalDragGestureRecognizer> {
    DragGestureRecognizer::new(arena, DragAxis::Vertical)
}

/// Construct a vertical-only drag recogniser with custom settings.
#[must_use]
pub fn vertical_drag_with_settings(
    arena: GestureArena,
    settings: crate::settings::GestureSettings,
) -> Arc<VerticalDragGestureRecognizer> {
    DragGestureRecognizer::with_settings(arena, DragAxis::Vertical, settings)
}

/// Construct a horizontal-only drag recogniser.
#[must_use]
pub fn horizontal_drag(arena: GestureArena) -> Arc<HorizontalDragGestureRecognizer> {
    DragGestureRecognizer::new(arena, DragAxis::Horizontal)
}

/// Construct a horizontal-only drag recogniser with custom settings.
#[must_use]
pub fn horizontal_drag_with_settings(
    arena: GestureArena,
    settings: crate::settings::GestureSettings,
) -> Arc<HorizontalDragGestureRecognizer> {
    DragGestureRecognizer::with_settings(arena, DragAxis::Horizontal, settings)
}

/// Construct a free-direction pan recogniser.
#[must_use]
pub fn pan(arena: GestureArena) -> Arc<PanGestureRecognizer> {
    DragGestureRecognizer::new(arena, DragAxis::Free)
}

/// Construct a free-direction pan recogniser with custom settings.
#[must_use]
pub fn pan_with_settings(
    arena: GestureArena,
    settings: crate::settings::GestureSettings,
) -> Arc<PanGestureRecognizer> {
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
    pub fn on_start(self: Arc<Self>, cb: DragStartCallback) -> Arc<Self> {
        // The aliased method already returns Arc<Self>; the closure is
        // forwarded as-is.
        self.with_on_start(move |d| cb(d))
    }

    /// Convenience builder equivalent to
    /// [`DragGestureRecognizer::with_on_update`].
    pub fn on_update(self: Arc<Self>, cb: DragUpdateCallback) -> Arc<Self> {
        self.with_on_update(move |d| cb(d))
    }

    /// Convenience builder equivalent to
    /// [`DragGestureRecognizer::with_on_end`].
    pub fn on_end(self: Arc<Self>, cb: DragEndCallback) -> Arc<Self> {
        self.with_on_end(move |d| cb(d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::GestureSettings;

    #[test]
    fn vertical_constructor_pins_axis() {
        let arena = crate::arena::GestureArena::new();
        let rec = vertical_drag(arena);
        assert_eq!(rec.axis(), DragAxis::Vertical);
    }

    #[test]
    fn horizontal_constructor_pins_axis() {
        let arena = crate::arena::GestureArena::new();
        let rec = horizontal_drag(arena);
        assert_eq!(rec.axis(), DragAxis::Horizontal);
    }

    #[test]
    fn pan_constructor_pins_axis() {
        let arena = crate::arena::GestureArena::new();
        let rec = pan(arena);
        assert_eq!(rec.axis(), DragAxis::Free);
    }

    #[test]
    fn with_settings_propagates_per_axis_slop() {
        // Custom per-axis slop must survive the constructor.
        let arena = crate::arena::GestureArena::new();
        let settings = GestureSettings::touch_defaults()
            .with_pan_slop_vertical(5.0)
            .with_pan_slop_horizontal(7.0);
        let rec = vertical_drag_with_settings(arena, settings);
        assert_eq!(rec.settings().pan_slop_vertical(), 5.0);
        assert_eq!(rec.settings().pan_slop_horizontal(), 7.0);
    }
}
