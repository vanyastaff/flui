//! A `StatefulRenderView` mounts one element that holds a state and one
//! render object (ADR-0322 §4).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_objects::{RenderOpacity, RenderSizedBox};
use flui_rendering::RenderUpdateImpact;
use flui_rendering::protocol::BoxProtocol;
use flui_testing::widgets::harness::{Harness, mount};
use flui_view::{
    BuildContext, BuildContextExt, BuildOwner, ElementTree, IntoView, LifecycleContext,
    LifecycleHook, Provided, ProvidedValue, RebuildHandle, RebuildReason, RenderObjectContext,
    RenderView, RenderViewState, StatefulRenderView, StatelessView, View,
};

type Log = Rc<RefCell<Vec<&'static str>>>;

/// What the test reaches into the mounted state through.
#[derive(Clone, Default)]
struct Wires {
    log: Log,
    /// The state's rebuild handle, taken in `init_state`.
    rebuild: Rc<RefCell<Option<RebuildHandle>>>,
    /// A factor the state multiplies the view's opacity by.
    factor: Rc<Cell<f64>>,
    /// What the child read from the value the state publishes.
    seen: Rc<Cell<Option<u32>>>,
}

impl Wires {
    fn new() -> Self {
        let wires = Self::default();
        wires.factor.set(1.0);
        wires
    }

    fn take_log(&self) -> Vec<&'static str> {
        std::mem::take(&mut *self.log.borrow_mut())
    }

    fn rebuild(&self) {
        self.rebuild
            .borrow()
            .as_ref()
            .expect("init_state stored the rebuild handle")
            .schedule(RebuildReason::StateChange);
    }
}

/// Which hook of the state is authored to fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Fail {
    #[default]
    Nothing,
    InitState,
    Update,
    Dispose,
}

#[derive(Clone, Debug, PartialEq)]
struct Tint(u32);

impl ProvidedValue for Tint {
    fn update_should_notify(&self, old: &Self) -> bool {
        self != old
    }
}

/// A fade whose state scales the view's opacity.
#[derive(Clone)]
struct Fade {
    opacity: f64,
    tint: u32,
    fail: Fail,
    wires: Wires,
}

impl Fade {
    fn new(opacity: f64, wires: &Wires) -> Self {
        Self {
            opacity,
            tint: 7,
            fail: Fail::Nothing,
            wires: wires.clone(),
        }
    }

    fn failing(mut self, fail: Fail) -> Self {
        self.fail = fail;
        self
    }
}

struct FadeState {
    wires: Wires,
    fail: Fail,
}

impl StatefulRenderView for Fade {
    type Protocol = BoxProtocol;
    type RenderObject = RenderOpacity;
    type State = FadeState;

    fn create_state(&self, _ctx: &dyn flui_view::LifecycleContext) -> Self::State {
        FadeState {
            wires: self.wires.clone(),
            fail: self.fail,
        }
    }

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        self.wires.log.borrow_mut().push("create");
        RenderOpacity::new(self.opacity)
    }

    fn did_unmount_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) {
        self.wires.log.borrow_mut().push("release");
    }
}

impl RenderViewState<Fade> for FadeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.wires.log.borrow_mut().push("init");
        assert!(self.fail != Fail::InitState, "init_state failure");
        *self.wires.rebuild.borrow_mut() = Some(ctx.rebuild_handle());
    }

    fn did_update_view(&mut self, _old: &Fade, new: &Fade) {
        self.wires.log.borrow_mut().push("did_update");
        self.fail = new.fail;
    }

    fn update_render_object(
        &self,
        view: &Fade,
        _ctx: &RenderObjectContext<'_>,
        render_object: &mut RenderOpacity,
    ) -> RenderUpdateImpact {
        self.wires.log.borrow_mut().push("update");
        assert!(self.fail != Fail::Update, "update failure");
        render_object.set_opacity(view.opacity * self.wires.factor.get())
    }

    fn provide(&self, view: &Fade, _ctx: &dyn BuildContext, values: &mut Provided<'_>) {
        self.wires.log.borrow_mut().push("provide");
        values.value(Tint(view.tint));
    }

    fn child(&self, _view: &Fade, _ctx: &dyn BuildContext) -> impl IntoView {
        self.wires.log.borrow_mut().push("child");
        Reader(self.wires.clone())
    }

    fn dispose(&mut self) {
        self.wires.log.borrow_mut().push("dispose");
        assert!(self.fail != Fail::Dispose, "dispose failure");
    }
}

flui_view::impl_stateful_render_view!(Fade);

/// Reads the tint published above it.
#[derive(Clone)]
struct Reader(Wires);

impl StatelessView for Reader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.0.seen.set(ctx.depend_on::<Tint, _>(|tint| tint.0));
        Leaf
    }
}

