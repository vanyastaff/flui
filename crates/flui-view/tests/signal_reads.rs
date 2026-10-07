//! Signal reads through the `ReadScope` contract (ADR-0085 §2).
//!
//! - every context shape a view author writes accepts `sig.get(cx)`;
//! - a read in `build` subscribes the building element through the context
//!   production builds with (`BuildCtx`, reached by a real mount), and a write
//!   rebuilds exactly that element;
//! - a handle of the wrong type is a typed error on every path, never a panic,
//!   and a write through one marks no reader.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_view::prelude::*;
use flui_view::{Signal, SignalSender};

static_assertions::assert_not_impl_any!(Signal<u32>: Send, Sync);
static_assertions::assert_impl_all!(SignalSender<u32>: Send, Sync);

// ============================================================================
// Every context shape reads
// ============================================================================

// ============================================================================
// A read in `build` subscribes through the production context
// ============================================================================

/// A leaf that renders nothing, so a build chain bottoms out.
#[derive(Clone)]
struct Leaf;

impl RenderView for Leaf {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }
}

impl View for Leaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// Reads `sig` in `build`, counting builds and recording its own element.
#[derive(Clone, StatefulView)]
struct SigReader {
    sig: Signal<u32>,
    builds: Rc<Cell<u32>>,
    id: Rc<Cell<Option<ElementId>>>,
}

struct SigReaderState;

impl StatefulView for SigReader {
    type State = SigReaderState;
    fn create_state(&self) -> Self::State {
        SigReaderState
    }
}

impl ViewState<SigReader> for SigReaderState {
    fn build(&self, view: &SigReader, ctx: &dyn BuildContext) -> impl IntoView {
        view.builds.set(view.builds.get() + 1);
        view.id.set(Some(ctx.element_id()));
        let _ = view.sig.get(ctx);
        Leaf
    }
}

/// The reader's parent: reads no signal, counts builds.
#[derive(Clone, StatefulView)]
struct Bystander {
    child: SigReader,
    builds: Rc<Cell<u32>>,
}

struct BystanderState;

impl StatefulView for Bystander {
    type State = BystanderState;
    fn create_state(&self) -> Self::State {
        BystanderState
    }
}

impl ViewState<Bystander> for BystanderState {
    fn build(&self, view: &Bystander, _ctx: &dyn BuildContext) -> impl IntoView {
        view.builds.set(view.builds.get() + 1);
        view.child.clone()
    }
}

const FRAME: Duration = Duration::from_millis(16);

#[derive(Clone, StatelessView)]
struct ManyReader {
    signals: Rc<Vec<Signal<u32>>>,
    trigger: Signal<u32>,
    count: Rc<Cell<usize>>,
    id: Rc<Cell<Option<ElementId>>>,
    order: Rc<RefCell<Vec<usize>>>,
    label: usize,
}

impl StatelessView for ManyReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.id.set(Some(ctx.element_id()));
        self.order.borrow_mut().push(self.label);
        let _ = self.trigger.get(ctx);
        for signal in self.signals.iter().take(self.count.get()) {
            // Repeated reads must remain one subscription.
            let _ = signal.get(ctx);
            let _ = signal.get(ctx);
        }
        Leaf
    }
}

#[derive(Clone)]
struct ReaderRow(Vec<ManyReader>);

impl RenderView for ReaderRow {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        !self.0.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        for reader in &self.0 {
            visitor(reader);
        }
    }
}

