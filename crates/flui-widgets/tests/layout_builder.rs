//! Public-API tests for [`LayoutBuilder`].
//!
//! These drive the widget through the real `flui-widgets` surface and a real
//! `HeadlessBinding` frame — the same path `UiRuntime::draw_frame` takes. The
//! `flui-view` unit tests cover the seam's internals; this file covers what an
//! app author can actually observe.
//!
//! # Scenarios
//!
//! Expected values are fixed by the documented contract, not by running the
//! code first: the builder sees the parent's size, a zero-area layout does not
//! crash, and a size change does not require a rebuild.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::common::{lay_out, loose};
use flui_foundation::geometry::Size;
use flui_rendering::constraints::BoxConstraints;
use parking_lot::Mutex;

// Exercise the public prelude import path: if `LayoutBuilder` were not exported
// from `flui_widgets::prelude`, this file would not compile.
use flui_view::{BoxedView, ViewExt};
use flui_widgets::prelude::*;
use flui_widgets::{
    ConstrainedBox, CustomMultiChildLayout, LayoutId, MultiChildLayoutContext,
    MultiChildLayoutDelegate, Padding, SizedBox,
};

/// A builder that records the constraints it was handed.
fn recorder(
    log: Arc<Mutex<Vec<BoxConstraints>>>,
) -> impl Fn(&dyn BuildContext, BoxConstraints) -> SizedBox + Send + Sync + 'static {
    move |_ctx, constraints| {
        log.lock().push(constraints);
        SizedBox::new(constraints.max_width / 2.0, constraints.max_height / 2.0)
    }
}

/// Changing the constraints re-invokes the builder and relays the child out —
/// in the frame the change lands, not the one after.
///
/// Loose constraints throughout, so the child's size (`biggest / 2`) actually
/// discriminates: under tight constraints the child would be stretched to fill
/// and the assertion would hold for the wrong reason.
pub(crate) fn layout_builder_constraint_change_rebuilds_in_the_same_frame() {
    let log = Arc::new(Mutex::new(Vec::new()));

    let bounds = |w: f64, h: f64| BoxConstraints::new(0.0, w, 0.0, h);

    let mut laid = lay_out(
        ConstrainedBox::new(bounds(200.0, 100.0))
            .child(LayoutBuilder::new(recorder(Arc::clone(&log)))),
        loose(400.0),
    );
    assert_eq!(log.lock().len(), 1);
    assert_eq!(
        laid.size(laid.only_child(laid.root())),
        Size::new(100.0, 50.0),
        "first frame: constrain(biggest/2)"
    );

    // A new root imposes different constraints on the same LayoutBuilder.
    laid.pump_widget(
        ConstrainedBox::new(bounds(80.0, 60.0))
            .child(LayoutBuilder::new(recorder(Arc::clone(&log)))),
    );

    let seen = log.lock().clone();
    assert_eq!(
        seen.last().copied(),
        Some(bounds(80.0, 60.0)),
        "the builder must re-run under the NEW constraints"
    );
    // Documented behavior (ADR-0017): `pump_widget` both rebuilds the widget
    // and changes the constraints, so FLUI invokes the builder twice this frame —
    // once in the leading `build_scope` with the last-published constraints, once
    // in the layout<->build fixpoint with the fresh ones. A design that defers
    // all building to layout would invoke it once. Both
    // paint the same final child; the builder must be a pure function of its
    // inputs. Pinned here so a change to that behavior is deliberate.
    assert_eq!(
        seen.len(),
        2,
        "one mount build plus one rebuild with fresh constraints"
    );

    let builder_node = laid.only_child(laid.current_root());
    assert_eq!(
        laid.size(builder_node),
        Size::new(40.0, 30.0),
        "the builder node follows its rebuilt child, in the same frame"
    );
    assert_eq!(
        laid.size(laid.only_child(builder_node)),
        Size::new(40.0, 30.0),
        "the rebuilt child (biggest/2) must be laid out in the same frame"
    );
}

/// `depth` nested `LayoutBuilder`s around a 30×20 leaf; every builder run
/// counts into `runs`. Each level builds a fresh view, so a rebuilt level
/// hands its inner builder a new one.
fn nested(depth: usize, runs: Rc<Cell<usize>>) -> BoxedView {
    if depth == 0 {
        return SizedBox::new(30.0, 20.0).boxed();
    }
    LayoutBuilder::new(move |_ctx, _constraints| {
        runs.set(runs.get() + 1);
        nested(depth - 1, Rc::clone(&runs))
    })
    .boxed()
}

/// The view `make` builds, wrapped in `nesting` pass-through `LayoutBuilder`s.
pub(crate) fn inside_layout_builders(nesting: usize, make: Rc<dyn Fn() -> BoxedView>) -> BoxedView {
    if nesting == 0 {
        return make();
    }
    LayoutBuilder::new(move |_ctx, _constraints| {
        inside_layout_builders(nesting - 1, Rc::clone(&make))
    })
    .boxed()
}

