//! Signal reads through the `ReadScope` contract (ADR-0085 §2).
//!
//! - every context shape a view author writes accepts `sig.get(cx)`;
//! - a read in `build` subscribes the building element through the context
//!   production builds with (`BuildCtx`, reached by a real mount), and a write
//!   rebuilds exactly that element;
//! - a handle of the wrong type is a typed error on every path, never a panic.
//!
//! The production-context test also runs in release (`cargo xtask test`), so
//! the half of the build path gated on `debug_assertions` is covered too.

// ADR-0027: ElementBuildContext's test seam takes Arc<RwLock<…>> over a !Send
// owner graph; do not restore Send + Sync to satisfy clippy.
#![expect(clippy::arc_with_non_send_sync)]

use std::any::type_name;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_view::prelude::*;
use flui_view::{
    ElementBuildContext, Reactive, ReadScope, ScopeRef, Signal, SignalError, SignalSender,
};
use parking_lot::RwLock;

static_assertions::assert_not_impl_any!(Signal<u32>: Send, Sync);
static_assertions::assert_impl_all!(SignalSender<u32>: Send, Sync);

// ============================================================================
// Every context shape reads
// ============================================================================

fn through_dyn(sig: Signal<u32>, cx: &dyn BuildContext) -> u32 {
    sig.get(cx)
}

fn through_dyn_ref(sig: Signal<u32>, cx: &&dyn BuildContext) -> u32 {
    sig.get(cx)
}

#[expect(
    clippy::borrowed_box,
    reason = "the shape under test: a boxed context passed by reference"
)]
fn through_box(sig: Signal<u32>, cx: &Box<dyn BuildContext>) -> u32 {
    sig.get(cx)
}

fn through_generic_unsized<C: BuildContext + ?Sized>(sig: Signal<u32>, cx: &C) -> u32 {
    sig.get(cx)
}

fn through_generic_sized<C: BuildContext>(sig: Signal<u32>, cx: &C) -> u32 {
    sig.get(cx)
}

fn through_read_scope(sig: Signal<u32>, cx: &dyn ReadScope) -> u32 {
    sig.get(cx)
}

fn through_lifecycle(sig: Signal<u32>, cx: &dyn LifecycleContext) -> u32 {
    sig.get(cx)
}

fn context() -> ElementBuildContext {
    let tree = Arc::new(RwLock::new(ElementTree::new()));
    let owner = Arc::new(RwLock::new(BuildOwner::new()));
    ElementBuildContext::new(ElementId::new(1), 0, true, tree, owner)
}

#[test]
fn signal_reads_accept_every_context_shape() {
    let cx = context();
    let sig = cx.reactive().signal(3u32);
    let as_dyn: &dyn BuildContext = &cx;
    let boxed: Box<dyn BuildContext> = Box::new(ElementBuildContext::new(
        ElementId::new(1),
        0,
        true,
        Arc::clone(cx.tree()),
        Arc::clone(cx.build_owner()),
    ));
    let closure = |c: &dyn BuildContext| sig.get(c);

    assert_eq!(through_dyn(sig, as_dyn), 3);
    assert_eq!(through_dyn_ref(sig, &as_dyn), 3);
    assert_eq!(through_box(sig, &boxed), 3);
    assert_eq!(through_generic_unsized(sig, as_dyn), 3);
    assert_eq!(through_generic_unsized(sig, &cx), 3);
    assert_eq!(through_generic_sized(sig, &cx), 3);
    assert_eq!(through_read_scope(sig, &cx), 3);
    assert_eq!(through_read_scope(sig, as_dyn), 3);
    assert_eq!(through_lifecycle(sig, &cx), 3);
    assert_eq!(closure(as_dyn), 3);
    assert_eq!(sig.with(as_dyn, |v| v + 1), 4);
    assert_eq!(sig.try_get(as_dyn), Ok(3));
}

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

#[test]
fn a_read_in_build_subscribes_through_the_production_context() {
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

// ============================================================================
// A handle of the wrong type
// ============================================================================

/// A context over a bare graph whose reads subscribe nobody.
struct Probe<'a>(&'a Reactive);

impl ReadScope for Probe<'_> {
    fn scope(&self) -> ScopeRef<'_> {
        ScopeRef::new(self.0, None)
    }
}

#[test]
fn a_signal_handle_of_the_wrong_type_is_a_typed_error() {
    let graph = Reactive::new();
    let n = graph.signal(7u32);
    let wrong = Signal::<String>::from_slot(n.slot());
    let expected = SignalError::TypeMismatch {
        index: n.slot().index(),
        expected: type_name::<String>(),
    };

    assert_eq!(wrong.try_get(&Probe(&graph)), Err(expected));
    assert_eq!(wrong.peek(&graph, String::len), Err(expected));
    assert_eq!(wrong.set(&graph, String::new()), Err(expected));

    assert_eq!(n.peek(&graph, |v| *v), Ok(7), "the slot is untouched");
    assert!(graph.readers_of(n.slot()).is_empty());
}
