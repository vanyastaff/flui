//! A frame that unwinds, out of a `tracing` subscriber, a visual-update wake or a render
//! object's hook, leaves the pipeline its owner (ADR-0048): the owner stays in its
//! `PipelineCell` with the work the frame had not finished, the retry at unchanged constraints
//! paints, and invalidation handles keep reaching it.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Size};
use flui_objects::{RenderColoredBox, RenderRepaintBoundary, RenderStack};
use flui_rendering::{
    TextContextHandle,
    constraints::BoxConstraints,
    error::RenderError,
    pipeline::{PipelineCell, PipelineOwner},
    protocol::{BoxProtocol, ProtocolGeometry},
    traits::{HitTestOutcome, RenderObject},
};

/// What makes [`PanicsOnce`] panic.
#[derive(Clone, Copy)]
enum Trigger {
    /// A span with this name opens.
    SpanOpens(&'static str),
    /// An event at this level comes from this module.
    Event {
        target: &'static str,
        level: tracing::Level,
    },
}

/// A subscriber that panics the first time its trigger fires, and never again: a second panic
/// while the first unwinds would abort.
struct PanicsOnce {
    trigger: Trigger,
    armed: AtomicBool,
}

impl PanicsOnce {
    fn fires(&self) -> bool {
        self.armed.swap(false, Ordering::SeqCst)
    }
}

impl tracing::Subscriber for PanicsOnce {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        if let Trigger::SpanOpens(name) = self.trigger
            && attributes.metadata().name() == name
            && self.fires()
        {
            panic!("a subscriber that cannot open {name}");
        }
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if let Trigger::Event { target, level } = self.trigger
            && event.metadata().target() == target
            && *event.metadata().level() == level
            && self.fires()
        {
            panic!("a subscriber that cannot report from {target}");
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// [`Rigged::always_needs_compositing`] answers `true`.
const COMPOSITED: u8 = 0;
/// [`Rigged::always_needs_compositing`] panics.
const PANICS: u8 = 1;
/// [`Rigged::always_needs_compositing`] answers `false`.
const PLAIN: u8 = 2;

/// A 40×40 leaf whose layout fails and whose compositing hook panics on demand.
#[derive(Debug)]
struct Rigged {
    fails_layout: Arc<AtomicBool>,
    compositing: Arc<AtomicU8>,
}

impl flui_foundation::inspect::Inspect for Rigged {
    fn kind(&self) -> &'static str {
        "Rigged"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl RenderObject<BoxProtocol> for Rigged {
    fn perform_layout_raw(
        &mut self,
        _ctx: &mut <BoxProtocol as flui_rendering::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> flui_rendering::error::RenderResult<ProtocolGeometry<BoxProtocol>> {
        if self.fails_layout.load(Ordering::SeqCst) {
            return Err(RenderError::contract_violation(
                "Rigged",
                "layout fails while armed",
            ));
        }
        Ok(Size::new(40.0, 40.0))
    }

    fn always_needs_compositing(&self) -> bool {
        match self.compositing.load(Ordering::SeqCst) {
            COMPOSITED => true,
            PANICS => panic!("a compositing hook that panics"),
            _ => false,
        }
    }

    fn paint_raw(
        &self,
        _recorder: &mut flui_rendering::context::FragmentRecorder,
        _child_count: usize,
        _size: Size,
    ) {
    }

    fn hit_test_raw(
        &self,
        _position: flui_rendering::protocol::ProtocolPosition<BoxProtocol>,
        _child_count: usize,
        _size: Size,
        _hit_child: &mut dyn FnMut(
            usize,
            Option<flui_rendering::protocol::ProtocolPosition<BoxProtocol>>,
            Option<Matrix4>,
        ) -> bool,
    ) -> HitTestOutcome {
        HitTestOutcome::miss()
    }
}

/// `RenderStack → [RenderRepaintBoundary → RenderColoredBox, Rigged]`, painted once at 100×100,
/// then asked for 150×150: the next frame owes layout and paint.
struct Fixture {
    cell: PipelineCell,
    text: TextContextHandle,
    root: RenderId,
    boundary: RenderId,
    rigged: RenderId,
    fails_layout: Arc<AtomicBool>,
    compositing: Arc<AtomicU8>,
}

fn fixture() -> Fixture {
    let text = TextContextHandle::standalone();
    let mut owner = PipelineOwner::new(text.clone());
    let root = owner.set_root_render_object(Box::new(RenderStack::new()));
    let boundary = owner
        .insert_child_render_object(root, Box::new(RenderRepaintBoundary::new()))
        .expect("the boundary attaches");
    owner
        .insert_child_render_object(boundary, Box::new(RenderColoredBox::red(10.0, 10.0)))
        .expect("the boundary's child attaches");
    let fails_layout = Arc::new(AtomicBool::new(false));
    let compositing = Arc::new(AtomicU8::new(COMPOSITED));
    let rigged = owner
        .insert_child_render_object(
            root,
            Box::new(Rigged {
                fails_layout: Arc::clone(&fails_layout),
                compositing: Arc::clone(&compositing),
            }),
        )
        .expect("the rigged leaf attaches");
    owner.set_root_constraints(Some(tight(100.0)));
    let cell = PipelineCell::new(owner);
    assert!(
        matches!(cell.run_frame(), Ok(Some(_))),
        "the first frame paints"
    );
    cell.with_mut(|owner| owner.set_root_constraints(Some(tight(150.0))));
    Fixture {
        cell,
        text,
        root,
        boundary,
        rigged,
        fails_layout,
        compositing,
    }
}

fn tight(side: f64) -> BoxConstraints {
    BoxConstraints::tight(Size::new(side, side))
}

/// Runs `frame` under a subscriber that panics once on `trigger`, and expects that panic to
/// reach the caller.
fn unwinds_under<R>(trigger: Trigger, frame: impl FnOnce() -> R) {
    let subscriber = PanicsOnce {
        trigger,
        armed: AtomicBool::new(true),
    };
    // A second dispatcher keeps the callsites' interest from being cached against another
    // thread's empty default.
    let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let unwound = tracing::subscriber::with_default(subscriber, || {
        tracing::callsite::rebuild_interest_cache();
        catch_unwind(AssertUnwindSafe(frame))
    });
    tracing::callsite::rebuild_interest_cache();
    match unwound {
        Ok(_) => panic!("the subscriber's panic goes on to the caller"),
        Err(payload) => flui_foundation::panic::discard_payload(payload),
    }
}

/// The checks every row ends with: the owner in the cell is the pipeline's own, no phase is
/// left open, and the retry at the constraints the frame already had paints. The production
/// retry runs at unchanged constraints, so only the work the unwound frame kept brings the
/// tree to its new size.
fn the_pipeline_kept_its_owner(f: &Fixture) {
    assert!(f.cell.is_free(), "the cell is not left borrowed");
    f.cell.with(|owner| {
        assert_eq!(
            owner.root_id(),
            Some(f.root),
            "the cell holds the same tree"
        );
        assert!(
            TextContextHandle::ptr_eq(owner.text_context_for_test(), &f.text),
            "the cell's owner measures through the pipeline's context"
        );
        assert!(!owner.debug_doing_any_phase(), "no phase is left open");
    });

    assert!(
        matches!(f.cell.run_frame(), Ok(Some(_))),
        "the retry at unchanged constraints paints"
    );
    assert_eq!(
        f.cell.with(|owner| owner.box_size(f.root)),
        Some(Size::new(150.0, 150.0)),
        "the retry laid the tree out at the constraints it was given"
    );

    f.cell
        .with_mut(|owner| owner.set_root_constraints(Some(tight(200.0))));
    assert!(
        matches!(f.cell.run_frame(), Ok(Some(_))),
        "the next frame paints"
    );
    assert_eq!(
        f.cell.with(|owner| owner.box_size(f.root)),
        Some(Size::new(200.0, 200.0)),
        "the next frame lays out"
    );
}

/// The paint span opens before the paint phase does.
fn paint_span_opens() {
    let f = fixture();
    unwinds_under(Trigger::SpanOpens("paint"), || f.cell.run_frame());
    the_pipeline_kept_its_owner(&f);
}

/// The layer count report comes after the paint walk, with the paint phase open.
fn paint_report() {
    let f = fixture();
    unwinds_under(
        Trigger::Event {
            target: "flui_rendering::pipeline::owner::paint",
            level: tracing::Level::DEBUG,
        },
        || f.cell.run_frame(),
    );
    the_pipeline_kept_its_owner(&f);
}

/// The batch order report comes with the layout batch taken out of the scheduler.
fn layout_report() {
    let f = fixture();
    unwinds_under(
        Trigger::Event {
            target: "flui_rendering::pipeline::owner::layout",
            level: tracing::Level::DEBUG,
        },
        || f.cell.run_frame(),
    );
    the_pipeline_kept_its_owner(&f);
}

/// The layout fixpoint lays out through the cell, outside a whole frame.
fn layout_root_span_in_the_fixpoint() {
    let f = fixture();
    unwinds_under(Trigger::SpanOpens("layout_root"), || f.cell.run_layout());
    the_pipeline_kept_its_owner(&f);
}

/// The frame drains a handle's mark and wakes the presentation, whose callback panics.
fn wake_as_the_frame_drains() {
    let f = fixture();
    let armed = Arc::new(AtomicBool::new(false));
    let handle = f.cell.with_mut(|owner| {
        let wake_armed = Arc::clone(&armed);
        owner.set_on_need_visual_update(move || {
            assert!(
                !wake_armed.swap(false, Ordering::SeqCst),
                "a wake that panics"
            );
        });
        owner
            .render_invalidation_handle(f.boundary)
            .expect("the boundary is live")
    });
    // The send wakes too; the panic is armed for the drain's wake only.
    handle.mark_needs_paint().expect("the owner is live");
    armed.store(true, Ordering::SeqCst);
    let unwound = catch_unwind(AssertUnwindSafe(|| f.cell.run_frame()));
    match unwound {
        Ok(_) => panic!("the wake's panic goes on to the caller"),
        Err(payload) => flui_foundation::panic::discard_payload(payload),
    }
    assert!(
        handle.mark_needs_paint().is_ok(),
        "the handle still reaches the owner"
    );
    the_pipeline_kept_its_owner(&f);
}

/// The dirty root reports its failed child after the boundary beside it has laid out again:
/// the paint the boundary owes for its new layout must survive the panic.
fn layout_report_after_a_boundary_laid_out() {
    let f = fixture();
    f.fails_layout.store(true, Ordering::SeqCst);
    unwinds_under(
        Trigger::Event {
            target: "flui_rendering::pipeline::owner::subtree_arena",
            level: tracing::Level::DEBUG,
        },
        || f.cell.run_frame(),
    );
    f.fails_layout.store(false, Ordering::SeqCst);
    assert!(
        f.cell.with(|owner| owner
            .nodes_needing_paint()
            .iter()
            .any(|entry| entry.id == f.boundary)),
        "the boundary that laid out owes its paint"
    );
    the_pipeline_kept_its_owner(&f);
}

/// A compositing hook panics: the node keeps the compositing answer it had, so the retry still
/// sees the change and repaints for it.
fn compositing_hook() {
    let f = fixture();
    let composited = |f: &Fixture| {
        f.cell.with(|owner| {
            owner
                .render_tree()
                .get(f.rigged)
                .expect("the rigged leaf is live")
                .needs_compositing()
        })
    };
    // A newly attached leaf is composited once its bits are first recomputed.
    f.cell
        .with_mut(|owner| owner.mark_needs_compositing_bits_update(f.rigged));
    assert!(
        matches!(f.cell.run_frame(), Ok(Some(_))),
        "the frame that composites the rigged leaf paints"
    );
    assert!(composited(&f), "the rigged leaf is composited");
    f.compositing.store(PANICS, Ordering::SeqCst);
    f.cell
        .with_mut(|owner| owner.mark_needs_compositing_bits_update(f.rigged));
    let unwound = catch_unwind(AssertUnwindSafe(|| f.cell.run_frame()));
    match unwound {
        Ok(_) => panic!("the hook's panic goes on to the caller"),
        Err(payload) => flui_foundation::panic::discard_payload(payload),
    }
    f.compositing.store(PLAIN, Ordering::SeqCst);
    assert!(
        composited(&f),
        "the panic left the rigged leaf its old compositing answer"
    );
    the_pipeline_kept_its_owner(&f);
    assert!(!composited(&f), "the retry took the new compositing answer");
}

#[test]
fn a_frame_that_unwinds_leaves_the_pipeline_its_owner() {
    crate::run_table(&[
        ("a paint span that opens", paint_span_opens),
        ("a paint report", paint_report),
        ("a layout report", layout_report),
        (
            "a layout root span in the fixpoint",
            layout_root_span_in_the_fixpoint,
        ),
        ("a wake as the frame drains", wake_as_the_frame_drains),
        (
            "a layout report after a boundary laid out",
            layout_report_after_a_boundary_laid_out,
        ),
        ("a compositing hook", compositing_hook),
    ]);
}