/// Nesting costs the fixpoint one pass per level, and none of those passes
/// counts against its bound (ADR-0308): sixteen nested builders settle on
/// their first frame, and again in the frame a rebuild above them hands each
/// level a new view, a level a pass.
pub(crate) fn nested_layout_builders_settle_on_their_first_frame() {
    const DEPTH: usize = 16;
    let runs = Rc::new(Cell::new(0));
    let mut laid = lay_out(nested(DEPTH, Rc::clone(&runs)), loose(400.0));
    assert_eq!(
        runs.get(),
        DEPTH,
        "each builder ran once, under stable constraints"
    );
    let leaf = (0..DEPTH).fold(laid.root(), |node, _| laid.only_child(node));
    assert_eq!(
        laid.size(leaf),
        Size::new(30.0, 20.0),
        "the innermost leaf is laid out"
    );
    assert_eq!(laid.size(laid.root()), Size::new(30.0, 20.0));

    // The root's new view rebuilds the outer level in the frame's build; each
    // inner level then waits for its own scope's drain in the next pass.
    laid.pump_widget(nested(DEPTH, Rc::clone(&runs)));
    assert_eq!(
        runs.get(),
        2 * DEPTH,
        "every level rebuilt once more, in the same frame"
    );
    let leaf = (0..DEPTH).fold(laid.current_root(), |node, _| laid.only_child(node));
    assert_eq!(laid.size(leaf), Size::new(30.0, 20.0));
}

/// A resize settles nested builders whose output depends on their
/// constraints one level a pass: each level pads each side by an eighth of
/// its width, so a level's constraints change again in every pass until the
/// level above it has settled, by an amount far above rounding at every
/// depth. Those repeats are explained by the level above rebuilding in the
/// pass before, and are not charged (ADR-0308): fourteen levels take more
/// passes than the bound on charged ones.
pub(crate) fn nested_constraint_dependent_layout_builders_settle_after_a_resize() {
    const DEPTH: usize = 14;
    fn level(i: usize, widths: Rc<RefCell<Vec<f64>>>) -> BoxedView {
        if i == DEPTH {
            return SizedBox::new(10.0, 10.0).boxed();
        }
        LayoutBuilder::new(move |_ctx, constraints: BoxConstraints| {
            widths.borrow_mut()[i] = constraints.max_width;
            Padding::symmetric(constraints.max_width / 8.0, 0.0)
                .child(level(i + 1, Rc::clone(&widths)))
        })
        .boxed()
    }
    let widths = Rc::new(RefCell::new(vec![f64::NAN; DEPTH]));
    // One chain view throughout: the resize, not a new view, drives it.
    let chain = level(0, Rc::clone(&widths));
    let bounds = |w: f64| BoxConstraints::new(0.0, w, 0.0, 100.0);
    let mut laid = lay_out(
        ConstrainedBox::new(bounds(300.0)).child(chain.clone()),
        loose(400.0),
    );
    laid.pump_widget(ConstrainedBox::new(bounds(400.0)).child(chain));

    let mut expected = 400.0_f64;
    let seen = widths.borrow().clone();
    for (i, width) in seen.into_iter().enumerate() {
        assert!(
            (width - expected).abs() < 1e-9,
            "level {i} last built for max width {width}, expected {expected}"
        );
        expected -= 2.0 * (expected / 8.0);
    }
}

/// A builder whose output changes its own constraints never settles. The
/// frame stops after a bounded number of builds, logs one error and paints
/// the last laid-out tree, in a debug build too: it is application content,
/// not a FLUI invariant (ADR-0308, `docs/PANIC-POLICY.md`).
pub(crate) fn a_layout_builder_feeding_its_own_constraints_paints_without_panicking() {
    /// Hands its one child loose constraints whose width alternates on
    /// every layout, so each rebuild of the child (which relays out this
    /// parent, the child being no relayout boundary) changes them again.
    #[derive(Debug, Default)]
    struct Alternating {
        layouts: Cell<u32>,
    }
    impl MultiChildLayoutDelegate for Alternating {
        fn perform_layout(
            &self,
            context: &mut dyn MultiChildLayoutContext,
            _parent_size: Size,
        ) -> flui_rendering::RenderResult<()> {
            let layouts = self.layouts.get();
            self.layouts.set(layouts.wrapping_add(1));
            let width = if layouts.is_multiple_of(2) {
                100.0
            } else {
                200.0
            };
            context.layout_child("builder", BoxConstraints::new(0.0, width, 0.0, 50.0))?;
            context.position_child("builder", flui_foundation::geometry::Offset::ZERO);
            Ok(())
        }

        fn should_relayout(&self, _old: &dyn MultiChildLayoutDelegate) -> bool {
            false
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    let runs = Rc::new(Cell::new(0_usize));
    let builds = Rc::clone(&runs);
    let laid = lay_out(
        CustomMultiChildLayout::new(
            Alternating::default(),
            (LayoutId::new(
                "builder",
                LayoutBuilder::new(move |_ctx, constraints: BoxConstraints| {
                    builds.set(builds.get() + 1);
                    SizedBox::new(constraints.max_width, 20.0)
                }),
            ),),
        ),
        loose(400.0),
    );
    // One first build, then one charged rebuild per pass up to the bound of
    // ten charged passes.
    assert!(
        (2..=11).contains(&runs.get()),
        "the builder ran {} times: it must keep rebuilding, and stop at the bound",
        runs.get()
    );
    let builder = laid.only_child(laid.root());
    let child = laid.only_child(builder);
    assert!(
        [100.0, 200.0].contains(&laid.size(child).width),
        "the last laid-out tree is painted: {:?}",
        laid.size(child)
    );
}
