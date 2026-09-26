use std::cell::{Cell, RefCell};

use super::*;

/// Captures its own `RebuildHandle` in `init_state` (capabilities are
/// only reachable from lifecycle hooks) and schedules through
/// it from `dispose`, exactly the shape a real widget's cleanup path
/// takes (e.g. cancelling a subscription and requesting one final
/// rebuild to reflect that).
#[derive(Clone)]
struct DisposeProbeView {
    handle_slot: Rc<RefCell<Option<flui_view::RebuildHandle>>>,
    disposed: Rc<Cell<bool>>,
}

struct DisposeProbeState {
    handle_slot: Rc<RefCell<Option<flui_view::RebuildHandle>>>,
    disposed: Rc<Cell<bool>>,
    handle: Option<flui_view::RebuildHandle>,
}

impl StatefulView for DisposeProbeView {
    type State = DisposeProbeState;

    fn create_state(&self) -> Self::State {
        DisposeProbeState {
            handle_slot: Rc::clone(&self.handle_slot),
            disposed: Rc::clone(&self.disposed),
            handle: None,
        }
    }
}

impl ViewState<DisposeProbeView> for DisposeProbeState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        let handle = ctx.rebuild_handle();
        let _prev = self.handle_slot.borrow_mut().replace(handle.clone());
        self.handle = Some(handle);
    }

    fn build(&self, _view: &DisposeProbeView, _ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::new(0.0, 0.0)
    }

    fn dispose(&mut self) {
        // Capabilities installed at init_state are still valid here
        // (nothing about this presentation's realm-shared dispatch
        // handles has been touched by teardown step 3 alone) —
        // schedule through this element's OWN handle, exactly a real
        // cleanup hook would.
        if let Some(handle) = &self.handle {
            handle.schedule(flui_foundation::RebuildReason::StateChange);
        }
        self.disposed.set(true);
    }
}

impl flui_view::View for DisposeProbeView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// A `State::dispose()` hook running as part of `UiRealm::
/// close_presentation_entered`'s teardown (step 3) schedules through
/// its OWN `RebuildHandle` — proving it reaches only its own (about
/// to be dropped) tree, never a live sibling's, by the same
/// `ExternalBuildScheduler`-is-minted-one-per-owner argument
/// `close_presentation_entered`'s own doc makes. "Dead services" — this
/// presentation's OWN focus/IME, already closed by teardown step 2
/// before step 3's detach runs — failing closed rather than
/// panicking is covered by `presentation.rs`'s
/// `text_input_handle_is_bound_to_the_owned_text_input_state`; this
/// test does not re-derive that, only the sibling-reach half.
#[test]
fn dispose_during_teardown_cannot_reach_sibling_or_dead_services() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();

    let handle_slot = Rc::new(RefCell::new(None));
    let disposed = Rc::new(Cell::new(false));
    let probe = DisposeProbeView {
        handle_slot: Rc::clone(&handle_slot),
        disposed: Rc::clone(&disposed),
    };
    // Through the normal `UiRealm::attach_root_widget` auto-wrap
    // (`FocusRoot`/`VsyncScope`/`GestureArenaScope`), so this probe
    // is a DESCENDANT of the mounted root, not the root itself —
    // exercising `WidgetsBinding::detach_root_widget`'s cascading
    // `remove_subtree` teardown (not just a single-node `remove`),
    // the same depth a real widget tree has.
    realm
        .enter(|realm| realm.attach_root_widget(&probe))
        .expect("A mounts the dispose probe");
    let _ = realm.enter(|realm| {
        realm.draw_frame_entered(BoxConstraints::tight(flui_types::Size::new(
            px(20.0),
            px(20.0),
        )))
    });
    assert!(
        handle_slot.borrow().is_some(),
        "init_state must have captured the rebuild handle"
    );

    fn b_has_pending_builds(realm: &UiRealm, b_id: PresentationId) -> bool {
        realm
            .presentations
            .get(b_id)
            .expect("B still installed")
            .widgets()
            .has_pending_builds()
    }
    assert!(
        !b_has_pending_builds(&realm, b_id),
        "precondition: B has no pending build before A tears down"
    );

    assert!(
        realm.close_presentation_entered(a_id),
        "A must have been installed and removable"
    );

    assert!(
        disposed.get(),
        "close_presentation_entered's detach_root_widget must have run A's dispose hook"
    );
    assert!(
        !b_has_pending_builds(&realm, b_id),
        "A's dispose-time schedule() must not have reached B's build inbox"
    );
}
