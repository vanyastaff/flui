//! [`RenderLayoutBuilder`] — the render half of the build-during-layout seam.
//!
//! # What it does
//!
//! A single-child box that, on every layout pass:
//!
//! 1. **publishes** the real incoming [`BoxConstraints`] into its shared
//!    [`LayoutConstraintsCell`],
//! 2. lays its child out under **those same constraints** and reads the child's
//!    size back (Flutter's `child.layout(constraints, parentUsesSize: true)`),
//! 3. sizes itself to `constraints.constrain(child_size)` — or, with no child,
//!    to `constraints.biggest()`.
//!
//! It performs **no build**. Publishing is the whole point: the cell's
//! edge-triggered `needs_build` flag is read *between* layout passes by
//! `BuildOwner::service_layout_builders`, which rebuilds the element and
//! re-dirties this node. FLUI cannot build mid-walk the way
//! Flutter's `invokeLayoutCallback` does — the constraints must be published
//! first, then the element rebuilt before this node's layout completes.
//!
//! # Not the public widget
//!
//! This is the render half. The public widget lives in `flui-view` and is
//! re-exported by `flui-widgets`; it registers its element
//! against this object's cell. App code should construct `LayoutBuilder`, not
//! this render object directly.
//!
//! # Verified against the reference
//!
//! Cross-checked against `.flutter/packages/flutter/lib/src/widgets/layout_builder.dart`
//! (`_RenderLayoutBuilder`) and `.flutter/packages/flutter/lib/src/rendering/box.dart`
//! (`debugCannotComputeDryLayout`), Flutter master `3.33.0-0.0.pre-6280-g88e87cd963f`:
//!
//! - **`performLayout`** — `child.layout(constraints, parentUsesSize: true)`;
//!   `size = constraints.constrain(child.size)`; `size = constraints.biggest`
//!   when there is no child. Matched exactly.
//! - **Intrinsics** — Flutter's `computeMin/MaxIntrinsicWidth/Height` all
//!   `assert(_debugThrowIfNotCheckingIntrinsics())` then `return 0.0`; the assert
//!   throws *"LayoutBuilder does not support returning intrinsic dimensions"*
//!   outside `RenderObject.debugCheckingIntrinsics`. FLUI returns the same
//!   `0.0`. **Documented divergence:** FLUI logs via `tracing::error!` instead of
//!   panicking — an intrinsic query returns `f32` with no error channel, and
//!   `docs/PANIC-POLICY.md` reserves panics for internal invariants, not caller
//!   misuse. FLUI also has no `debugCheckingIntrinsics` flag to distinguish
//!   Flutter's own intrinsic-checking probe from real use.
//! - **Dry layout** — Flutter's `computeDryLayout` asserts
//!   `debugCannotComputeDryLayout(reason: 'Calculating the dry layout would
//!   require running the layout callback speculatively, which might mutate the
//!   live render object tree.')` and returns `Size.zero`; `computeDryBaseline`
//!   likewise returns `null`. FLUI returns `Size::ZERO` for the same reason,
//!   with the same `tracing::error!` divergence. It must NOT answer from the
//!   currently-built child: that child was built for *different* constraints, so
//!   the answer would be confidently wrong.

use std::sync::Arc;

use flui_tree::Single;
use flui_types::Size;

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::RenderBox,
};

use super::layout_constraints_cell::LayoutConstraintsCell;

/// Single-child box that publishes its incoming constraints for the element
/// layer to build against.
///
/// Holds the `Arc<LayoutConstraintsCell>` its element registered under the same
/// `RenderId` (`BuildOwner::layout_builder_registry`). The cell is the only
/// channel between them; this object never touches the element tree.
#[derive(Debug, Clone)]
pub struct RenderLayoutBuilder {
    /// Shared with the element registered under this node's `RenderId`.
    cell: Arc<LayoutConstraintsCell>,
    /// Tracked in `perform_layout` because `BoxHitTestContext` exposes no
    /// child count — the same reason `RenderOpacity` carries this flag.
    has_child: bool,
}

