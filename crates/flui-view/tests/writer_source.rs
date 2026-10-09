//! `LifecycleContext::writer_source` (ADR-0086): the source a widget takes in
//! `init_state` writes into its own presentation's graph and rebuilds the
//! reader, and a write it opens inside its own `build` is refused by the
//! guard.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_view::SignalError;
use flui_view::prelude::*;

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

pub(crate) fn writer_source_from_init_state_writes_and_rebuilds_the_reader() {
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

#[derive(Clone, StatefulView)]
struct LocalStateProbe {
    count: flui_view::StateCell<u32>,
    items: flui_view::StateHandle<Vec<u32>>,
    attempted: Rc<Cell<bool>>,
    callbacks: Rc<Cell<usize>>,
    reads: Rc<RefCell<Vec<(u32, usize)>>>,
}

struct LocalStateProbeState {
    count: flui_view::StateCell<u32>,
    items: flui_view::StateHandle<Vec<u32>>,
}

impl StatefulView for LocalStateProbe {
    type State = LocalStateProbeState;
    fn create_state(&self) -> Self::State {
        LocalStateProbeState {
            count: self.count.clone(),
            items: self.items.clone(),
        }
    }
}

impl ViewState<LocalStateProbe> for LocalStateProbeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count.bind(ctx);
        self.items.bind(ctx);
    }
    fn build(&self, view: &LocalStateProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        if !view.attempted.replace(true) {
            self.count.set(9);
            self.count.update(|value| {
                view.callbacks.set(view.callbacks.get() + 1);
                value + 1
            });
            self.items.update(|items| {
                view.callbacks.set(view.callbacks.get() + 1);
                items.push(9);
            });
        }
        view.reads
            .borrow_mut()
            .push((self.count.get(), self.items.with(Vec::len)));
        Leaf
    }
}

pub(crate) fn bound_local_state_refuses_build_mutation_then_recovers() {
    let count = flui_view::StateCell::new(0);
    let items = flui_view::StateHandle::new(Vec::new());
    let callbacks = Rc::new(Cell::new(0));
    let reads = Rc::new(RefCell::new(Vec::new()));
    let root = LocalStateProbe {
        count: count.clone(),
        items: items.clone(),
        attempted: Rc::new(Cell::new(false)),
        callbacks: callbacks.clone(),
        reads: reads.clone(),
    };
    let mut binding = HeadlessBinding::new();
    binding.mount_root(
        &root,
        MountOwners::fresh(),
        MountOptions::tight(100.0, 100.0),
    );
    binding.pump_frame(FRAME);
    assert_eq!(
        count.get(),
        0,
        "a refused build write leaves the accepted value intact"
    );
    assert_eq!(items.with(Vec::len), 0);
    assert_eq!(
        callbacks.get(),
        0,
        "refusal precedes invoking the mutation closure"
    );
    assert_eq!(*reads.borrow(), [(0, 0)], "refusal creates no rebuild debt");

    count.set(5);
    items.update(|items| items.push(5));
    binding.pump_frame(FRAME);
    assert_eq!(
        *reads.borrow(),
        [(0, 0), (5, 1)],
        "a later admitted mutation rebuilds once"
    );
}

pub(crate) fn a_panicking_local_update_rebuilds_its_committed_value() {
    for competing_wake_failure in [false, true] {
        let count = flui_view::StateCell::new(0);
        let items = flui_view::StateHandle::new(Vec::new());
        let reads = Rc::new(RefCell::new(Vec::new()));
        let root = LocalStateProbe {
            count,
            items: items.clone(),
            attempted: Rc::new(Cell::new(true)),
            callbacks: Rc::new(Cell::new(0)),
            reads: reads.clone(),
        };
        let mut binding = HeadlessBinding::new();
        let wake_failure = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let wake_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut owners = MountOwners::fresh();
        let armed = wake_failure.clone();
        let calls = wake_calls.clone();
        owners.build_owner.set_on_build_scheduled(move || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert!(
                !armed.load(std::sync::atomic::Ordering::SeqCst),
                "frame request failed"
            );
        });
        binding.mount_root(&root, owners, MountOptions::tight(100.0, 100.0));
        binding.pump_frame(FRAME);
        let initial_wakes = wake_calls.load(std::sync::atomic::Ordering::SeqCst);
        wake_failure.store(competing_wake_failure, std::sync::atomic::Ordering::SeqCst);
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            items.update(|items| {
                items.push(5);
                panic!("local update failed after commit");
            });
        }))
        .expect_err("the original update failure is propagated");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"local update failed after commit")
        );
        assert_eq!(
            wake_calls.load(std::sync::atomic::Ordering::SeqCst),
            initial_wakes + 1,
            "the committed update attempts its frame wake even after failure"
        );
        wake_failure.store(false, std::sync::atomic::Ordering::SeqCst);
        binding.pump_frame(FRAME);
        assert_eq!(
            *reads.borrow(),
            [(0, 0), (0, 1)],
            "the committed edit remains deliverable after failure"
        );
        items.update(|items| items.push(6));
        binding.pump_frame(FRAME);
        assert_eq!(
            *reads.borrow(),
            [(0, 0), (0, 1), (0, 2)],
            "the next update remains usable"
        );
    }
}
