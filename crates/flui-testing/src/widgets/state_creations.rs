//! [`StateCreations`] — a leaf whose state creations are counted.
//!
//! "Was this subtree destroyed and rebuilt?" and "did a reorder reuse this
//! element?" are observable as the number of times a stateful leaf's
//! `create_state` ran; the leaf is a 10×10 box.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::SizedBox;

/// A stateful leaf that adds one to its counter each time its state is
/// created. Clones share the counter.
#[derive(Clone, Debug, Default)]
pub struct StateCreations {
    creations: Arc<AtomicUsize>,
}

impl StateCreations {
    /// A leaf counting into `creations`.
    #[must_use]
    pub fn counting(creations: &Arc<AtomicUsize>) -> Self {
        Self {
            creations: Arc::clone(creations),
        }
    }

    /// How many states this leaf, and its clones, have created.
    #[must_use]
    pub fn count(&self) -> usize {
        self.creations.load(Ordering::Relaxed)
    }
}

/// The state of a [`StateCreations`]: nothing, its creation is the signal.
#[derive(Debug)]
pub struct StateCreationsState;

impl StatefulView for StateCreations {
    type State = StateCreationsState;

    fn create_state(&self) -> Self::State {
        self.creations.fetch_add(1, Ordering::Relaxed);
        StateCreationsState
    }
}

impl ViewState<StateCreations> for StateCreationsState {
    fn build(&self, _view: &StateCreations, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

impl View for StateCreations {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}