impl RenderLayoutBuilder {
    /// Creates a render object publishing into `cell`.
    ///
    /// The caller (the element owning this render object) must hold the same `Arc` and register
    /// it against this node's `RenderId`, or nothing will ever read what this
    /// object publishes.
    #[must_use]
    pub fn new(cell: Arc<LayoutConstraintsCell>) -> Self {
        Self {
            cell,
            has_child: false,
        }
    }

    /// The cell this object publishes into.
    ///
    /// Exposed so the element can assert it registered the same `Arc`, and so
    /// harness tests can observe what a layout pass published.
    #[must_use]
    pub fn cell(&self) -> &Arc<LayoutConstraintsCell> {
        &self.cell
    }
}

/// Flutter throws `'LayoutBuilder does not support returning intrinsic dimensions'`
/// here; FLUI has no error channel on an `f32`-returning intrinsic, so it logs.
#[cold]
fn report_unsupported_intrinsics() {
    tracing::error!(
        "RenderLayoutBuilder does not support intrinsic dimensions: computing them would \
         require running the layout callback speculatively for a hypothetical constraint. \
         Returning 0.0 (Flutter throws here)."
    );
}

impl flui_foundation::Diagnosticable for RenderLayoutBuilder {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_optional(
            "published_constraints",
            self.cell.constraints().map(|c| format!("{c:?}")),
        );
        builder.add("needs_build", self.cell.needs_build());
    }
}

impl RenderBox for RenderLayoutBuilder {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();

        // Publish BEFORE laying the child out. `publish` raises `needs_build`
        // only when these constraints differ from the last committed ones, so a
        // pass that changes nothing leaves the cell clean and the binding's
        // layout<->build fixpoint terminates.
        self.cell.publish(constraints);

        self.has_child = ctx.child_count() > 0;

        if self.has_child {
            // Flutter: `child.layout(constraints, parentUsesSize: true)` — the
            // child gets our constraints verbatim (NOT loosened), and we read
            // its size back, which is what makes us a non-boundary that resizes
            // with its child.
            let child_size = ctx.layout_single_child();
            ctx.position_single_child_at_origin();
            constraints.constrain(child_size)
        } else {
            // No child yet — the first pass of a freshly mounted builder, before
            // `service_layout_builders` has run the builder even once.
            constraints.biggest()
        }
    }

    /// Dry layout is **not supported**, matching Flutter's `_RenderLayoutBuilder`.
    ///
    /// Answering would require running the builder speculatively (the child that
    /// exists right now was built for *different* constraints). Flutter asserts
    /// and returns `Size.zero`; FLUI logs and returns `Size::ZERO`.
    ///
    /// Deliberately does NOT publish: a dry probe is a hypothetical, and
    /// dirtying the cell from one would rebuild the element against constraints
    /// the node was never laid out with.
    fn compute_dry_layout(
        &self,
        _constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        tracing::error!(
            "RenderLayoutBuilder does not support dry layout: computing it would require \
             running the layout callback speculatively, which would mutate the live element \
             and render trees. Returning Size::ZERO (Flutter throws here)."
        );
        Size::ZERO
    }

    // ── Intrinsics: unsupported, exactly as Flutter's `_RenderLayoutBuilder` ──
    //
    // The child depends on the incoming constraints, so an intrinsic extent
    // cannot be answered without building speculatively. Flutter throws in debug
    // and returns 0.0; FLUI logs and returns 0.0 (see the module docs).

    fn compute_min_intrinsic_width(&self, _height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        report_unsupported_intrinsics();
        0.0
    }

    fn compute_max_intrinsic_width(&self, _height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        report_unsupported_intrinsics();
        0.0
    }

    fn compute_min_intrinsic_height(&self, _width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        report_unsupported_intrinsics();
        0.0
    }

    fn compute_max_intrinsic_height(&self, _width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        report_unsupported_intrinsics();
        0.0
    }

    // paint() uses the default: a layout builder draws nothing of its own.

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_layout_offset(0)
        } else {
            false
        }
    }
}
