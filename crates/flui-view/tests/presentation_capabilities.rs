//! The presentation capabilities a UI runtime installs on its owner reach
//! `LifecycleContext` as installed, and a bare owner reports every one of
//! them absent.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_testing::HeadlessBinding;
use flui_testing::bootstrap::{MountOptions, MountOwners};
use flui_testing::storage::MemoryStorage;
use flui_view::PresentationCapabilities;
use flui_view::prelude::*;

/// What `init_state` found, one flag per capability.
#[derive(Debug, Default, PartialEq, Eq)]
struct Present {
    text_input: bool,
    clipboard: bool,
    haptics: bool,
    drag_and_drop: bool,
    textures: bool,
    storage: bool,
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

impl View for Leaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

#[derive(Clone, StatefulView)]
struct Asker {
    found: Rc<RefCell<Option<Present>>>,
}

struct AskerState {
    found: Rc<RefCell<Option<Present>>>,
}

impl StatefulView for Asker {
    type State = AskerState;
    fn create_state(&self) -> Self::State {
        AskerState {
            found: Rc::clone(&self.found),
        }
    }
}

impl ViewState<Asker> for AskerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.found.borrow_mut() = Some(Present {
            text_input: ctx.text_input_handle().is_some(),
            clipboard: ctx
                .capability::<flui_interaction::ClipboardCapability>()
                .is_ok(),
            haptics: ctx.haptics().is_some(),
            drag_and_drop: ctx.drag_and_drop().is_some(),
            textures: ctx.texture_registry().is_some(),
            storage: ctx.storage().is_some(),
        });
    }

    fn build(&self, _view: &Asker, _ctx: &dyn BuildContext) -> impl IntoView {
        Leaf
    }
}

fn found_after(install: impl FnOnce(&mut PresentationCapabilities)) -> Present {
    let found = Rc::new(RefCell::new(None));
    let mut owners = MountOwners::fresh();
    install(owners.build_owner.capabilities_mut());
    let mut binding = HeadlessBinding::new();
    binding.mount_root(
        &Asker {
            found: Rc::clone(&found),
        },
        owners,
        MountOptions::tight(100.0, 100.0),
    );
    binding.pump_frame(Duration::from_millis(16));
    found.borrow_mut().take().expect("init_state ran")
}

#[test]
fn a_bare_owner_reports_every_capability_absent() {
    assert_eq!(found_after(|_| {}), Present::default());
}

#[test]
fn an_installed_capability_reaches_the_lifecycle_context_alone() {
    let found = found_after(|capabilities| {
        capabilities.storage = Some(Arc::new(MemoryStorage::default()));
    });
    assert_eq!(
        found,
        Present {
            storage: true,
            ..Present::default()
        }
    );
}
