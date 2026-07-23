//! Public `ExcludeFocus` construction and focus-policy behavior.

mod common;

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use common::{lay_out, loose};
use flui_interaction::FocusNode;
use flui_widgets::prelude::*;

const CONSTRUCTOR_DEFAULT: u8 = 0;
const EXCLUSION_DISABLED: u8 = 1;
const EXCLUSION_ENABLED: u8 = 2;

#[derive(Clone, StatelessView)]
struct ExcludeFocusHost {
    mode: Arc<AtomicU8>,
    node: Rc<FocusNode>,
}

impl StatelessView for ExcludeFocusHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let boundary = ExcludeFocus::new(
            Focus::new(SizedBox::new(20.0, 10.0)).focus_node(Rc::clone(&self.node)),
        );
        match self.mode.load(Ordering::Relaxed) {
            EXCLUSION_DISABLED => boundary.excluding(false),
            EXCLUSION_ENABLED => boundary.excluding(true),
            _ => boundary,
        }
    }
}

#[test]
fn prelude_exclude_focus_refuses_then_allows_and_evicts_descendant_focus() {
    let mode = Arc::new(AtomicU8::new(CONSTRUCTOR_DEFAULT));
    let node = FocusNode::with_debug_label("exclude-focus-child");
    let mut laid = lay_out(
        ExcludeFocusHost {
            mode: Arc::clone(&mode),
            node: Rc::clone(&node),
        },
        loose(100.0),
    );
    let manager = laid.focus_manager();

    node.request_focus();
    assert!(
        manager.primary_focus().is_none(),
        "ExcludeFocus::new excludes descendants without calling the builder"
    );

    mode.store(EXCLUSION_DISABLED, Ordering::Relaxed);
    laid.pump();
    node.request_focus();
    assert!(node.has_primary_focus());

    mode.store(EXCLUSION_ENABLED, Ordering::Relaxed);
    laid.pump();
    assert!(
        manager.primary_focus().is_none(),
        "enabling exclusion evicts focus"
    );

    mode.store(EXCLUSION_DISABLED, Ordering::Relaxed);
    laid.pump();
    assert!(
        manager.primary_focus().is_none(),
        "disabling does not auto-refocus"
    );
    node.request_focus();
    assert!(node.has_primary_focus());
    manager.unfocus();
}