impl View for ReaderRow {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

pub(crate) fn changing_read_sets_preserves_peer_rebuild_order() {
    let owners = MountOwners::fresh();
    let graph = owners.build_owner.reactive().clone();
    let signals = Rc::new((0..9).map(|_| graph.signal(0u32)).collect::<Vec<_>>());
    let order = Rc::new(RefCell::new(Vec::new()));
    let readers = (0..9)
        .map(|label| ManyReader {
            signals: Rc::clone(&signals),
            trigger: graph.signal(0u32),
            count: Rc::new(Cell::new(9)),
            id: Rc::new(Cell::new(None)),
            order: Rc::clone(&order),
            label,
        })
        .collect::<Vec<_>>();
    let mut binding = HeadlessBinding::new();
    binding.mount_root(
        &ReaderRow(readers.clone()),
        owners,
        MountOptions::tight(100.0, 100.0),
    );
    binding.pump_frame(FRAME);
    let mut expected = (0..9).collect::<Vec<_>>();

    for round in 0..24 {
        // Moving one reader to the end exercises repeated unlink/reinsert
        // while both membership directions have more than four entries.
        let label = (round * 5) % readers.len();
        readers[label]
            .trigger
            .set(&graph, round as u32)
            .expect("live");
        binding.pump_frame(FRAME);
        expected.retain(|value| *value != label);
        expected.push(label);
        order.borrow_mut().clear();
        signals[8].set(&graph, round as u32).expect("live");
        binding.pump_frame(FRAME);
        assert_eq!(*order.borrow(), expected, "peer order after re-reading");
        assert_eq!(
            graph.readers_of(signals[8].slot()),
            expected
                .iter()
                .map(|label| readers[*label].id.get().expect("mounted"))
                .collect::<Vec<_>>()
        );
    }

    // Shrink each read set and shared reader set, then grow them again.
    // Formerly read slots must not schedule an element after it opts out.
    for count in [2, 0, 9] {
        for reader in &readers {
            reader.count.set(count);
            reader.trigger.set(&graph, count as u32).expect("live");
        }
        binding.pump_frame(FRAME);
        order.borrow_mut().clear();
        signals[8].set(&graph, count as u32).expect("live");
        binding.pump_frame(FRAME);
        let expected = if count == 9 {
            (0..9).collect()
        } else {
            Vec::new()
        };
        assert_eq!(*order.borrow(), expected, "read-set change to {count}");
    }
    graph.release(signals[8].slot());
    let replacement = graph.signal(42u32);
    assert_eq!(graph.readers_of(replacement.slot()), [] as [ElementId; 0]);
    order.borrow_mut().clear();
    replacement.set(&graph, 43).expect("replacement is live");
    binding.pump_frame(FRAME);
    assert!(order.borrow().is_empty(), "slot reuse inherits no readers");
}

struct ReleaseProbe {
    graph: flui_view::Reactive,
    drops: Rc<Cell<usize>>,
    replacement: Rc<Cell<Option<Signal<u32>>>>,
    fails: bool,
}

impl Drop for ReleaseProbe {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        assert_eq!(
            self.graph.live_slot_count(),
            0,
            "release precedes retirement"
        );
        self.replacement.set(Some(self.graph.signal(7u32)));
        assert!(!self.fails, "first release destructor");
    }
}

pub(crate) fn explicit_release_allows_destructor_reentry_and_slot_reuse() {
    let graph = flui_view::Reactive::new();
    let drops = Rc::new(Cell::new(0));
    let replacement = Rc::new(Cell::new(None));
    let signal = graph.signal(ReleaseProbe {
        graph: graph.clone(),
        drops: Rc::clone(&drops),
        replacement: Rc::clone(&replacement),
        fails: false,
    });
    graph.release(signal.slot());
    assert_eq!(drops.get(), 1);
    assert!(matches!(
        signal.peek(&graph, |_| ()),
        Err(flui_view::SignalError::Released { .. })
    ));
    let next = replacement
        .get()
        .expect("destructor allocated a replacement");
    next.set(&graph, 9).expect("replacement is writable");
    assert_eq!(next.peek(&graph, |value| *value), Ok(9));
    graph.release(signal.slot());
    assert_eq!(
        next.peek(&graph, |value| *value),
        Ok(9),
        "stale release leaves replacement live"
    );
}

pub(crate) fn owner_release_commits_the_batch_before_the_first_destructor_failure() {
    struct Tail(Rc<Cell<usize>>);
    impl Drop for Tail {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
            panic!("later release destructor");
        }
    }
    let mut owners = MountOwners::fresh();
    let graph = owners.build_owner.reactive().clone();
    let element = owners.tree.mount_root_with_pipeline_owner(
        &Leaf,
        Some(owners.pipeline_owner.clone()),
        &mut owners.build_owner.element_owner_mut(),
    );
    let drops = Rc::new(Cell::new(0));
    let tail_drops = Rc::new(Cell::new(0));
    let replacement = Rc::new(Cell::new(None));
    let first = graph.signal_owned_by(
        element,
        ReleaseProbe {
            graph: graph.clone(),
            drops: Rc::clone(&drops),
            replacement: Rc::clone(&replacement),
            fails: true,
        },
    );
    let tail = graph.signal_owned_by(element, Tail(Rc::clone(&tail_drops)));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owners
            .tree
            .remove(element, &mut owners.build_owner.element_owner_mut());
    }))
    .expect_err("the first destructor failure propagates");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("first release destructor")
    );
    assert_eq!(
        (drops.get(), tail_drops.get()),
        (1, 0),
        "later opaque retirement is retained"
    );
    assert!(matches!(
        first.peek(&graph, |_| ()),
        Err(flui_view::SignalError::Released { .. })
    ));
    assert!(matches!(
        tail.peek(&graph, |_| ()),
        Err(flui_view::SignalError::Released { .. })
    ));
    let next = replacement
        .get()
        .expect("first destructor reentered the graph");
    next.set(&graph, 11)
        .expect("the next write succeeds after containment");
    assert_eq!(next.peek(&graph, |value| *value), Ok(11));
}

