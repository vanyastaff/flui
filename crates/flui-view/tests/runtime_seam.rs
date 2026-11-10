//! The composition-root seam (`flui_view::__runtime`, ADR-0081 §4) driven
//! from outside the crate, the way `flui-runtime`, `flui-testing` and
//! `flui-hot-reload` use it: always compiled, no cargo feature involved.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use flui_foundation::ElementId;
use flui_objects::RenderSizedBox;
use flui_rendering::{RenderUpdateImpact, protocol::BoxProtocol};
use flui_view::{
    __runtime::{BindingRuntime as _, FramePhaseMarker, GlobalKeyRegistryComposite},
    AppLifecycleState, BuildContext, GlobalKey, IntoView, KeyLookup, LifecycleClosed,
    RenderObjectContext, RenderView, StatelessView, View, WidgetsBinding,
    element::ElementKind,
};

#[test]
fn with_global_key_registry_resolves_this_bindings_keys_only_while_entered() {
    let binding = WidgetsBinding::new();
    let key = GlobalKey::<()>::new();
    let id = ElementId::new(7);
    binding.with_build_owner_mut(|owner| owner.register_global_key(&key, id));

    assert_eq!(key.current_element(), KeyLookup::NoActiveRuntime);
    assert_eq!(
        binding.with_global_key_registry(|| key.current_element()),
        KeyLookup::Mounted(id)
    );
    assert_eq!(key.current_element(), KeyLookup::NoActiveRuntime);
}

#[test]
fn composite_resolves_a_key_held_by_any_member() {
    let first = WidgetsBinding::new();
    let second = WidgetsBinding::new();
    let key = GlobalKey::<()>::new();
    let id = ElementId::new(11);
    second.with_build_owner_mut(|owner| owner.register_global_key(&key, id));

    let composite = GlobalKeyRegistryComposite::assemble([&first, &second]);
    assert_eq!(
        composite.enter(|| key.current_element()),
        KeyLookup::Mounted(id)
    );
    assert_eq!(key.current_element(), KeyLookup::NoActiveRuntime);
}

#[test]
fn draw_frame_with_phase_marker_stamps_the_finalize_phase() {
    let binding = WidgetsBinding::new();
    let marker = FramePhaseMarker::new(0_u8);
    marker.set(1);
    assert_eq!(marker.get(), 1);

    binding.draw_frame_with_phase_marker(&marker, 2);
    assert_eq!(marker.get(), 2);
}

#[test]
fn terminal_lifecycle_ladder_runs_through_the_seam() {
    use AppLifecycleState::{Detached, Resumed};

    let binding = WidgetsBinding::new();
    binding.handle_app_lifecycle_state_changed(Resumed);
    let handle = binding.lifecycle_source().handle();
    let seen = Rc::new(Cell::new(None));
    let log = Rc::clone(&seen);
    let (snapshot, _subscription) = handle
        .subscribe(move |state| log.set(Some(state)))
        .expect("an open lifecycle accepts subscriptions");
    assert_eq!(snapshot, Some(Resumed));

    binding.lifecycle_source().begin_close();
    // An ordinary commit is fenced once terminal close begins.
    assert_eq!(
        binding.lifecycle_source().commit(Resumed),
        Err(LifecycleClosed)
    );
    binding
        .lifecycle_source()
        .commit_terminal(Detached)
        .expect("terminal commit after begin_close");
    assert_eq!(binding.lifecycle_source().current(), Some(Detached));
    binding.notify_committed_lifecycle(Detached);
    assert_eq!(seen.get(), Some(Detached));

    binding.lifecycle_source().finish_close();
    assert_eq!(handle.snapshot(), Err(LifecycleClosed));
}

/// The frame-request callback is user code run with no borrow of its cell held: a callback that
/// installs its own replacement takes effect, and the replaced callback stays alive until it
/// returns.
///
/// Fails without the clone-out in `handle_build_scheduled`: `set_on_need_frame` then panics with
/// `BorrowMutError`, since the cell is still borrowed for the call.
#[test]
fn an_on_need_frame_callback_may_replace_itself() {
    let binding = Rc::new(WidgetsBinding::new());
    let first = Rc::new(Cell::new(0_u32));
    let second = Rc::new(Cell::new(0_u32));
    binding.set_on_need_frame({
        let binding = Rc::downgrade(&binding);
        let first = Rc::clone(&first);
        let second = Rc::clone(&second);
        move || {
            first.set(first.get() + 1);
            let second = Rc::clone(&second);
            binding
                .upgrade()
                .expect("the binding outlives its callback")
                .set_on_need_frame(move || second.set(second.get() + 1));
        }
    });

    binding.handle_build_scheduled();
    binding.handle_build_scheduled();

    assert_eq!(
        (first.get(), second.get()),
        (1, 1),
        "the first request runs the original callback, the second its replacement"
    );
}

/// Reads a key from its own `build` and records what the lookup reported.
#[derive(Clone)]
struct LookupInBuild {
    key: GlobalKey<()>,
    seen: Rc<RefCell<Vec<KeyLookup>>>,
}

impl StatelessView for LookupInBuild {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.seen.borrow_mut().push(self.key.current_element());
        LookupLeaf
    }
}

impl View for LookupInBuild {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

#[derive(Clone)]
struct LookupLeaf;

impl RenderView for LookupLeaf {
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

impl View for LookupLeaf {
    fn create_element(&self) -> ElementKind {
        ElementKind::render_variable(self)
    }
}

#[test]
fn a_miss_names_why_it_missed() {
    let binding = WidgetsBinding::new();
    let key = GlobalKey::<()>::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    binding
        .attach_root_widget(&LookupInBuild {
            key: key.clone(),
            seen: Rc::clone(&seen),
        })
        .expect("attach succeeds");

    // Outside any runtime scope no registry is consulted.
    assert_eq!(key.current_element(), KeyLookup::NoActiveRuntime);
    // Inside the scope the registry is consulted and holds nothing for the key.
    assert_eq!(
        binding.with_global_key_registry(|| key.current_element()),
        KeyLookup::NotMounted
    );
    // From `build`, the binding's own frame holds its tree.
    binding.with_global_key_registry(|| binding.draw_frame());
    assert_eq!(seen.take(), vec![KeyLookup::RegistryBusy]);
}
