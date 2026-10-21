//! [`Keyed`] — a list row that keeps its element, and the widget state under
//! it, when the rows around it are added or removed.

use flui::foundation::ViewKey;
use flui::prelude::*;
use flui::view::ValueKey;
use flui::view::element::ElementKind;

/// `child` under the identity `id`: a removed row before it no longer hands
/// its swipe offset or its dismissal to the next row.
#[derive(Clone)]
pub(crate) struct Keyed {
    key: ValueKey<u64>,
    child: BoxedView,
}

impl Keyed {
    pub(crate) fn new(id: u64, child: impl IntoView) -> Self {
        Self {
            key: ValueKey::new(id),
            child: child.into_view().boxed(),
        }
    }
}

impl View for Keyed {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

impl StatelessView for Keyed {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }
}