pub(crate) fn owner_release_refuses_signals_its_destructors_reintroduce() {
    struct Late(Rc<Cell<usize>>);
    impl Drop for Late {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    struct Reintroduce {
        graph: flui_view::Reactive,
        element: ElementId,
        late_drops: Rc<Cell<usize>>,
        refused: Rc<Cell<Option<bool>>>,
    }
    impl Drop for Reintroduce {
        fn drop(&mut self) {
            let late = self
                .graph
                .try_signal_owned_by(self.element, Late(Rc::clone(&self.late_drops)));
            self.refused.set(Some(matches!(
                late,
                Err(flui_view::SignalError::Released { .. })
            )));
        }
    }
    // Recreates itself from its destructor, at most `LIMIT` times, through
    // the panicking constructor.
    struct Producer {
        graph: flui_view::Reactive,
        element: ElementId,
        attempts: Rc<Cell<usize>>,
    }
    impl Drop for Producer {
        fn drop(&mut self) {
            const LIMIT: usize = 32;
            self.attempts.set(self.attempts.get() + 1);
            if self.attempts.get() < LIMIT {
                let _next = self.graph.signal_owned_by(
                    self.element,
                    Producer {
                        graph: self.graph.clone(),
                        element: self.element,
                        attempts: Rc::clone(&self.attempts),
                    },
                );
            }
        }
    }

    let mut owners = MountOwners::fresh();
    let graph = owners.build_owner.reactive().clone();
    let element = owners.tree.mount_root_with_pipeline_owner(
        &Leaf,
        Some(owners.pipeline_owner.clone()),
        &mut owners.build_owner.element_owner_mut(),
    );
    let late_drops = Rc::new(Cell::new(0));
    let refused = Rc::new(Cell::new(None));
    let _owned = graph.signal_owned_by(
        element,
        Reintroduce {
            graph: graph.clone(),
            element,
            late_drops: Rc::clone(&late_drops),
            refused: Rc::clone(&refused),
        },
    );
    owners
        .tree
        .remove(element, &mut owners.build_owner.element_owner_mut());
    assert_eq!(
        refused.get(),
        Some(true),
        "a departing element admits no new owned signal"
    );
    assert_eq!(late_drops.get(), 1, "the refused value is dropped");
    assert_eq!(graph.live_slot_count(), 0);

    let element = owners.tree.mount_root_with_pipeline_owner(
        &Leaf,
        Some(owners.pipeline_owner.clone()),
        &mut owners.build_owner.element_owner_mut(),
    );
    let attempts = Rc::new(Cell::new(0));
    let _producer = graph.signal_owned_by(
        element,
        Producer {
            graph: graph.clone(),
            element,
            attempts: Rc::clone(&attempts),
        },
    );
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owners
            .tree
            .remove(element, &mut owners.build_owner.element_owner_mut());
    }))
    .expect_err("the refused recreation propagates from the destructor");
    assert!(
        flui_foundation::panic::payload_text(&*failure)
            .is_some_and(|text| text.starts_with("Reactive::signal_owned_by")),
        "the refusal is the reported failure"
    );
    assert_eq!(
        attempts.get(),
        1,
        "the release ends after one pass; the refused producer is retained"
    );
    assert_eq!(graph.live_slot_count(), 0);
    let next = graph.signal(3u32);
    assert_eq!(
        next.peek(&graph, |value| *value),
        Ok(3),
        "the graph admits signals after the contained refusal"
    );
}