impl View for Reader {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

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

flui_view::impl_render_view!(Leaf);

/// The opacity of every `RenderOpacity` in the render tree.
fn opacities(harness: &Harness) -> Vec<f64> {
    harness.pipeline_owner().with(|owner| {
        owner
            .render_tree()
            .iter()
            .filter_map(|(_, node)| node.downcast_render_object::<RenderOpacity>())
            .map(RenderOpacity::opacity)
            .collect::<Vec<f64>>()
    })
}

#[test]
fn a_stateful_render_view_is_one_element_with_its_state_and_render_object() {
    let wires = Wires::new();
    let mut harness = mount(Fade::new(0.5, &wires));

    assert_eq!(
        wires.take_log(),
        ["create", "init", "update", "provide", "child"],
        "the render object exists at mount; the state initializes at the first build"
    );
    assert_eq!(opacities(&harness), [0.5]);
    assert_eq!(
        wires.seen.get(),
        Some(7),
        "the child mounts under `provide`"
    );

    let root = harness.root();
    let children = harness.children_of(root);
    assert_eq!(children.len(), 1);
    assert_eq!(
        harness.view_type_of(children[0]),
        std::any::TypeId::of::<Reader>(),
        "no element sits between the view and the child it builds"
    );
}

#[test]
fn a_scheduled_state_change_updates_the_render_object() {
    let wires = Wires::new();
    let mut harness = mount(Fade::new(0.5, &wires));
    wires.take_log();

    harness.tick();
    assert!(wires.take_log().is_empty(), "a clean element runs no hook");

    wires.factor.set(0.5);
    wires.rebuild();
    harness.tick();
    assert_eq!(wires.take_log(), ["update", "provide", "child"]);
    assert_eq!(opacities(&harness), [0.25]);
}

#[test]
fn a_new_view_reaches_the_state_before_the_render_object() {
    let wires = Wires::new();
    let mut harness = mount(Fade::new(0.5, &wires));
    wires.take_log();

    let mut next = Fade::new(0.8, &wires);
    next.tint = 9;
    harness.swap_root(next);
    assert_eq!(
        wires.take_log(),
        ["did_update", "update", "provide", "child"]
    );
    assert_eq!(
        opacities(&harness),
        [0.8],
        "the same render object, updated"
    );
    assert_eq!(
        wires.seen.get(),
        Some(9),
        "the reader follows the new value"
    );
}

#[test]
fn unmount_disposes_the_state_and_removes_the_render_object() {
    let wires = Wires::new();
    let mut harness = mount(Fade::new(0.5, &wires));
    wires.take_log();

    harness.swap_root(Leaf);
    assert_eq!(
        wires.take_log(),
        ["dispose", "release"],
        "the view releases the render object after the state is disposed"
    );
    assert_eq!(opacities(&harness), [] as [f64; 0]);
}

#[test]
fn a_dispose_panic_still_removes_the_render_object() {
    let wires = Wires::new();
    let mut harness = mount(Fade::new(0.5, &wires).failing(Fail::Dispose));
    wires.take_log();

    harness.swap_root(Leaf);
    assert_eq!(
        wires.take_log(),
        ["dispose", "release"],
        "the view releases the render object after the state is disposed"
    );
    assert_eq!(opacities(&harness), [] as [f64; 0]);
}

#[test]
fn a_state_that_failed_to_initialize_is_not_disposed() {
    let wires = Wires::new();
    let harness = mount(Fade::new(0.5, &wires).failing(Fail::InitState));

    assert_eq!(
        wires.take_log(),
        ["create", "init", "release"],
        "nothing of the state runs after its init_state failed; the render object is released"
    );
    assert!(
        opacities(&harness).is_empty(),
        "the element is replaced, and its render object goes with it"
    );
}

#[test]
fn an_update_panic_replaces_the_element_and_the_next_frame_runs() {
    let wires = Wires::new();
    let mut harness = mount(Fade::new(0.5, &wires));
    wires.take_log();

    harness.swap_root(Fade::new(0.8, &wires).failing(Fail::Update));
    assert_eq!(
        wires.take_log(),
        ["did_update", "update", "dispose", "release"],
        "the failed update stops the build; the initialized state is disposed"
    );
    assert_eq!(opacities(&harness), [] as [f64; 0]);

    harness.swap_root(Fade::new(0.3, &wires));
    assert_eq!(
        wires.take_log(),
        ["create", "init", "update", "provide", "child"]
    );
    assert_eq!(opacities(&harness), [0.3]);
}

/// A tree with no pipeline, so the build owner's own record of a contained
/// failure can be read: the host of a `Harness` consumes it as a frame
/// failure.
#[test]
fn a_state_hook_failure_is_recorded_against_its_hook() {
    for (fail, hook) in [
        (Fail::InitState, LifecycleHook::InitState),
        (Fail::Dispose, LifecycleHook::Dispose),
    ] {
        let wires = Wires::new();
        let view = Under(Fade::new(0.5, &wires).failing(fail));
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let root = tree.mount_root(&view, &mut owner.element_owner_mut());
        owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
        owner.build_scope(&mut tree);
        if fail == Fail::Dispose {
            let fade = tree
                .iter_nodes()
                .find(|(_, node)| node.parent() == Some(root))
                .map(|(id, _)| id)
                .expect("the fade is mounted under its parent");
            let _removed = tree.remove(fade, &mut owner.element_owner_mut());
        }
        let recovered = owner.take_recovered_panics();
        assert_eq!(recovered.len(), 1, "{fail:?}: {recovered:?}");
        assert_eq!(recovered[0].hook, hook);
    }
}

/// A parent for the fade, so a failed first build has an element to be
/// replaced under.
#[derive(Clone)]
struct Under(Fade);

impl StatelessView for Under {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.0.clone()
    }
}

impl View for Under {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}
