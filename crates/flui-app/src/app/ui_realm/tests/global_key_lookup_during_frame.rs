//! A `GlobalKey` read from inside a realm frame returns instead of blocking.
//!
//! A presentation's `WidgetsBinding` holds its own frame lock for the whole
//! build. A key read inside that frame (the shape of `DrawerHandle::open_drawer`
//! called from a build) resolves that presentation to nothing, and still
//! resolves keys held by every other presentation of the realm.

use std::cell::{Cell, RefCell};

use super::*;

/// Run `f` on a fresh thread and fail the test if it has not returned within
/// ten seconds. The realm is `!Send`, so `f` builds it on that thread; a
/// lookup that re-enters a binding's frame lock parks the thread forever and
/// the timeout turns that into a failure.
fn within_deadline<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .expect("deadlock: GlobalKey lookup re-entered the binding's frame lock")
}

fn frame_constraints() -> BoxConstraints {
    BoxConstraints::tight(flui_types::Size::new(px(20.0), px(20.0)))
}

/// A stateful toggle, the shape of the Material drawer controller: its
/// state is mutated through `&self` from outside, by `GlobalKey`.
#[derive(Clone)]
struct Toggle {
    key: flui_view::GlobalKey<ToggleState>,
    initially_open: bool,
    child: Option<ToggleReader>,
}

struct ToggleState {
    open: Cell<bool>,
}

impl ToggleState {
    fn open(&self) {
        self.open.set(true);
    }

    fn is_open(&self) -> bool {
        self.open.get()
    }
}

impl flui_view::StatefulView for Toggle {
    type State = ToggleState;

    fn create_state(&self) -> Self::State {
        ToggleState {
            open: Cell::new(self.initially_open),
        }
    }
}

impl flui_view::ViewState<Toggle> for ToggleState {
    fn build(&self, view: &Toggle, _ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        match &view.child {
            Some(reader) => flui_view::ViewExt::boxed(reader.clone()),
            None => flui_view::ViewExt::boxed(SizedBox::new(0.0, 0.0)),
        }
    }
}

impl flui_view::View for Toggle {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }

    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.key)
    }
}

/// Reads the toggle through its key from `build`, recording what it saw.
#[derive(Clone)]
struct ToggleReader {
    key: flui_view::GlobalKey<ToggleState>,
    seen: Rc<RefCell<Vec<Option<bool>>>>,
}

impl flui_view::StatelessView for ToggleReader {
    fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl IntoView {
        let _ = self.key.with_current_state(ToggleState::open);
        self.seen
            .borrow_mut()
            .push(self.key.with_current_state(ToggleState::is_open));
        SizedBox::new(0.0, 0.0)
    }
}

impl flui_view::View for ToggleReader {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

#[test]
fn drawer_style_state_read_during_the_realm_frame_does_not_deadlock() {
    let (during, after) = within_deadline(|| {
        let realm = UiRealm::for_test();
        let key = flui_view::GlobalKey::<ToggleState>::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        realm
            .enter(|realm| {
                realm.attach_root_widget(&Toggle {
                    key: key.clone(),
                    initially_open: false,
                    child: Some(ToggleReader {
                        key: key.clone(),
                        seen: Rc::clone(&seen),
                    }),
                })
            })
            .expect("the toggle mounts");
        let _ = realm.enter(|realm| realm.draw_frame_entered(frame_constraints()));
        let after = realm.enter(|_| key.with_current_state(ToggleState::is_open));
        (seen.take(), after)
    });
    assert_eq!(
        during,
        vec![None],
        "the read inside the presentation's own frame resolves to nothing"
    );
    assert_eq!(
        after,
        Some(false),
        "the in-frame open was a no-op, and the key resolves after the frame"
    );
}

#[test]
fn state_read_across_presentations_during_a_segment_resolves() {
    let during = within_deadline(|| {
        let mut realm = UiRealm::for_test();
        let holder_id = realm.install_second_presentation_for_test();
        let key = flui_view::GlobalKey::<ToggleState>::new();
        let seen = Rc::new(RefCell::new(Vec::new()));

        // The second presentation holds the key and builds it first.
        realm
            .attach_root_widget_to_for_test(
                holder_id,
                &Toggle {
                    key: key.clone(),
                    initially_open: true,
                    child: None,
                },
            )
            .expect("the holder mounts");
        let _ = realm.enter(|realm| realm.draw_frame_entered(frame_constraints()));

        // The primary presentation reads it from its own build. The realm
        // composite tries the primary first, whose frame lock is held: it
        // must be skipped, not end the lookup.
        realm
            .enter(|realm| {
                realm.attach_root_widget(&ToggleReader {
                    key,
                    seen: Rc::clone(&seen),
                })
            })
            .expect("the reader mounts");
        let _ = realm.enter(|realm| realm.draw_frame_entered(frame_constraints()));
        seen.take()
    });
    assert_eq!(during, vec![Some(true)]);
}