/// Recreates itself from every destructor through the fallible constructor,
/// with no limit of its own.
struct Recreate {
    graph: flui_view::Reactive,
    element: ElementId,
    drops: Rc<Cell<usize>>,
}
impl Drop for Recreate {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        let next = self.graph.try_signal_owned_by(
            self.element,
            Recreate {
                graph: self.graph.clone(),
                element: self.element,
                drops: Rc::clone(&self.drops),
            },
        );
        assert!(
            matches!(next, Err(flui_view::SignalError::Released { .. })),
            "a departing element admits no new owned signal"
        );
    }
}

pub(crate) fn owner_release_bounds_a_destructor_that_always_recreates_itself() {
    let mut owners = MountOwners::fresh();
    let graph = owners.build_owner.reactive().clone();
    let drops = Rc::new(Cell::new(0));
    // Twice: the second release shows the first left no refusal state behind.
    for pass in 1..=2 {
        let element = owners.tree.mount_root_with_pipeline_owner(
            &Leaf,
            Some(owners.pipeline_owner.clone()),
            &mut owners.build_owner.element_owner_mut(),
        );
        let _owned = graph.signal_owned_by(
            element,
            Recreate {
                graph: graph.clone(),
                element,
                drops: Rc::clone(&drops),
            },
        );
        owners
            .tree
            .remove(element, &mut owners.build_owner.element_owner_mut());
        assert_eq!(
            drops.get(),
            2 * pass,
            "the released value and its first refused recreation drop; the nested refusal is retained"
        );
        assert_eq!(graph.live_slot_count(), 0);
    }

    let element = owners.tree.mount_root_with_pipeline_owner(
        &Leaf,
        Some(owners.pipeline_owner.clone()),
        &mut owners.build_owner.element_owner_mut(),
    );
    let next = graph.signal_owned_by(element, 5u32);
    assert_eq!(
        next.peek(&graph, |value| *value),
        Ok(5),
        "the next element owns signals after the bounded refusal"
    );
    owners
        .tree
        .remove(element, &mut owners.build_owner.element_owner_mut());
    assert_eq!(graph.live_slot_count(), 0);
}

pub(crate) fn a_panicking_refusal_diagnostic_cannot_unwind_into_a_retained_value() {
    use tracing_subscriber::{Layer, layer::Context, prelude::*};

    /// Whether an event's message reports a nested refusal.
    struct NestedRefusal(bool);
    impl tracing::field::Visit for NestedRefusal {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.0 |= format!("{value:?}").contains("caused another refusal");
            }
        }
    }
    /// Panics on the diagnostic a nested refusal reports.
    struct HostileSignalsLayer;
    impl<S: tracing::Subscriber> Layer<S> for HostileSignalsLayer {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            let mut nested = NestedRefusal(false);
            event.record(&mut nested);
            if nested.0 {
                std::panic::panic_any("hostile signals subscriber");
            }
        }
    }

    let mut owners = MountOwners::fresh();
    let graph = owners.build_owner.reactive().clone();
    let drops = Rc::new(Cell::new(0));
    let element = owners.tree.mount_root_with_pipeline_owner(
        &Leaf,
        Some(owners.pipeline_owner.clone()),
        &mut owners.build_owner.element_owner_mut(),
    );
    let _owned = graph.signal_owned_by(
        element,
        Recreate {
            graph: graph.clone(),
            element,
            drops: Rc::clone(&drops),
        },
    );
    let subscriber = tracing_subscriber::registry().with(HostileSignalsLayer);
    let failure = tracing::subscriber::with_default(subscriber, || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            owners
                .tree
                .remove(element, &mut owners.build_owner.element_owner_mut());
        }))
    })
    .expect_err("the subscriber's panic propagates from the release");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"hostile signals subscriber"),
        "the subscriber's panic is the reported failure, not a second refusal"
    );
    assert_eq!(
        drops.get(),
        2,
        "the released value and its first refused recreation drop; the nested one is retained before the diagnostic"
    );

    let element = owners.tree.mount_root_with_pipeline_owner(
        &Leaf,
        Some(owners.pipeline_owner.clone()),
        &mut owners.build_owner.element_owner_mut(),
    );
    let next = graph.signal_owned_by(element, 5u32);
    assert_eq!(
        next.peek(&graph, |value| *value),
        Ok(5),
        "the graph admits owned signals after the contained diagnostic panic"
    );
}

