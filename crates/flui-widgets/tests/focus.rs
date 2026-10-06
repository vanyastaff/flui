//! [`Focus`], [`FocusScope`] and [`ExcludeFocus`] against a mounted tree:
//! attachment, autofocus, focus-change callbacks, exclusion and
//! geometry-ordered traversal.

use std::rc::Rc;

use flui_interaction::routing::{FocusNode, FocusScopeNode};
use flui_view::ViewExt;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::interaction::{Focus, FocusChangeHandler, FocusScope};
use flui_widgets::{Positioned, SizedBox, Stack};

use crate::common::harness::mount;

/// A root that can drop the focus subtree without changing its own type —
/// `swap_root` dispatches by `TypeId`.
#[derive(Clone)]
struct Host {
    show: bool,
    scope: Rc<FocusScopeNode>,
    node: Rc<FocusNode>,
    autofocus: bool,
    on_focus_change: Option<FocusChangeHandler>,
}

impl View for Host {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for Host {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        if !self.show {
            return SizedBox::new(1.0, 1.0).into_view().boxed();
        }
        let mut focus = Focus::new(SizedBox::new(10.0, 10.0))
            .focus_node(Rc::clone(&self.node))
            .autofocus(self.autofocus);
        if let Some(handler) = &self.on_focus_change {
            let handler = Rc::clone(handler);
            focus = focus.on_focus_change(move |cx, focused| handler(cx, focused));
        }
        FocusScope::with_external_node(Rc::clone(&self.scope), focus)
            .into_view()
            .boxed()
    }
}

/// The mount shape: the widget scope hangs under the
/// presentation's standard shortcut focus, the node hangs under the
/// widget scope, and unmounting detaches both and releases primary focus.
///
/// Also covers autofocus on mount.
///
/// Red-check: make `enclosing_scope` always answer the root scope — the
/// node parents to the root and the first assertion fails.
pub(crate) fn a_focus_widget_attaches_under_the_nearest_scope_and_unmount_releases() {
    let scope = FocusScopeNode::with_debug_label("host-scope");
    let node = FocusNode::with_debug_label("host-node");
    let mut harness = mount(Host {
        show: true,
        scope: Rc::clone(&scope),
        node: Rc::clone(&node),
        autofocus: true,
        on_focus_change: None,
    });
    let manager = harness.focus_manager();

    assert_eq!(
        node.parent().map(|parent| parent.id()),
        Some(scope.as_focus_node().id()),
        "the node hangs under the widget scope, not the root"
    );
    let traversal_parent = scope
        .as_focus_node()
        .parent()
        .expect("the widget scope has the presentation traversal parent");
    assert_eq!(traversal_parent.debug_label(), Some("Shortcuts"));
    assert_eq!(
        traversal_parent.parent().map(|parent| parent.id()),
        Some(manager.root_scope().as_focus_node().id()),
        "the presentation traversal focus hangs under the root scope"
    );
    assert!(
        node.has_primary_focus(),
        "autofocus focused the node on mount"
    );
    assert_eq!(
        scope.focused_child().map(|focused| focused.id()),
        Some(node.id())
    );

    harness.swap_root(Host {
        show: false,
        scope: Rc::clone(&scope),
        node: Rc::clone(&node),
        autofocus: true,
        on_focus_change: None,
    });

    assert!(!node.is_attached(), "unmount detached the node");
    assert!(
        !scope.as_focus_node().is_attached(),
        "unmount detached the widget scope"
    );
    assert!(
        manager.primary_focus().is_none(),
        "a disposed focused widget releases the primary focus"
    );
}

// ------------------------------------------------------------------
// Focus::of / Focus::maybe_of / FocusScope::of
// ------------------------------------------------------------------

// ------------------------------------------------------------------------
// Traversal order
// ------------------------------------------------------------------------

