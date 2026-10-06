//! The flush registry: the host-owned slots a document publishes its latest
//! bytes into, written by the host's IO and flushed when the application
//! ends.

use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_objects::RenderSizedBox;
use flui_platform_api::{StorageName, WriteMode};
use flui_rendering::protocol::BoxProtocol;
use flui_testing::storage::MemoryStorage;
use flui_testing::widgets::{lay_out, tight};
use flui_view::__runtime::{FlushRegistry, FlushRegistryHost as _};
use flui_view::prelude::*;
use flui_view::{CloseReason, LifecycleSubscription};

const SESSION: StorageName = StorageName::machine_local("session");

/// A leaf that renders nothing, so the build chain bottoms out.
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

/// Publishes its session into the registry when its presentation is told it
/// is detached, as an application saving on close does.
#[derive(Clone, StatefulView)]
struct SavesOnDetach {
    registry: FlushRegistry,
}

struct SavesOnDetachState {
    registry: FlushRegistry,
    subscription: Option<LifecycleSubscription>,
}

impl StatefulView for SavesOnDetach {
    type State = SavesOnDetachState;

    fn create_state(&self) -> Self::State {
        SavesOnDetachState {
            registry: self.registry.clone(),
            subscription: None,
        }
    }
}

impl ViewState<SavesOnDetach> for SavesOnDetachState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        let registry = self.registry.clone();
        let lifecycle = cx.lifecycle_handle().expect("a realm presentation has one");
        let (_, subscription) = lifecycle
            .subscribe(move |state| {
                if state == AppLifecycleState::Detached {
                    registry.publish(SESSION, Arc::from(&b"closing"[..]), WriteMode::Replace);
                }
            })
            .expect("the presentation is open");
        self.subscription = Some(subscription);
    }

    fn build(&self, _view: &SavesOnDetach, _cx: &dyn BuildContext) -> impl IntoView {
        Leaf
    }
}

/// Bytes published while the presentation closes are on the storage once
/// the host's teardown flush returns, with no IO pool to write them before.
#[test]
#[ignore = "contract: the teardown flush writes bytes published at Detached"]
fn a_registry_entry_published_at_detached_is_written_at_teardown() {
    let storage = MemoryStorage::new();
    let registry = FlushRegistry::new(
        Arc::new(storage.clone()),
        // No IO pool: every write is left to the teardown flush.
        Box::new(Err),
    );
    let laid = lay_out(
        SavesOnDetach {
            registry: registry.clone(),
        },
        tight(10.0, 10.0),
    );

    laid.request_close(CloseReason::User);
    drop(laid);
    let report = registry.flush_within(Instant::now() + Duration::from_secs(5));

    assert_eq!(
        storage.contents(&SESSION).as_deref(),
        Some(&b"closing"[..]),
        "the teardown flush writes what was published at Detached"
    );
    assert!(
        report.is_complete(),
        "nothing is left unwritten: {report:?}"
    );
}