pub(crate) fn a_read_in_build_subscribes_through_the_production_context() {
    let owners = MountOwners::fresh();
    let graph = owners.build_owner.reactive().clone();
    let sig = graph.signal(1u32);
    let reader_builds = Rc::new(Cell::new(0));
    let bystander_builds = Rc::new(Cell::new(0));
    let reader_id = Rc::new(Cell::new(None));
    let root = Bystander {
        child: SigReader {
            sig,
            builds: Rc::clone(&reader_builds),
            id: Rc::clone(&reader_id),
        },
        builds: Rc::clone(&bystander_builds),
    };

    let mut binding = HeadlessBinding::new();
    binding.mount_root(&root, owners, MountOptions::tight(100.0, 100.0));
    assert_eq!(
        binding.reactive().expect("tree-bound").id(),
        graph.id(),
        "the binding's graph is the owner's"
    );
    binding.pump_frame(FRAME);
    assert_eq!(
        (reader_builds.get(), bystander_builds.get()),
        (1, 1),
        "mounted once; an idle frame rebuilds nothing"
    );
    let reader = reader_id.get().expect("the reader built");
    assert_eq!(graph.readers_of(sig.slot()), [reader]);

    sig.set(&graph, 2)
        .expect("a write outside the frame phases");
    binding.pump_frame(FRAME);

    assert_eq!(reader_builds.get(), 2, "the write rebuilds the reader");
    assert_eq!(bystander_builds.get(), 1, "and nothing else");
    // The drain rebuilt the reader for the write, and the leaf below it as
    // that rebuild's child update; nothing was scheduled for any other reason.
    let report = binding.build_owner_mut().last_frame_build_report();
    assert_eq!(report.elements_built, 2, "{report:?}");
    assert_eq!(report.count(RebuildReason::SignalChange), 1, "{report:?}");
    assert_eq!(report.count(RebuildReason::ParentUpdate), 1, "{report:?}");
    assert_eq!(
        graph.readers_of(sig.slot()),
        [reader],
        "the rebuild re-subscribed it"
    );
}

pub(crate) fn a_partially_committed_panicking_update_rebuilds_its_mounted_reader() {
    let owners = MountOwners::fresh();
    let graph = owners.build_owner.reactive().clone();
    let signal = graph.signal(1u32);
    let builds = Rc::new(Cell::new(0));
    let reader_id = Rc::new(Cell::new(None));
    let root = SigReader {
        sig: signal,
        builds: Rc::clone(&builds),
        id: Rc::clone(&reader_id),
    };
    let mut binding = HeadlessBinding::new();
    binding.mount_root(&root, owners, MountOptions::tight(100.0, 100.0));
    binding.pump_frame(FRAME);

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        signal.update(&graph, |value| {
            *value = 9;
            panic!("update probe");
        })
    }));
    assert!(outcome.is_err());
    assert_eq!(signal.peek(&graph, |value| *value), Ok(9));

    binding.pump_frame(FRAME);
    assert_eq!(
        builds.get(),
        2,
        "the partial commit invalidates its live reader"
    );
    let report = binding.build_owner_mut().last_frame_build_report();
    assert_eq!(report.count(RebuildReason::SignalChange), 1, "{report:?}");
}

// ============================================================================
// A handle of the wrong type
// ============================================================================

// Released loan values may have aggregate drop glue that aborts the process.
// Exercise those negative variants in children, keeping the family table alive.
const LOAN_CHILD_CASE: &str = "FLUI_SIGNAL_RELEASED_LOAN_CASE";

fn released_loan_child(case: &str) {
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "signal_read_and_write_matrix", "--nocapture"])
        .env(LOAN_CHILD_CASE, case)
        .output()
        .expect("released-loan child process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success()
            && stdout.contains("running 1 test")
            && stdout.contains("test result: ok. 1 passed; 0 failed;"),
        "{case}: child failed: {}\n{}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn released_update_retains_aggregate_before_resuming_failure() {
    released_loan_child("update aggregate");
}

pub(crate) fn released_read_retains_aggregate_before_resuming_failure() {
    released_loan_child("read aggregate");
}

pub(crate) fn released_update_retains_nested_release_obligations() {
    released_loan_child("update nested");
}

pub(crate) fn released_read_retains_nested_release_obligations() {
    released_loan_child("read nested");
}

