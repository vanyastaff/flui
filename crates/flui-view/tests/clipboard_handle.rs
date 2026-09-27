//! `LifecycleContext::clipboard_handle` reaches `init_state` with the handle
//! the owner was given, and is absent on a bare owner.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use flui_interaction::ClipboardHandle;
use flui_objects::RenderSizedBox;
use flui_platform_api::{Clipboard, InMemoryClipboard};
use flui_rendering::{
    pipeline::{PipelineCell, PipelineOwner},
    protocol::BoxProtocol,
};
use flui_view::{
    BuildContext, BuildOwner, ElementTree, IntoView, LifecycleContext, RebuildReason, RenderView,
    RootRenderView, StatefulView, View, ViewState,
};

type Seen = Rc<RefCell<Option<Option<ClipboardHandle>>>>;

#[derive(Clone)]
struct Probe {
    seen: Seen,
}

struct ProbeState {
    seen: Seen,
}

impl StatefulView for Probe {
    type State = ProbeState;

    fn create_state(&self) -> Self::State {
        ProbeState {
            seen: Rc::clone(&self.seen),
        }
    }
}

impl ViewState<Probe> for ProbeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.seen.borrow_mut() = Some(ctx.clipboard_handle());
    }

    fn build(&self, _view: &Probe, _ctx: &dyn BuildContext) -> impl IntoView {
        Leaf
    }
}

impl View for Probe {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

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

/// Mount a `Probe` under `owner` and return what its `init_state` saw.
fn seen_by_init_state(mut owner: BuildOwner) -> Option<ClipboardHandle> {
    let seen: Seen = Rc::new(RefCell::new(None));
    let mut tree = ElementTree::new();
    let root = RootRenderView::new(
        Probe {
            seen: Rc::clone(&seen),
        },
        800.0,
        600.0,
    );
    let root_id = tree.mount_root_with_pipeline_owner(
        &root,
        Some(PipelineCell::new(PipelineOwner::new())),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(root_id, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    seen.borrow_mut()
        .take()
        .expect("init_state must have run during the mount build")
}

#[test]
fn clipboard_handle_reaches_init_state_and_is_none_on_a_bare_owner() {
    assert!(
        seen_by_init_state(BuildOwner::new()).is_none(),
        "a bare owner has no clipboard"
    );

    let backend = Arc::new(InMemoryClipboard::new());
    let mut owner = BuildOwner::new();
    owner.set_clipboard_handle(ClipboardHandle::new(backend.clone()));
    let handle = seen_by_init_state(owner).expect("the installed handle reaches init_state");

    handle.write_text("from init_state");
    assert_eq!(
        backend.read_text().as_deref(),
        Some("from init_state"),
        "the handle writes through to the clipboard the owner was given"
    );
}
