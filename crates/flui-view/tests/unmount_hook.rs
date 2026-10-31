//! `LifecycleContext::on_unmount`: hooks run once, right after `dispose`, in
//! registration order, each contained, at every unmount and never at a move.

use std::cell::RefCell;
use std::rc::Rc;

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_testing::widgets::{lay_out, tight};
use flui_view::{
    BuildContext, BuildOwner, ElementTree, IntoView, LifecycleContext, LifecycleHook, RenderView,
    StatefulView, UnmountHook, View, ViewExt, ViewState,
};

type Log = Rc<RefCell<Vec<&'static str>>>;

#[derive(Clone)]
struct Leaf;

impl RenderView for Leaf {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for Leaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// What the hooked state registers in `init_state`.
#[derive(Clone, Copy)]
enum Hooks {
    /// Two hooks, kept.
    Two,
    /// Two hooks, the first of which panics.
    FirstPanics,
    /// One hook whose token is dropped at once, one forgotten.
    DroppedAndForgotten,
}

#[derive(Clone)]
struct Hooked {
    log: Log,
    hooks: Hooks,
}

struct HookedState {
    log: Log,
    hooks: Hooks,
    /// The tokens of the hooks it keeps.
    kept: Vec<UnmountHook>,
}

impl StatefulView for Hooked {
    type State = HookedState;

    fn create_state(&self) -> Self::State {
        HookedState {
            log: Rc::clone(&self.log),
            hooks: self.hooks,
            kept: Vec::new(),
        }
    }
}

fn logging(log: &Log, entry: &'static str) -> Box<dyn FnOnce()> {
    let log = Rc::clone(log);
    Box::new(move || log.borrow_mut().push(entry))
}

impl ViewState<Hooked> for HookedState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        match self.hooks {
            Hooks::Two => {
                self.kept = vec![
                    cx.on_unmount(logging(&self.log, "first")),
                    cx.on_unmount(logging(&self.log, "second")),
                ];
            }
            Hooks::FirstPanics => {
                let log = Rc::clone(&self.log);
                self.kept = vec![
                    cx.on_unmount(Box::new(move || {
                        log.borrow_mut().push("first");
                        panic!("unmount hook failure");
                    })),
                    cx.on_unmount(logging(&self.log, "second")),
                ];
            }
            Hooks::DroppedAndForgotten => {
                drop(cx.on_unmount(logging(&self.log, "dropped")));
                cx.on_unmount(logging(&self.log, "forgotten")).forget();
            }
        }
    }

    fn build(&self, _view: &Hooked, _cx: &dyn BuildContext) -> impl IntoView {
        Leaf.boxed()
    }

    fn dispose(&mut self) {
        self.log.borrow_mut().push("dispose");
    }
}

impl View for Hooked {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// Mount `hooks` as a root and run its `init_state`.
fn mounted(hooks: Hooks) -> (ElementTree, BuildOwner, flui_foundation::ElementId, Log) {
    let log = Log::default();
    let view = Hooked {
        log: Rc::clone(&log),
        hooks,
    };
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root = tree.mount_root(&view, &mut owner.element_owner_mut());
    owner.schedule_build_for(root, 0, flui_view::RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    (tree, owner, root, log)
}

#[test]
fn hooks_run_once_after_dispose_in_registration_order() {
    let (mut tree, mut owner, root, log) = mounted(Hooks::Two);
    assert!(log.borrow().is_empty(), "nothing runs while mounted");
    let _removed = tree.remove(root, &mut owner.element_owner_mut());
    assert_eq!(*log.borrow(), ["dispose", "first", "second"]);
    assert!(owner.take_recovered_panics().is_empty());
}

#[test]
fn a_panicking_hook_leaves_the_next_one_running_and_is_the_dispose_failure() {
    let (mut tree, mut owner, root, log) = mounted(Hooks::FirstPanics);
    let _removed = tree.remove(root, &mut owner.element_owner_mut());
    assert_eq!(
        *log.borrow(),
        ["dispose", "first", "second"],
        "the second hook runs although the first panicked"
    );
    let recovered = owner.take_recovered_panics();
    assert_eq!(recovered.len(), 1, "one failure is recorded: {recovered:?}");
    assert_eq!(recovered[0].element(), Some(root));
    assert_eq!(recovered[0].hook, LifecycleHook::Dispose);
    assert_eq!(
        recovered[0].payload_text.as_deref(),
        Some("unmount hook failure")
    );
}

#[test]
fn a_dropped_token_unregisters_and_a_forgotten_one_runs() {
    let (mut tree, mut owner, root, log) = mounted(Hooks::DroppedAndForgotten);
    let _removed = tree.remove(root, &mut owner.element_owner_mut());
    assert_eq!(*log.borrow(), ["dispose", "forgotten"]);
}

#[test]
fn a_move_is_not_an_unmount() {
    let (mut tree, mut owner, root, log) = mounted(Hooks::Two);
    tree.deactivate(root, &mut owner.element_owner_mut());
    tree.activate(root, &mut owner.element_owner_mut());
    assert!(
        log.borrow().is_empty(),
        "deactivating and reactivating runs no hook"
    );
    let _removed = tree.remove(root, &mut owner.element_owner_mut());
    assert_eq!(*log.borrow(), ["dispose", "first", "second"]);
}

#[test]
fn a_tree_torn_down_runs_its_hooks() {
    let log = Log::default();
    let laid = lay_out(
        Hooked {
            log: Rc::clone(&log),
            hooks: Hooks::Two,
        },
        tight(10.0, 10.0),
    );
    assert!(log.borrow().is_empty());
    drop(laid);
    assert_eq!(*log.borrow(), ["dispose", "first", "second"]);
}