pub(crate) fn released_update_reports_ordinary_retirement_failure() {
    released_loan_child("update retirement");
}

pub(crate) fn released_read_reports_ordinary_retirement_failure() {
    released_loan_child("read retirement");
}

pub(crate) fn release_during_unwind_preserves_the_primary_failure() {
    released_loan_child("release during unwind");
}

pub(crate) fn run_released_loan_child() -> bool {
    let Ok(case) = std::env::var(LOAN_CHILD_CASE) else {
        return false;
    };
    if case == "release during unwind" {
        struct Bomb(Rc<Cell<usize>>);
        impl Drop for Bomb {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
                panic!("secondary release destructor");
            }
        }
        struct ReleaseOnUnwind(flui_view::Reactive, flui_view::SignalSlot);
        impl Drop for ReleaseOnUnwind {
            fn drop(&mut self) {
                self.0.release(self.1);
            }
        }
        let graph = flui_view::Reactive::new();
        let drops = Rc::new(Cell::new(0));
        let signal = graph.signal(Bomb(Rc::clone(&drops)));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _release = ReleaseOnUnwind(graph.clone(), signal.slot());
            panic!("primary release failure");
        }))
        .expect_err("the primary panic propagates");
        assert_eq!(
            flui_foundation::panic::payload_text(&*failure),
            Some("primary release failure")
        );
        assert_eq!(drops.get(), 0, "opaque value retained during unwind");
        assert_eq!(graph.live_slot_count(), 0);
        let next = graph.signal(3u32);
        next.set(&graph, 5)
            .expect("next write after failed release");
        assert_eq!(next.peek(&graph, |value| *value), Ok(5));
        return true;
    }
    let (update, nested, callback_fails) = match case.as_str() {
        "update aggregate" => (true, false, true),
        "read aggregate" => (false, false, true),
        "update nested" => (true, true, true),
        "read nested" => (false, true, true),
        "update retirement" => (true, false, false),
        "read retirement" => (false, false, false),
        _ => panic!("unknown released-loan child case: {case}"),
    };
    struct Bomb(Rc<Cell<usize>>, bool);
    impl Drop for Bomb {
        fn drop(&mut self) {
            if self.1 {
                self.0.set(self.0.get() + 1);
                panic!("released value destructor");
            }
        }
    }
    struct ReleaseOnDrop {
        graph: flui_view::reactive::Reactive,
        slot: flui_view::SignalSlot,
        drops: Rc<Cell<usize>>,
    }
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            self.graph.release(self.slot);
        }
    }
    let graph = flui_view::reactive::Reactive::new();
    let next = graph.signal(10u32);
    let drops = Rc::new(Cell::new(0));
    let nested_drops = Rc::new(Cell::new(0));
    let obligation = nested.then(|| ReleaseOnDrop {
        graph: graph.clone(),
        slot: next.slot(),
        drops: Rc::clone(&nested_drops),
    });
    let signal = graph.signal((
        Bomb(Rc::clone(&drops), true),
        Bomb(Rc::clone(&drops), callback_fails),
        obligation,
    ));
    let capture_drops = Rc::new(Cell::new(0));
    let capture = Bomb(Rc::clone(&capture_drops), true);
    let callback_graph = graph.clone();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if update {
            signal.update(&graph, move |_| {
                let _keep_capture = &capture;
                callback_graph.release(signal.slot());
                assert!(!callback_fails, "primary callback failure");
            })
        } else {
            signal.peek(&graph, move |_| {
                let _keep_capture = &capture;
                callback_graph.release(signal.slot());
                assert!(!callback_fails, "primary callback failure");
            })
        }
    }))
    .expect_err("callback failed");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some(if callback_fails {
            "primary callback failure"
        } else {
            "released value destructor"
        })
    );
    assert_eq!(drops.get(), usize::from(!callback_fails));
    assert_eq!(
        capture_drops.get(),
        0,
        "capture destruction stays secondary"
    );
    assert_eq!(nested_drops.get(), 0, "nested obligations remain retained");
    assert!(matches!(
        signal.peek(&graph, |_| ()),
        Err(flui_view::SignalError::Released { .. })
    ));
    next.update(&graph, |value| *value += 1)
        .expect("next live update");
    assert_eq!(next.peek(&graph, |value| *value), Ok(11));
    true
}