/// Widget-mounted nodes traverse in **reading order**, not attach order —
/// the ADR-0026 traversal-geometry gap, closed: every `Focus` anchors
/// its child and installs a rect provider, so `ReadingOrderPolicy` sorts
/// real committed geometry. The attach order (`a`, `b`, `c`) is chosen so
/// the on-screen order (`b`, `a`, `c`) is **not** one of its rotations:
/// from `a`, geometry says `c` next, attach order would say `b`.
///
/// Red-check (the pre-fix behavior): skip `install_rect_provider` in
/// `init_state` — every rect reads zero, the sort degenerates to attach
/// order, and the first assertion gets `b`.
pub(crate) fn tab_traversal_follows_geometry_not_attach_order() {
    let scope = FocusScopeNode::with_debug_label("traversal-scope");
    let a = FocusNode::with_debug_label("a-middle");
    let b = FocusNode::with_debug_label("b-top");
    let c = FocusNode::with_debug_label("c-bottom");

    let positioned = |top: f64, node: &Rc<FocusNode>| {
        Positioned::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(node)))
            .left(0.0)
            .top(top)
            .width(10.0)
            .height(10.0)
            .into_view()
            .boxed()
    };
    let harness = mount(FocusScope::with_external_node(
        Rc::clone(&scope),
        Stack::new(vec![
            positioned(50.0, &a),
            positioned(0.0, &b),
            positioned(100.0, &c),
        ]),
    ));
    let manager = harness.focus_manager();

    assert_eq!(
        b.rect().min_y(),
        0.0,
        "sanity: the provider measures committed layout"
    );
    assert_eq!(a.rect().min_y(), 50.0);

    a.request_focus();

    manager.focus_next();
    assert!(
        c.has_primary_focus(),
        "after the middle node comes the bottom one — reading order, not attach order"
    );
    manager.focus_next();
    assert!(b.has_primary_focus(), "wraparound lands on the top node");
    manager.focus_next();
    assert!(a.has_primary_focus(), "then the middle again");

    manager.unfocus();
}

/// Event context (ADR-0086): the focus edge and the key handler run inside a
/// write the `Focus` opens from the writer source it acquired in
/// `init_state`.
pub(crate) mod event_cx {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_interaction::routing::FocusNode;
    use flui_view::prelude::*;
    use flui_widgets::SizedBox;
    use flui_widgets::interaction::Focus;

    use crate::common::harness::mount;
    use crate::common::{ProbeSignals, SignalProbe};

    pub(crate) fn replacing_a_focused_node_delivers_a_writable_loss_and_new_gain() {
        let old_node = FocusNode::with_debug_label("old-signal-focus");
        let new_node = FocusNode::with_debug_label("new-signal-focus");
        let selected = Rc::new(RefCell::new(Rc::clone(&old_node)));
        let configuration = Rc::clone(&selected);
        let edges = Rc::new(RefCell::new(Vec::new()));
        let recorded = Rc::clone(&edges);
        let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
            let recorded = Rc::clone(&recorded);
            Focus::new(SizedBox::new(10.0, 10.0))
                .focus_node(Rc::clone(&configuration.borrow()))
                .on_focus_change(move |cx, focused| {
                    recorded.borrow_mut().push(focused);
                    count.set(cx, u32::from(focused))
                })
        });
        let mut harness = mount(probe.view());
        old_node.request_focus();
        assert_eq!(probe.value(), Ok(1));

        *selected.borrow_mut() = Rc::clone(&new_node);
        harness.swap_root(probe.view());
        assert!(!old_node.has_focus());
        assert_eq!(probe.value(), Ok(0), "replacement delivered its loss");
        new_node.request_focus();
        assert_eq!(probe.value(), Ok(1));
        assert_eq!(*edges.borrow(), [true, false, true]);
        old_node.request_focus();
        assert_eq!(
            *edges.borrow(),
            [true, false, true],
            "the retired node cannot notify its old widget"
        );
    }

    pub(crate) fn a_refused_write_in_a_focus_edge_is_reported_not_panicked() {
        let node = FocusNode::with_debug_label("probe");
        let probe_node = Rc::clone(&node);
        let probe = SignalProbe::new(move |ProbeSignals { released, .. }| {
            Focus::new(SizedBox::new(10.0, 10.0))
                .focus_node(Rc::clone(&probe_node))
                .on_focus_change(move |cx, _focused| released.set(cx, 1))
        });
        let _harness = mount(probe.view());

        let (_, log) = flui_testing::log_capture::capture(|| node.request_focus());

        assert!(
            log.contains("an event callback's signal write was refused"),
            "the refusal is logged at the dispatch boundary: {log}"
        );
        assert_eq!(probe.value(), Ok(0));
    }
}

