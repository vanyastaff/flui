//! `LifecycleContext::writer_source` (ADR-0086): the source a widget takes in
//! `init_state` writes into its own presentation's graph and rebuilds the
//! reader, and a write it opens inside its own `build` is refused by the
//! guard.

// ADR-0027: ElementBuildContext's test seam takes Arc<RwLock<…>> over a !Send
// owner graph; do not restore Send + Sync to satisfy clippy.
#![expect(clippy::arc_with_non_send_sync)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_view::prelude::*;
use flui_view::{ElementBuildContext, SignalError};
use parking_lot::RwLock;

const FRAME: Duration = Duration::from_millis(16);

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

/// What the probe hands back to the test.
#[derive(Default)]
struct Seen {
    /// The signal and source `init_state` acquired.
    acquired: RefCell<Option<(Signal<u32>, WriterSource)>>,
    /// Every value `build` read, in order.
    reads: RefCell<Vec<u32>>,
    /// The probe's element.
    id: Cell<Option<ElementId>>,
    /// What a write opened inside `build` returned, when the probe makes one.
    write_in_build: RefCell<Option<Result<(), SignalError>>>,
}

/// Acquires a signal and a writer source in `init_state` and reads the
/// signal in `build`. With `write_in_build`, `build` also runs a callback that
/// writes through the source, as a widget invoking a user callback
/// synchronously inside its own `build` would.
#[derive(Clone, StatefulView)]
struct Probe {
    seen: Rc<Seen>,
    write_in_build: bool,
}

#[derive(Default)]
struct ProbeState {
    count: Signal<u32>,
    writer: Option<WriterSource>,
}

impl StatefulView for Probe {
    type State = ProbeState;

    fn create_state(&self) -> Self::State {
        ProbeState::default()
    }
}

impl ViewState<Probe> for ProbeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count = ctx.signal(0);
        self.writer = Some(ctx.writer_source());
    }

    fn build(&self, view: &Probe, ctx: &dyn BuildContext) -> impl IntoView {
        view.seen.id.set(Some(ctx.element_id()));
        let writer = self
            .writer
            .clone()
            .expect("init_state acquired the writer source");
        view.seen
            .acquired
            .borrow_mut()
            .get_or_insert_with(|| (self.count, writer.clone()));
        view.seen.reads.borrow_mut().push(self.count.get(ctx));
        if view.write_in_build {
            let count = self.count;
            *view.seen.write_in_build.borrow_mut() = Some(writer.write(move |cx| count.set(cx, 9)));
        }
        Leaf
    }
}

fn mount(write_in_build: bool) -> (HeadlessBinding, Rc<Seen>) {
    let seen = Rc::new(Seen::default());
    let root = Probe {
        seen: Rc::clone(&seen),
        write_in_build,
    };
    let mut binding = HeadlessBinding::new();
    binding.mount_root(
        &root,
        MountOwners::fresh(),
        MountOptions::tight(100.0, 100.0),
    );
    binding.pump_frame(FRAME);
    (binding, seen)
}

#[test]
fn writer_source_from_init_state_writes_and_rebuilds_the_reader() {
    let (mut binding, seen) = mount(false);
    assert_eq!(
        *seen.reads.borrow(),
        [0],
        "mounted once, read the initial value"
    );
    let (count, source) = seen.acquired.borrow().clone().expect("the probe built");

    assert_eq!(source.write(|cx| count.set(cx, 5)), Ok(()));
    binding.pump_frame(FRAME);

    assert_eq!(
        *seen.reads.borrow(),
        [0, 5],
        "the write rebuilt the reader exactly once, and it read the new value"
    );
    let report = binding.build_owner_mut().last_frame_build_report();
    assert_eq!(report.count(RebuildReason::SignalChange), 1, "{report:?}");
}

/// The guard stays authoritative for the path the types cannot see: a
/// widget holding a `WriterSource` that opens a write inside its own `build`.
#[test]
fn a_callback_run_inside_its_widgets_build_is_refused_by_the_guard() {
    let (mut binding, seen) = mount(true);
    let own = seen.id.get().expect("the probe built");
    assert_eq!(
        *seen.write_in_build.borrow(),
        Some(Err(SignalError::WrittenDuringBuild { element: own }))
    );
    let (count, _) = seen.acquired.borrow().clone().expect("the probe built");
    let graph = binding.reactive().expect("tree-bound");
    assert_eq!(count.peek(&graph, |v| *v), Ok(0), "the value is unchanged");

    binding.pump_frame(FRAME);
    assert_eq!(
        *seen.reads.borrow(),
        [0],
        "the refused write scheduled no extra rebuild"
    );
}

/// The test context and the production context hand out a source over the
/// same graph the owner's signals are minted on, not a fresh one.
#[test]
fn the_test_context_writer_source_targets_the_owner_graph() {
    let tree = Arc::new(RwLock::new(ElementTree::new()));
    let owner = Arc::new(RwLock::new(BuildOwner::new()));
    let sig = owner.read().reactive().signal(1u32);
    let ctx = ElementBuildContext::new(ElementId::new(1), 0, false, tree, Arc::clone(&owner));

    assert_eq!(ctx.writer_source().write(|cx| sig.set(cx, 2)), Ok(()));
    assert_eq!(sig.peek(owner.read().reactive(), |v| *v), Ok(2));
}
