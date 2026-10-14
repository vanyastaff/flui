//! A focus request on a node its widget has let go answers `Stale` and does
//! nothing, now or later; a node the app owns stays usable.

use std::cell::RefCell;
use std::rc::Rc;

use flui_interaction::focus::{FocusNode, FocusRequestOutcome};
use flui_view::prelude::*;
use flui_widgets::interaction::Focus;
use flui_widgets::{Row, SizedBox};

use crate::common::harness::mount;

type Slot = Rc<RefCell<Option<Rc<FocusNode>>>>;

/// Records the node of the `Focus` enclosing it — how a callback captures it.
#[derive(Clone, StatelessView)]
struct Capture(Slot);

impl StatelessView for Capture {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.0.borrow_mut() = Focus::maybe_of(ctx);
        SizedBox::new(20.0, 20.0)
    }
}

/// A widget-owned `Focus` capturing its node, beside one hosting `other`.
fn captured_beside(slot: &Slot, other: &Rc<FocusNode>) -> impl View {
    Row::new((
        Focus::new(Capture(Rc::clone(slot))),
        Focus::new(SizedBox::new(20.0, 20.0)).focus_node(Rc::clone(other)),
    ))
}

fn captured(slot: &Slot) -> Rc<FocusNode> {
    slot.borrow().clone().expect("the Focus built its child")
}

/// The widget's node after the widget is gone, and after the widget switched
/// to another node, answers `Stale`; the focus stays where it was. An app's
/// own node outlives its host: a request waits for its next attachment.
pub(crate) fn stale_focus_request_is_inert() {
    // Disposed with its widget.
    let slot: Slot = Rc::default();
    let other = FocusNode::with_debug_label("other");
    let mut harness = mount(captured_beside(&slot, &other));
    let retired = captured(&slot);
    assert_eq!(other.request_focus(), FocusRequestOutcome::Focused);
    harness.swap_root(Row::new((
        SizedBox::new(20.0, 20.0),
        Focus::new(SizedBox::new(20.0, 20.0)).focus_node(Rc::clone(&other)),
    )));
    harness.tick();
    assert!(
        other.has_primary_focus(),
        "removing a sibling keeps the focus"
    );
    assert_eq!(retired.request_focus(), FocusRequestOutcome::Stale);
    harness.swap_root(captured_beside(&Rc::default(), &other));
    harness.tick();
    assert!(other.has_primary_focus(), "the stale request never lands");

    // Replaced by an app node on rebuild.
    let slot: Slot = Rc::default();
    let other = FocusNode::with_debug_label("other");
    let adopted = FocusNode::with_debug_label("adopted");
    let mut harness = mount(captured_beside(&slot, &other));
    let replaced = captured(&slot);
    assert_eq!(other.request_focus(), FocusRequestOutcome::Focused);
    harness.swap_root(Row::new((
        Focus::new(Capture(Rc::clone(&slot))).focus_node(Rc::clone(&adopted)),
        Focus::new(SizedBox::new(20.0, 20.0)).focus_node(Rc::clone(&other)),
    )));
    harness.tick();
    assert!(
        Rc::ptr_eq(&captured(&slot), &adopted),
        "the widget hosts the app's node now"
    );
    assert_eq!(replaced.request_focus(), FocusRequestOutcome::Stale);
    assert!(other.has_primary_focus());

    // The app's own node is not retired with its host.
    let app_node = FocusNode::with_debug_label("app node");
    let mut harness = mount(Focus::new(SizedBox::new(20.0, 20.0)).focus_node(Rc::clone(&app_node)));
    harness.swap_root(SizedBox::new(20.0, 20.0));
    harness.tick();
    assert_eq!(app_node.request_focus(), FocusRequestOutcome::Queued);
    harness.swap_root(Focus::new(SizedBox::new(20.0, 20.0)).focus_node(Rc::clone(&app_node)));
    harness.tick();
    assert!(
        app_node.has_primary_focus(),
        "it takes the focus when hosted again"
    );
}