/// An assistive focus request uses the actual mounted focus node, then publishes it.
pub(crate) fn platform_focus_requests_the_mounted_node_and_rejects_disabled_focus() {
    use crate::common::{lay_out, loose};
    use flui_testing::{Action, ActionRequest, TreeId};
    use flui_widgets::Semantics;

    let node = FocusNode::with_debug_label("assistive-target");
    let mut laid = lay_out(
        Focus::new(
            Semantics::new()
                .label("assistive target")
                .child(SizedBox::new(10.0, 10.0)),
        )
        .focus_node(Rc::clone(&node)),
        loose(100.0),
    );
    laid.enable_semantics();
    laid.pump();
    assert!(!node.has_primary_focus());
    let tree = laid.a11y_tree().expect("semantics enabled");
    let target = tree
        .find_by_label("assistive target")
        .expect("named focus target");
    assert!(
        target.supports_action(Action::Focus),
        "focusable control must expose the action"
    );
    let listener = laid
        .accessibility_action_listener()
        .expect("production platform listener");
    listener(ActionRequest {
        action: Action::Focus,
        target_tree: TreeId::ROOT,
        target_node: target.id(),
        data: None,
    });
    laid.tick();
    assert!(
        node.has_primary_focus(),
        "the actual FocusNode accepted assistive focus"
    );
    let tree = laid.a11y_tree().expect("focused frame published");
    assert_eq!(
        tree.raw().focus,
        tree.find_by_label("assistive target")
            .expect("live target")
            .id()
    );

    let refused = FocusNode::with_debug_label("disabled-assistive-target");
    let mut disabled = lay_out(
        Focus::new(
            Semantics::new()
                .label("disabled target")
                .child(SizedBox::new(10.0, 10.0)),
        )
        .focus_node(Rc::clone(&refused))
        .can_request_focus(false),
        loose(100.0),
    );
    disabled.enable_semantics();
    disabled.pump();
    let tree = disabled.a11y_tree().expect("disabled tree published");
    let target = tree
        .find_by_label("disabled target")
        .expect("named disabled target");
    assert!(!target.supports_action(Action::Focus));
    assert!(
        disabled
            .invoke_semantics_action(ActionRequest {
                action: Action::Focus,
                target_tree: TreeId::ROOT,
                target_node: target.id(),
                data: None
            })
            .is_err()
    );
    assert!(!refused.has_primary_focus());
}

/// A `Focus` whose external node another owner adopted no longer focuses it
/// on an assistive request: the request reaches the old element, which no
/// longer owns the attachment.
pub(crate) fn an_adopted_external_node_ignores_the_old_elements_focus_action() {
    use crate::common::{lay_out, loose};
    use flui_testing::{Action, ActionRequest, TreeId};
    use flui_widgets::Semantics;

    let node = FocusNode::with_debug_label("adopted-target");
    let mut laid = lay_out(
        Focus::new(
            Semantics::new()
                .label("adopted target")
                .child(SizedBox::new(10.0, 10.0)),
        )
        .focus_node(Rc::clone(&node)),
        loose(100.0),
    );
    laid.enable_semantics();
    laid.pump();
    let target = laid
        .a11y_tree()
        .expect("semantics enabled")
        .find_by_label("adopted target")
        .expect("named focus target")
        .id();
    let adopter = node
        .parent()
        .and_then(|parent| parent.parent())
        .expect("the mounted node hangs below a scope with a parent");
    let _adopted = adopter
        .adopt_node(&node)
        .expect("a live node moves under another owner");

    let listener = laid
        .accessibility_action_listener()
        .expect("production platform listener");
    listener(ActionRequest {
        action: Action::Focus,
        target_tree: TreeId::ROOT,
        target_node: target,
        data: None,
    });
    laid.tick();
    assert!(
        !node.has_primary_focus(),
        "the old element focused a node another owner adopted"
    );
}
