//! `RenderIgnorePointer` — single-child proxy that, when active, makes
//! its entire subtree invisible to pointer events. Pointers pass
//! through to whatever is painted *behind* the stack.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderIgnorePointer`](https://api.flutter.dev/flutter/rendering/RenderIgnorePointer-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`).
//!
//! # Rust-native improvements
//!
//! * `ignoring` is a typed `bool` boundary; setter returns
//!   `bool` change-flag for pipeline `mark_needs_paint` /
//!   `mark_needs_layout` short-circuit.
//! * Layout / paint are pure pass-through — only `hit_test` differs
//!   from a transparent proxy. The semantic difference between
//!   `RenderIgnorePointer` and [`crate::RenderAbsorbPointer`]
//!   ("ignore = pointers pass through" vs "absorb = pointer caught
//!   here, nothing below sees it") lives entirely in `hit_test`.

use flui_foundation::Single;
use flui_foundation::geometry::Offset;

use flui_rendering::{context::BoxHitTestContext, parent_data::BoxParentData, traits::RenderBox};

/// A render object that, when `ignoring` is true, returns `false` from
/// hit testing — making the subtree (and itself) invisible to pointer
/// events, so the gesture system sees through to siblings below.
///
/// Layout and paint pass through transparently in all cases.
#[derive(Debug, Clone)]
pub struct RenderIgnorePointer {
    ignoring: bool,
    has_child: bool,
}

impl RenderIgnorePointer {
    /// Creates an ignore-pointer render object with the given flag.
    pub const fn new(ignoring: bool) -> Self {
        Self {
            ignoring,
            has_child: false,
        }
    }

    /// Returns whether pointer events are currently ignored.
    #[inline]
    pub fn ignoring(&self) -> bool {
        self.ignoring
    }

    /// Updates the ignoring flag and reports semantics when changed.
    pub fn set_ignoring(&mut self, ignoring: bool) -> flui_rendering::RenderUpdateImpact {
        if self.ignoring == ignoring {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.ignoring = ignoring;
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }
}

impl Default for RenderIgnorePointer {
    /// Defaults to `ignoring = true` (Flutter parity).
    fn default() -> Self {
        Self::new(true)
    }
}

impl flui_foundation::Diagnosticable for RenderIgnorePointer {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_flag("ignoring", self.ignoring, "ignoring");
    }
}

impl RenderBox for RenderIgnorePointer {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    // paint: default pass-through (splices the child in order).

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if self.ignoring {
            // Pointer events pass straight through to siblings below.
            return false;
        }
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================
