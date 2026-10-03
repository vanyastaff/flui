//! Signal reads through the `ReadScope` contract (ADR-0085 §2).
//!
//! - every context shape a view author writes accepts `sig.get(cx)`;
//! - a read in `build` subscribes the building element through the context
//!   production builds with (`BuildCtx`, reached by a real mount), and a write
//!   rebuilds exactly that element;
//! - a handle of the wrong type is a typed error on every path, never a panic,
//!   and a write through one marks no reader.

use std::cell::Cell;
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

pub(crate) fn run_released_loan_child() -> bool {
    let Ok(case) = std::env::var(LOAN_CHILD_CASE) else {
        return false;
    };
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
