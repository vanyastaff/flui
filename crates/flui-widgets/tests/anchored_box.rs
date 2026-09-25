//! Rebinding an anchor preserves the mounted subtree and its state.
use crate::common::{lay_out, loose};
use flui_objects::SubtreeAnchor;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{__private::AnchoredBox, Padding, SizedBox};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, StatefulView)]
struct Probe {
    creates: Arc<AtomicUsize>,
    disposes: Arc<AtomicUsize>,
}
struct ProbeState {
    disposes: Arc<AtomicUsize>,
}
impl StatefulView for Probe {
    type State = ProbeState;
    fn create_state(&self) -> Self::State {
        self.creates.fetch_add(1, Ordering::SeqCst);
        ProbeState {
            disposes: Arc::clone(&self.disposes),
        }
    }
}
impl ViewState<Probe> for ProbeState {
    fn build(&self, _: &Probe, _: &dyn BuildContext) -> impl IntoView {
        SizedBox::square(10.0)
    }
    fn dispose(&mut self) {
        self.disposes.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn changing_anchor_republishes_the_same_node_and_preserves_child_state() {
    let old = SubtreeAnchor::new();
    let new = SubtreeAnchor::new();
    let creates = Arc::new(AtomicUsize::new(0));
    let disposes = Arc::new(AtomicUsize::new(0));
    let child = Probe {
        creates: Arc::clone(&creates),
        disposes: Arc::clone(&disposes),
    };
    let mut laid = lay_out(
        Padding::all(0.0).child(AnchoredBox::new(old.clone(), child.clone())),
        loose(100.0),
    );
    let root = laid.only_child(laid.current_root());
    let child_id = laid.only_child(root);
    assert_eq!(old.get(), Some(root));
    assert_eq!(new.get(), None);
    for (anchor, retired) in [
        (new.clone(), old.clone()),
        (new.clone(), old.clone()),
        (old.clone(), new.clone()),
    ] {
        laid.pump_widget(Padding::all(0.0).child(AnchoredBox::new(anchor.clone(), child.clone())));
        assert_eq!(anchor.get(), Some(root));
        assert_eq!(laid.only_child(laid.current_root()), root);
        assert_eq!(laid.only_child(root), child_id);
        assert_eq!(creates.load(Ordering::SeqCst), 1);
        assert_eq!(disposes.load(Ordering::SeqCst), 0);
        assert_eq!(retired.get(), None);
    }
    laid.pump_widget(Padding::all(0.0).child(SizedBox::square(20.0)));
    assert_eq!(old.get(), None);
    assert_eq!(new.get(), None);
    assert_eq!(disposes.load(Ordering::SeqCst), 1);
}
