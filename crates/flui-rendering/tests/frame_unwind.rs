//! A frame that unwinds, out of a `tracing` subscriber, a visual-update wake or a render
//! object's hook, leaves the pipeline its owner (ADR-0048): the owner stays in its
//! `PipelineCell` with the work the frame had not finished, the retry at unchanged constraints
//! paints, and invalidation handles keep reaching it. The paint a pass committed before it failed
//! is announced on every exit, an error or a panic included (ADR-0287).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
};

use flui_foundation::RenderId;
use flui_foundation::Single;
use flui_foundation::geometry::{Matrix4, Size};
use flui_objects::{RenderColoredBox, RenderRepaintBoundary, RenderStack};
use flui_rendering::{
    TextContextHandle,
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    error::RenderError,
    parent_data::BoxParentData,
    pipeline::{PipelineCell, PipelineOwner},
    protocol::{BoxProtocol, ProtocolGeometry},
    testing::SceneHost,
    traits::{HitTestOutcome, RenderBox, RenderObject},
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

/// `RenderStack → [RenderRepaintBoundary → RenderColoredBox, Rigged, Rigged]`, painted once at
/// 100×100, then asked for 150×150: the next frame owes layout and paint.
struct Fixture {
    cell: PipelineCell,
    scene: std::cell::RefCell<SceneHost>,
    text: TextContextHandle,
    root: RenderId,
    boundary: RenderId,
    rigged: RenderId,
    fails_layout: Arc<AtomicBool>,
    compositing: Arc<AtomicU8>,
    /// The second rigged leaf, after the first in the walks.
    late: RenderId,
    late_compositing: Arc<AtomicU8>,
}

/// The fixture's tree, before its first frame.
struct Tree {
    owner: PipelineOwner,
    root: RenderId,
    boundary: RenderId,
    rigged: RenderId,
    fails_layout: Arc<AtomicBool>,
    compositing: Arc<AtomicU8>,
    late: RenderId,
    late_compositing: Arc<AtomicU8>,
}

fn tree(text: &TextContextHandle) -> Tree {
    let mut owner = PipelineOwner::new(text.clone());
    let root = owner.set_root_render_object(Box::new(RenderStack::new()));
    let boundary = owner
        .insert_child_render_object(root, Box::new(RenderRepaintBoundary::new()))
        .expect("the boundary attaches");
    owner
        .insert_child_render_object(boundary, Box::new(RenderColoredBox::red(10.0, 10.0)))
        .expect("the boundary's child attaches");
    let rigged = |owner: &mut PipelineOwner, fails_layout: &Arc<AtomicBool>| {
        let compositing = Arc::new(AtomicU8::new(COMPOSITED));
        let id = owner
            .insert_child_render_object(
                root,
                Box::new(Rigged {
                    fails_layout: Arc::clone(fails_layout),
                    compositing: Arc::clone(&compositing),
                }),
            )
            .expect("a rigged leaf attaches");
        (id, compositing)
    };
    let fails_layout = Arc::new(AtomicBool::new(false));
    let (rigged_id, compositing) = rigged(&mut owner, &fails_layout);
    let (late, late_compositing) = rigged(&mut owner, &Arc::new(AtomicBool::new(false)));
    owner.set_root_constraints(Some(tight(100.0)));
    Tree {
        owner,
        root,
        boundary,
        rigged: rigged_id,
        fails_layout,
        compositing,
        late,
        late_compositing,
    }
}

impl Fixture {
    /// One frame of the cell's owner into the fixture's scene.
    fn run_frame(
        &self,
    ) -> Result<Option<flui_layer::LayerTree>, flui_foundation::TreeError<RenderError>> {
        self.scene.borrow_mut().run_cell_frame(&self.cell)
    }
}

fn fixture() -> Fixture {
    let text = TextContextHandle::standalone();
    let Tree {
        owner,
        root,
        boundary,
        rigged,
        fails_layout,
        compositing,
        late,
        late_compositing,
    } = tree(&text);
    let cell = PipelineCell::new(owner);
    let mut scene = SceneHost::new();
    assert!(
        matches!(scene.run_cell_frame(&cell), Ok(Some(_))),
        "the first frame paints"
    );
    cell.with_mut(|owner| owner.set_root_constraints(Some(tight(150.0))));
    Fixture {
        cell,
        scene: std::cell::RefCell::new(scene),
        text,
        root,
        boundary,
        rigged,
        fails_layout,
        compositing,
        late,
        late_compositing,
    }
}

/// Counts the wakes `owner` sends from here on.
fn count_wakes<Phase: flui_rendering::pipeline::PipelinePhase>(
    owner: &mut PipelineOwner<Phase>,
) -> Arc<AtomicUsize> {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    owner.set_on_need_visual_update(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    wakes
}

/// Installs a wake that panics the first time it runs, and reports whether it ran.
fn wake_panics_once<Phase: flui_rendering::pipeline::PipelinePhase>(
    owner: &mut PipelineOwner<Phase>,
) -> Arc<AtomicBool> {
    let armed = Arc::new(AtomicBool::new(true));
    let wake_armed = Arc::clone(&armed);
    owner.set_on_need_visual_update(move || {
        assert!(
            !wake_armed.swap(false, Ordering::SeqCst),
            "a wake that panics"
        );
    });
    armed
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
        matches!(f.run_frame(), Ok(Some(_))),
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
        matches!(f.run_frame(), Ok(Some(_))),
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
    unwinds_under(Trigger::SpanOpens("paint"), || f.run_frame());
    the_pipeline_kept_its_owner(&f);
}

/// The layer count report comes after the paint walk, with the paint phase open. A layout mark
/// after the unwind is an ordinary one, not a mark made while painting.
fn paint_report() {
    let f = fixture();
    unwinds_under(
        Trigger::Event {
            target: "flui_rendering::pipeline::owner::paint",
            level: tracing::Level::DEBUG,
        },
        || f.run_frame(),
    );
    f.cell
        .with(|owner| owner.local_render_invalidation_handle(f.root))
        .expect("the root is attached")
        .mark_needs_layout()
        .expect("the owner is live");
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
        || f.run_frame(),
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
    let unwound = catch_unwind(AssertUnwindSafe(|| f.run_frame()));
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
    let wakes = f.cell.with_mut(count_wakes);
    f.fails_layout.store(true, Ordering::SeqCst);
    unwinds_under(
        Trigger::Event {
            target: "flui_rendering::pipeline::owner::subtree_arena",
            level: tracing::Level::DEBUG,
        },
        || f.run_frame(),
    );
    f.fails_layout.store(false, Ordering::SeqCst);
    assert!(
        f.cell.with(|owner| owner
            .nodes_needing_paint()
            .iter()
            .any(|entry| entry.id == f.boundary)),
        "the boundary that laid out owes its paint"
    );
    assert!(
        wakes.load(Ordering::SeqCst) > 0,
        "the paint the boundary owes is announced"
    );
    assert!(
        f.cell.with(|owner| owner
            .render_tree()
            .get(f.root)
            .expect("the root is live")
            .needs_layout()),
        "the root, whose layout the panic cut short, lays out again"
    );
    the_pipeline_kept_its_owner(&f);
}

/// A compositing hook panics after the walk took a change from an earlier node: the node whose
/// hook panicked keeps the compositing answer it had, so the retry still sees its change, and the
/// paint the earlier change owes is queued and announced.
fn compositing_hook() {
    let f = fixture();
    let composited = |id: RenderId| {
        f.cell.with(|owner| {
            owner
                .render_tree()
                .get(id)
                .expect("the rigged leaf is live")
                .needs_compositing()
        })
    };
    let mark_both = |owner: &mut PipelineOwner| {
        owner.mark_needs_compositing_bits_update(f.rigged);
        owner.mark_needs_compositing_bits_update(f.late);
    };
    // Newly attached leaves are composited once their bits are first recomputed.
    f.cell.with_mut(mark_both);
    assert!(
        matches!(f.run_frame(), Ok(Some(_))),
        "the frame that composites the rigged leaves paints"
    );
    assert!(
        composited(f.rigged) && composited(f.late),
        "the rigged leaves are composited"
    );
    f.compositing.store(PLAIN, Ordering::SeqCst);
    f.late_compositing.store(PANICS, Ordering::SeqCst);
    f.cell.with_mut(mark_both);
    let wakes = f.cell.with_mut(count_wakes);
    let unwound = catch_unwind(AssertUnwindSafe(|| f.run_frame()));
    match unwound {
        Ok(_) => panic!("the hook's panic goes on to the caller"),
        Err(payload) => flui_foundation::panic::discard_payload(payload),
    }
    assert!(
        !composited(f.rigged),
        "the walk took the first leaf's change"
    );
    assert!(
        f.cell.with(|owner| owner
            .nodes_needing_paint()
            .iter()
            .any(|entry| entry.id == f.root)),
        "the first leaf's change owes the paint of its repaint boundary"
    );
    assert!(
        wakes.load(Ordering::SeqCst) > 0,
        "the paint the change owes is announced"
    );
    f.late_compositing.store(PLAIN, Ordering::SeqCst);
    assert!(
        composited(f.late),
        "the panic left the second leaf its old compositing answer"
    );
    the_pipeline_kept_its_owner(&f);
    assert!(
        !composited(f.late),
        "the retry took the second leaf's new compositing answer"
    );
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

/// Lays its child out loose within its own size, then fails while armed, with an error that
/// counts toward its poison budget rather than poisoning it at once.
#[derive(Debug)]
struct FailsAfterItsChild(Arc<AtomicBool>);

impl flui_foundation::inspect::Inspect for FailsAfterItsChild {
    fn kind(&self) -> &'static str {
        "FailsAfterItsChild"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl RenderBox for FailsAfterItsChild {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let size = ctx.constraints().biggest();
        ctx.layout_child(0, BoxConstraints::tight(size).loosen())?;
        if self.0.load(Ordering::SeqCst) {
            return Err(RenderError::invalid_constraints(
                "layout fails after the child laid out",
            ));
        }
        Ok(size)
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Self::Arity, Self::ParentData>) -> bool {
        false
    }
}

/// Paints `owner`'s first frame at 100×100 and asks for 150×150, so the next layout owes
/// paint the queue does not hold yet.
fn painted_then_resized(owner: &mut PipelineOwner) {
    assert!(
        matches!(SceneHost::new().run_frame_in_place(owner), Ok(Some(_))),
        "the first frame paints"
    );
    owner.set_root_constraints(Some(tight(150.0)));
}

/// `FailsAfterItsChild → RenderRepaintBoundary → RenderColoredBox`, painted and resized, then
/// armed: its layout fails after the boundary laid out at the new size, which leaves the
/// boundary's paint queued.
fn fails_after_a_boundary() -> (PipelineOwner, RenderId, RenderId) {
    let mut owner = PipelineOwner::new(TextContextHandle::standalone());
    let fails = Arc::new(AtomicBool::new(false));
    let root = owner.set_root_render_object(Box::new(FailsAfterItsChild(Arc::clone(&fails))));
    let boundary = owner
        .insert_child_render_object(root, Box::new(RenderRepaintBoundary::new()))
        .expect("the boundary attaches");
    owner
        .insert_child_render_object(boundary, Box::new(RenderColoredBox::red(10.0, 10.0)))
        .expect("the boundary's child attaches");
    owner.set_root_constraints(Some(tight(100.0)));
    painted_then_resized(&mut owner);
    fails.store(true, Ordering::SeqCst);
    (owner, root, boundary)
}

/// The cell's layout pass fails at a dirty root after a boundary laid out, and the wake that
/// announces the boundary's paint panics: the error is the first failure and is returned.
fn an_error_in_the_layout_pass() {
    let (mut owner, _, boundary) = fails_after_a_boundary();
    let woke = wake_panics_once(&mut owner);
    let cell = PipelineCell::new(owner);
    let failed = catch_unwind(AssertUnwindSafe(|| cell.run_layout()));
    assert!(
        matches!(failed, Ok(Err(_))),
        "the layout error reaches the caller, not the wake's panic"
    );
    assert!(!woke.load(Ordering::SeqCst), "the wake was sent");
    assert!(
        cell.with(|owner| owner
            .nodes_needing_paint()
            .iter()
            .any(|entry| entry.id == boundary)),
        "the boundary that laid out owes its paint"
    );
}

/// The same failure through a single dirty root's layout.
fn an_error_at_a_dirty_root() {
    let (mut owner, root, _) = fails_after_a_boundary();
    let woke = wake_panics_once(&mut owner);
    let mut owner = owner.into_layout();
    let failed = catch_unwind(AssertUnwindSafe(|| {
        owner.layout_dirty_root(root, tight(150.0))
    }));
    assert!(
        matches!(failed, Ok(Err(_))),
        "the layout error reaches the caller, not the wake's panic"
    );
    assert!(!woke.load(Ordering::SeqCst), "the wake was sent");
}

/// A subscriber panics in a single dirty root's layout after a boundary laid out: the paint
/// the boundary owes is announced before the panic goes on.
fn a_panic_at_a_dirty_root() {
    let text = TextContextHandle::standalone();
    let Tree {
        mut owner,
        root,
        fails_layout,
        ..
    } = tree(&text);
    painted_then_resized(&mut owner);
    fails_layout.store(true, Ordering::SeqCst);
    let wakes = count_wakes(&mut owner);
    let mut owner = owner.into_layout();
    let before = wakes.load(Ordering::SeqCst);
    unwinds_under(
        Trigger::Event {
            target: "flui_rendering::pipeline::owner::subtree_arena",
            level: tracing::Level::DEBUG,
        },
        || owner.layout_dirty_root(root, tight(150.0)),
    );
    assert!(
        wakes.load(Ordering::SeqCst) > before,
        "the paint the boundary owes is announced"
    );
}

#[test]
fn a_layout_pass_announces_its_paint_on_every_exit() {
    crate::run_table(&[
        ("an error in the layout pass", an_error_in_the_layout_pass),
        ("an error at a dirty root", an_error_at_a_dirty_root),
        ("a panic at a dirty root", a_panic_at_a_dirty_root),
    ]);
}

/// The dirty root lays out while its child's first poison report panics. The root's own success
/// is recorded before the panic goes on, so its earlier failures stop counting toward its poison
/// budget.
#[test]
fn a_poison_report_that_panics_comes_after_the_dirty_roots_success() {
    let mut owner = PipelineOwner::new(TextContextHandle::standalone());
    let root_fails = Arc::new(AtomicBool::new(false));
    let root = owner.set_root_render_object(Box::new(FailsAfterItsChild(Arc::clone(&root_fails))));
    let child_fails = Arc::new(AtomicBool::new(false));
    owner
        .insert_child_render_object(
            root,
            Box::new(Rigged {
                fails_layout: Arc::clone(&child_fails),
                compositing: Arc::new(AtomicU8::new(PLAIN)),
            }),
        )
        .expect("the child attaches");
    owner.set_root_constraints(Some(tight(100.0)));
    let cell = PipelineCell::new(owner);
    let mut scene = SceneHost::new();
    assert!(
        matches!(scene.run_cell_frame(&cell), Ok(Some(_))),
        "the first frame paints"
    );

    // Two failures of the root's own layout, one short of its poison budget.
    root_fails.store(true, Ordering::SeqCst);
    for side in [150.0, 200.0] {
        cell.with_mut(|owner| owner.set_root_constraints(Some(tight(side))));
        assert!(
            scene.run_cell_frame(&cell).is_err(),
            "the root's layout fails"
        );
    }

    // New constraints reach the child, whose layout now fails for the first time.
    root_fails.store(false, Ordering::SeqCst);
    child_fails.store(true, Ordering::SeqCst);
    cell.with_mut(|owner| owner.set_root_constraints(Some(tight(225.0))));
    unwinds_under(
        Trigger::Event {
            target: "flui_rendering::pipeline::owner::layout",
            level: tracing::Level::ERROR,
        },
        || scene.run_cell_frame(&cell),
    );

    // The first failure of the root since its success.
    child_fails.store(false, Ordering::SeqCst);
    root_fails.store(true, Ordering::SeqCst);
    cell.with_mut(|owner| owner.set_root_constraints(Some(tight(250.0))));
    assert!(
        scene.run_cell_frame(&cell).is_err(),
        "the root's layout fails"
    );
    assert!(
        !cell.with(|owner| owner.is_layout_poisoned(root)),
        "the root's success restarted its poison budget"
    );
}
