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
