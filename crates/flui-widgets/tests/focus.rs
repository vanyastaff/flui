//! [`Focus`], [`FocusScope`] and [`ExcludeFocus`] against a mounted tree:
//! attachment, autofocus, focus-change callbacks, exclusion and
//! geometry-ordered traversal.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_interaction::routing::{FocusNode, FocusScopeNode, KeyEventHandler};
use flui_view::ViewExt;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::interaction::{ExcludeFocus, Focus, FocusChangeHandler, FocusScope};
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

#[derive(Clone, StatelessView)]
struct ExcludeHost {
    excluding: bool,
    node: Rc<FocusNode>,
}

impl StatelessView for ExcludeHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        ExcludeFocus::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&self.node)))
            .excluding(self.excluding)
    }
}

#[derive(Clone, StatelessView)]
struct FocusDependencyProbe {
    builds: Rc<Cell<usize>>,
    focused: Rc<Cell<bool>>,
}

impl StatelessView for FocusDependencyProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        self.focused.set(Focus::of(ctx).has_focus());
        SizedBox::new(1.0, 1.0)
    }
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
            focus = focus.on_focus_change(move |focused| handler(focused));
        }
        FocusScope::with_external_node(Rc::clone(&self.scope), focus)
            .into_view()
            .boxed()
    }
}

/// Flutter parity: `focus_scope_test.dart`'s `"Descendants of ExcludeFocus
/// aren't focusable."` (a request while excluding refuses) and
/// `"ExcludeFocus doesn't transfer focus to another descendant."` (turning
/// exclusion on evicts an already-focused descendant without picking a new
/// one), tag `3.44.0`. Also the idempotent-toggle and no-auto-refocus-on-
/// re-enable properties, which the oracle tests do not separately cover.
#[test]
fn exclude_focus_refuses_allows_evicts_idempotently_and_does_not_refocus() {
    let node = FocusNode::with_debug_label("exclude-focus-unit-child");
    let mut harness = mount(ExcludeHost {
        excluding: true,
        node: Rc::clone(&node),
    });
    let manager = harness.focus_manager();
    node.request_focus();
    assert!(manager.primary_focus().is_none());

    harness.swap_root(ExcludeHost {
        excluding: false,
        node: Rc::clone(&node),
    });
    node.request_focus();
    assert!(node.has_primary_focus());

    harness.swap_root(ExcludeHost {
        excluding: true,
        node: Rc::clone(&node),
    });
    assert!(manager.primary_focus().is_none());
    harness.swap_root(ExcludeHost {
        excluding: true,
        node: Rc::clone(&node),
    });
    assert!(manager.primary_focus().is_none());

    harness.swap_root(ExcludeHost {
        excluding: false,
        node: Rc::clone(&node),
    });
    assert!(manager.primary_focus().is_none());
    node.request_focus();
    assert!(node.has_primary_focus());
    manager.unfocus();
}

/// The mount shape (`_FocusState.initState` + `FocusScope`,
/// `focus_scope.dart:565-630`): the widget scope hangs under the
/// presentation's standard shortcut focus, the node hangs under the
/// widget scope, and unmounting detaches both and releases primary focus.
///
/// Flutter parity: `focus_scope_test.dart`'s `'Removing a FocusScope
/// removes its node from the tree'` (the unmount-detaches-both half) and
/// `'Autofocus works'` (the autofocus-on-mount half), tag `3.44.0`.
///
/// Red-check: make `enclosing_scope` always answer the root scope — the
/// node parents to the root and the first assertion fails.
#[test]
fn a_focus_widget_attaches_under_the_nearest_scope_and_unmount_releases() {
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

/// A rebuild that flips `autofocus` from `false` to `true` makes the
/// still-unattempted autofocus request — Flutter's `didUpdateWidget`
/// re-running `_handleAutofocus` on an `autofocus` change
/// (`focus_scope_test.dart`'s "Can autofocus a node.", tag 3.44.0), not
/// just `initState`/`didChangeDependencies`.
///
/// Red-check (verified): drop the `try_autofocus()` call from
/// `did_update_view` — the node mounted with `autofocus: false` never
/// requests focus on the later rebuild, and the assertion fails.
#[test]
fn a_rebuild_that_turns_on_autofocus_requests_focus() {
    let scope = FocusScopeNode::with_debug_label("rebuild-autofocus-scope");
    let node = FocusNode::with_debug_label("rebuild-autofocus-node");
    let mut harness = mount(Host {
        show: true,
        scope: Rc::clone(&scope),
        node: Rc::clone(&node),
        autofocus: false,
        on_focus_change: None,
    });
    let manager = harness.focus_manager();
    assert!(!node.has_primary_focus(), "sanity: not focused on mount");

    harness.swap_root(Host {
        show: true,
        scope: Rc::clone(&scope),
        node: Rc::clone(&node),
        autofocus: true,
        on_focus_change: None,
    });

    assert!(
        node.has_primary_focus(),
        "the rebuild's autofocus: true made its one-shot request"
    );

    // A second rebuild that merely repeats `autofocus: true` must not
    // re-attempt: nothing else focused now, so an unfocus followed by a
    // repeated-`true` rebuild staying unfocused proves the latch, not a
    // silently-passing accident.
    manager.unfocus();
    harness.swap_root(Host {
        show: true,
        scope: Rc::clone(&scope),
        node: Rc::clone(&node),
        autofocus: true,
        on_focus_change: None,
    });
    assert!(
        !node.has_primary_focus(),
        "the one-shot latch does not re-request on a value-repeating rebuild"
    );

    manager.unfocus();
}

/// `autofocus` yields when the scope already focused something
/// (`_handleAutofocus`, `focus_scope.dart:625-630`): with two autofocus
/// siblings, the first to mount wins and the second is skipped.
///
/// Flutter parity: `focus_scope_test.dart`'s `"Won't autofocus a node if
/// one is already focused."`, tag `3.44.0`.
///
/// Red-check: drop the `focused_child().is_none()` gate in `init_state` —
/// the second steals the focus and both assertions flip.
#[test]
fn autofocus_yields_to_an_already_focused_scope() {
    let scope = FocusScopeNode::with_debug_label("autofocus-scope");
    let first = FocusNode::with_debug_label("first");
    let second = FocusNode::with_debug_label("second");
    let harness = mount(FocusScope::with_external_node(
        Rc::clone(&scope),
        flui_widgets::Column::new(vec![
            Focus::new(SizedBox::new(10.0, 10.0))
                .focus_node(Rc::clone(&first))
                .autofocus(true)
                .into_view()
                .boxed(),
            Focus::new(SizedBox::new(10.0, 10.0))
                .focus_node(Rc::clone(&second))
                .autofocus(true)
                .into_view()
                .boxed(),
        ]),
    ));
    let manager = harness.focus_manager();

    assert!(first.has_primary_focus(), "the first autofocus wins");
    assert!(!second.has_primary_focus(), "the second yields");

    manager.unfocus();
}

/// `on_focus_change` fires on the edges — `true` on gain, `false` on loss
/// (`Focus.onFocusChange`, `focus_scope.dart:167`).
///
/// Red-check: report `was_focused` instead of `now_focused` in
/// `install_focus_listener` — the recorded edges invert.
#[test]
fn on_focus_change_reports_gain_and_loss() {
    let scope = FocusScopeNode::with_debug_label("edge-scope");
    let node = FocusNode::with_debug_label("edge-node");
    let edges = Rc::new(RefCell::new(Vec::<bool>::new()));
    let recorded = Rc::clone(&edges);
    let mut harness = mount(Host {
        show: true,
        scope: Rc::clone(&scope),
        node: Rc::clone(&node),
        autofocus: false,
        on_focus_change: Some(Rc::new(move |focused| recorded.borrow_mut().push(focused))),
    });
    let manager = harness.focus_manager();

    node.request_focus();
    assert_eq!(
        edges.borrow().as_slice(),
        [true],
        "the focus-manager notification phase delivers the gain outside build"
    );
    harness.tick();
    manager.unfocus();
    assert_eq!(
        edges.borrow().as_slice(),
        [true, false],
        "the loss is delivered by focus notification, not a later build"
    );
    harness.tick();
    assert_eq!(
        edges.borrow().as_slice(),
        [true, false],
        "gain then loss, exactly once each"
    );
}

#[test]
fn focus_of_dependency_rebuilds_when_the_node_focus_changes() {
    let node = FocusNode::with_debug_label("dependency-node");
    let builds = Rc::new(Cell::new(0));
    let focused = Rc::new(Cell::new(false));
    let mut harness = mount(
        Focus::new(FocusDependencyProbe {
            builds: Rc::clone(&builds),
            focused: Rc::clone(&focused),
        })
        .focus_node(Rc::clone(&node)),
    );
    let initial_builds = builds.get();

    node.request_focus();
    harness.tick();
    assert!(focused.get());
    assert!(
        builds.get() > initial_builds,
        "the inherited dependency rebuilt after focus gain"
    );

    let focused_builds = builds.get();
    harness.focus_manager().unfocus();
    harness.tick();
    assert!(!focused.get());
    assert!(
        builds.get() > focused_builds,
        "the inherited dependency rebuilt after focus loss"
    );
}

/// A configurable `Focus` whose flags/handlers change across a `swap_root`, so
/// the inner `Focus`'s `did_update_view` → `configure` runs with a new config.
#[derive(Clone)]
struct Configurable {
    node: Rc<FocusNode>,
    scope: Rc<FocusScopeNode>,
    can_request_focus: Option<bool>,
    skip_traversal: Option<bool>,
    on_key_event: Option<KeyEventHandler>,
    on_focus_change: Option<FocusChangeHandler>,
}

impl View for Configurable {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for Configurable {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let mut focus = Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&self.node));
        if let Some(can) = self.can_request_focus {
            focus = focus.can_request_focus(can);
        }
        if let Some(skip) = self.skip_traversal {
            focus = focus.skip_traversal(skip);
        }
        if let Some(handler) = &self.on_key_event {
            focus = focus.on_key_event(Rc::clone(handler));
        }
        if let Some(handler) = &self.on_focus_change {
            let handler = Rc::clone(handler);
            focus = focus.on_focus_change(move |focused| handler(focused));
        }
        FocusScope::with_external_node(Rc::clone(&self.scope), focus)
            .into_view()
            .boxed()
    }
}

#[derive(Clone, StatelessView)]
struct ScopeSwapHost {
    external_scope: Option<Rc<FocusScopeNode>>,
    node: Rc<FocusNode>,
}

impl StatelessView for ScopeSwapHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let child = Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&self.node));
        match &self.external_scope {
            Some(scope) => FocusScope::with_external_node(Rc::clone(scope), child)
                .into_view()
                .boxed(),
            None => FocusScope::new(child).into_view().boxed(),
        }
    }
}

#[derive(Clone, StatelessView)]
struct ParentNodeSwapHost {
    parent: Rc<FocusNode>,
    child: Rc<FocusNode>,
}

impl StatelessView for ParentNodeSwapHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Focus::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&self.child)))
            .focus_node(Rc::clone(&self.parent))
    }
}

/// On the regular external-node path, omitted attributes read through to
/// the node's current values. Dropping an explicit override therefore
/// preserves the value already installed on that caller-owned node —
/// Flutter's `Focus.focusNode` getter/update contract.
#[test]
fn an_external_node_keeps_managed_values_when_overrides_are_dropped() {
    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
    use flui_interaction::routing::KeyEventResult;

    let scope = FocusScopeNode::with_debug_label("cfg-scope");
    let node = FocusNode::with_debug_label("cfg-node");
    let mut harness = mount(Configurable {
        node: Rc::clone(&node),
        scope: Rc::clone(&scope),
        can_request_focus: Some(false),
        skip_traversal: Some(true),
        on_key_event: Some(Rc::new(|_event| KeyEventResult::Handled)),
        on_focus_change: None,
    });
    let key = || KeyEvent {
        state: KeyState::Down,
        key: Key::Character("a".into()),
        modifiers: Modifiers::default(),
        ..KeyEvent::default()
    };
    assert!(
        !node.can_request_focus(),
        "configured can_request_focus(false)"
    );
    assert!(node.skip_traversal(), "configured skip_traversal(true)");
    assert_eq!(
        node.handle_key_event(&key()),
        KeyEventResult::Handled,
        "the configured key handler runs"
    );

    // Rebuild with none of the three set.
    harness.swap_root(Configurable {
        node: Rc::clone(&node),
        scope: Rc::clone(&scope),
        can_request_focus: None,
        skip_traversal: None,
        on_key_event: None,
        on_focus_change: None,
    });

    assert!(
        !node.can_request_focus(),
        "the managed external node retains its current false value"
    );
    assert!(
        node.skip_traversal(),
        "the managed external node retains its current true value"
    );
    assert_eq!(
        node.handle_key_event(&key()),
        KeyEventResult::Handled,
        "the installed handler remains the external node's current value"
    );

    // Even after the override disappears, this host remembers that it
    // installed the handler and removes it when it releases the node.
    harness.swap_root(SizedBox::new(1.0, 1.0));
    assert_eq!(
        node.handle_key_event(&key()),
        KeyEventResult::Ignored,
        "a released managed node does not retain a widget-owned callback"
    );
}

/// Cleanup is tied to the exact handler generation installed by this
/// widget, not merely to the node identity. A caller may replace the
/// handler while the node is hosted; unmounting the stale registration
/// must preserve that newer value.
#[test]
fn managed_node_cleanup_cannot_erase_a_later_external_handler() {
    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
    use flui_interaction::routing::KeyEventResult;

    let node = FocusNode::with_debug_label("generation-node");
    let scope = FocusScopeNode::with_debug_label("generation-scope");
    let mut harness = mount(Configurable {
        node: Rc::clone(&node),
        scope,
        can_request_focus: None,
        skip_traversal: None,
        on_key_event: Some(Rc::new(|_| KeyEventResult::Handled)),
        on_focus_change: None,
    });
    node.set_on_key_event(Rc::new(|_| KeyEventResult::SkipRemainingHandlers));

    harness.swap_root(SizedBox::new(1.0, 1.0));
    let key = KeyEvent {
        state: KeyState::Down,
        key: Key::Character("a".into()),
        modifiers: Modifiers::default(),
        ..KeyEvent::default()
    };
    assert_eq!(
        node.handle_key_event(&key),
        KeyEventResult::SkipRemainingHandlers,
        "generation-checked cleanup preserves the later external writer"
    );
}

/// `with_external_node` makes every node attribute caller-owned, including
/// the key handler. Conflicting widget builders cannot mutate it, and
/// disposal cannot erase it.
#[test]
fn a_source_of_truth_external_node_is_never_reconfigured() {
    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
    use flui_interaction::routing::KeyEventResult;

    let node = FocusNode::with_debug_label("source-node");
    node.set_can_request_focus(false);
    node.set_skip_traversal(true);
    node.set_descendants_are_focusable(false);
    node.set_on_key_event(Rc::new(|_| KeyEventResult::Handled));

    let mut harness = mount(
        Focus::with_external_node(Rc::clone(&node), SizedBox::new(10.0, 10.0))
            .can_request_focus(true)
            .skip_traversal(false)
            .descendants_are_focusable(true)
            .on_key_event(Rc::new(|_| KeyEventResult::Ignored)),
    );
    let key = KeyEvent {
        state: KeyState::Down,
        key: Key::Character("a".into()),
        modifiers: Modifiers::default(),
        ..KeyEvent::default()
    };

    assert!(!node.can_request_focus());
    assert!(node.skip_traversal());
    assert!(!node.descendants_are_focusable());
    assert_eq!(node.handle_key_event(&key), KeyEventResult::Handled);

    harness.swap_root(SizedBox::new(1.0, 1.0));
    assert!(!node.is_attached(), "the widget still owns the attachment");
    assert!(!node.can_request_focus());
    assert!(node.skip_traversal());
    assert!(!node.descendants_are_focusable());
    assert_eq!(
        node.handle_key_event(&key),
        KeyEventResult::Handled,
        "the caller-owned handler survives widget disposal"
    );
}

#[test]
fn a_rebuild_replaces_the_external_node_without_leaking_attachment_or_handler() {
    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
    use flui_interaction::routing::{FocusRequestOutcome, KeyEventResult};

    let scope = FocusScopeNode::with_debug_label("node-replacement-scope");
    let first = FocusNode::with_debug_label("first");
    let replacement = FocusNode::with_debug_label("replacement");
    let handler: KeyEventHandler = Rc::new(|_| KeyEventResult::Handled);
    let mut harness = mount(Configurable {
        node: Rc::clone(&first),
        scope: Rc::clone(&scope),
        can_request_focus: None,
        skip_traversal: None,
        on_key_event: Some(Rc::clone(&handler)),
        on_focus_change: None,
    });
    let manager = harness.focus_manager();
    let key = KeyEvent {
        state: KeyState::Down,
        key: Key::Character("a".into()),
        modifiers: Modifiers::default(),
        ..KeyEvent::default()
    };

    first.request_focus();
    assert_eq!(
        replacement.request_focus(),
        FocusRequestOutcome::Queued,
        "a detached replacement may queue focus before the rebuild"
    );

    harness.swap_root(Configurable {
        node: Rc::clone(&replacement),
        scope: Rc::clone(&scope),
        can_request_focus: None,
        skip_traversal: None,
        on_key_event: Some(handler),
        on_focus_change: None,
    });

    assert!(!first.is_attached(), "the superseded node was detached");
    assert!(
        replacement.has_primary_focus(),
        "the replacement attached and fulfilled its queued request"
    );
    assert_eq!(
        first.handle_key_event(&key),
        KeyEventResult::Ignored,
        "the widget-owned handler was removed from the old external node"
    );
    assert_eq!(
        replacement.handle_key_event(&key),
        KeyEventResult::Handled,
        "the replacement received the current handler"
    );
    assert_eq!(
        manager.listener_count(),
        0,
        "a Focus without an edge callback installs no manager subscription"
    );
}

#[test]
fn a_live_focus_scope_swap_preserves_its_descendant_subtree_and_focus() {
    let first = FocusScopeNode::with_debug_label("first external scope");
    let second = FocusScopeNode::with_debug_label("second external scope");
    let third = FocusScopeNode::with_debug_label("third external scope");
    let node = FocusNode::with_debug_label("scope swap descendant");
    let mut harness = mount(ScopeSwapHost {
        external_scope: Some(Rc::clone(&first)),
        node: Rc::clone(&node),
    });
    node.request_focus();

    harness.swap_root(ScopeSwapHost {
        external_scope: Some(Rc::clone(&second)),
        node: Rc::clone(&node),
    });
    assert!(!first.as_focus_node().is_attached());
    assert!(Rc::ptr_eq(
        &node.parent().expect("descendant remains parented"),
        second.as_focus_node()
    ));
    assert!(node.has_primary_focus());

    harness.swap_root(ScopeSwapHost {
        external_scope: None,
        node: Rc::clone(&node),
    });
    let internal = node
        .parent()
        .and_then(|parent| parent.as_scope())
        .expect("external-to-internal installs a fresh scope");
    assert!(!Rc::ptr_eq(&internal, &second));
    assert!(!second.as_focus_node().is_attached());
    assert!(node.has_primary_focus());

    harness.swap_root(ScopeSwapHost {
        external_scope: Some(Rc::clone(&third)),
        node: Rc::clone(&node),
    });
    assert!(!internal.as_focus_node().is_attached());
    assert!(Rc::ptr_eq(
        &node.parent().expect("descendant remains parented"),
        third.as_focus_node()
    ));
    assert!(node.has_primary_focus());
}

#[test]
fn replacing_a_parent_focus_node_keeps_the_focused_child_attached() {
    let first_parent = FocusNode::with_debug_label("first parent");
    let replacement_parent = FocusNode::with_debug_label("replacement parent");
    let child = FocusNode::with_debug_label("focused child");
    let mut harness = mount(ParentNodeSwapHost {
        parent: Rc::clone(&first_parent),
        child: Rc::clone(&child),
    });
    child.request_focus();

    harness.swap_root(ParentNodeSwapHost {
        parent: Rc::clone(&replacement_parent),
        child: Rc::clone(&child),
    });

    assert!(!first_parent.is_attached());
    assert!(Rc::ptr_eq(
        &child.parent().expect("the child remains in the focus tree"),
        &replacement_parent
    ));
    assert!(
        child.has_primary_focus(),
        "a descendant primary focus survives its parent-node replacement"
    );
}

/// Changing `on_focus_change` across a rebuild takes effect: the listener reads
/// the current handler, not the one captured when it was installed.
///
/// Red-check: in `did_update_view`, stop updating the shared cell — the listener
/// keeps the first handler, `first` fires and `second` is never called.
#[test]
fn a_rebuild_swaps_the_on_focus_change_handler() {
    let scope = FocusScopeNode::with_debug_label("swap-scope");
    let node = FocusNode::with_debug_label("swap-node");
    let first = Rc::new(RefCell::new(Vec::<bool>::new()));
    let second = Rc::new(RefCell::new(Vec::<bool>::new()));

    let first_rec = Rc::clone(&first);
    let mut harness = mount(Configurable {
        node: Rc::clone(&node),
        scope: Rc::clone(&scope),
        can_request_focus: None,
        skip_traversal: None,
        on_key_event: None,
        on_focus_change: Some(Rc::new(move |focused| first_rec.borrow_mut().push(focused))),
    });
    let manager = harness.focus_manager();

    // Rebuild with a different handler.
    let second_rec = Rc::clone(&second);
    harness.swap_root(Configurable {
        node: Rc::clone(&node),
        scope: Rc::clone(&scope),
        can_request_focus: None,
        skip_traversal: None,
        on_key_event: None,
        on_focus_change: Some(Rc::new(move |focused| {
            second_rec.borrow_mut().push(focused);
        })),
    });

    node.request_focus();
    harness.tick();
    manager.unfocus();
    harness.tick();

    assert!(
        first.borrow().is_empty(),
        "the superseded handler no longer fires"
    );
    assert_eq!(
        second.borrow().as_slice(),
        [true, false],
        "the current handler fires the gain/loss edges"
    );
}

// ------------------------------------------------------------------
// Focus::of / Focus::maybe_of / FocusScope::of
// ------------------------------------------------------------------

/// Which tree shape a [`FocusOfProbe`] is mounted under — one reusable
/// host below instead of a bespoke type per shape.
#[derive(Clone, Copy)]
enum FocusOfShape {
    /// No Focus/FocusScope ancestor at all.
    Bare,
    /// A single plain `Focus` directly wrapping the probe.
    OneFocus,
    /// Two nested plain `Focus` widgets — the probe sits under the INNER
    /// one, so a correct lookup must not stop at the outer one.
    NestedFocus,
    /// A bare `FocusScope` directly wrapping the probe (no plain `Focus`
    /// in between) — the scope-vs-node distinction.
    BareScope,
    /// `FocusScope`, then a plain `Focus`, then the probe —
    /// `FocusScope::of` must walk past the plain `Focus` to the scope.
    ScopeThenFocus,
}

/// A leaf that records what [`Focus::maybe_of`] and [`FocusScope::of`]
/// resolve to from its own build context.
#[derive(Clone, StatelessView)]
struct FocusOfProbe {
    found_node: Rc<RefCell<Option<Rc<FocusNode>>>>,
    found_scope: Rc<RefCell<Option<Rc<FocusScopeNode>>>>,
}

impl StatelessView for FocusOfProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let _prev = std::mem::replace(&mut *self.found_node.borrow_mut(), Focus::maybe_of(ctx));
        let _prev = self.found_scope.borrow_mut().replace(FocusScope::of(ctx));
        SizedBox::new(1.0, 1.0)
    }
}

/// Composes a [`FocusOfProbe`] under `shape`, or drops the whole subtree
/// when `show` is `false` — the same toggle-to-unmount idiom `Host`/
/// `ExcludeHost` above use, so a test can `swap_root` back to a bare leaf
/// at the end and let real `dispose()` detach every node this mounted.
#[derive(Clone, StatelessView)]
struct FocusOfHost {
    shape: FocusOfShape,
    show: bool,
    outer_node: Rc<FocusNode>,
    inner_node: Rc<FocusNode>,
    scope: Rc<FocusScopeNode>,
    found_node: Rc<RefCell<Option<Rc<FocusNode>>>>,
    found_scope: Rc<RefCell<Option<Rc<FocusScopeNode>>>>,
}

impl StatelessView for FocusOfHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        if !self.show {
            return SizedBox::new(1.0, 1.0).into_view().boxed();
        }
        let probe = FocusOfProbe {
            found_node: Rc::clone(&self.found_node),
            found_scope: Rc::clone(&self.found_scope),
        };
        match self.shape {
            FocusOfShape::Bare => probe.into_view().boxed(),
            FocusOfShape::OneFocus => Focus::new(probe)
                .focus_node(Rc::clone(&self.outer_node))
                .into_view()
                .boxed(),
            FocusOfShape::NestedFocus => {
                Focus::new(Focus::new(probe).focus_node(Rc::clone(&self.inner_node)))
                    .focus_node(Rc::clone(&self.outer_node))
                    .into_view()
                    .boxed()
            }
            FocusOfShape::BareScope => {
                FocusScope::with_external_node(Rc::clone(&self.scope), probe)
                    .into_view()
                    .boxed()
            }
            FocusOfShape::ScopeThenFocus => FocusScope::with_external_node(
                Rc::clone(&self.scope),
                Focus::new(probe).focus_node(Rc::clone(&self.outer_node)),
            )
            .into_view()
            .boxed(),
        }
    }
}

/// Builds a fresh [`FocusOfHost`] with brand-new nodes/scope and empty
/// result cells for `shape`.
fn focus_of_host(shape: FocusOfShape) -> FocusOfHost {
    FocusOfHost {
        shape,
        show: true,
        outer_node: FocusNode::with_debug_label("focus-of-outer"),
        inner_node: FocusNode::with_debug_label("focus-of-inner"),
        scope: FocusScopeNode::with_debug_label("focus-of-scope"),
        found_node: Rc::new(RefCell::new(None)),
        found_scope: Rc::new(RefCell::new(None)),
    }
}

/// A presentation always has the standard traversal `Focus`: a bare app
/// subtree resolves it through `Focus::maybe_of`, while
/// `FocusScope::of` resolves its enclosing root scope.
#[test]
fn bare_presentation_resolves_default_focus_and_root_scope() {
    let host = focus_of_host(FocusOfShape::Bare);
    let mut harness = mount(host.clone());
    let manager = harness.focus_manager();

    let resolved_node = host
        .found_node
        .borrow()
        .clone()
        .expect("FocusRoot installs the standard traversal Focus");
    assert_eq!(resolved_node.debug_label(), Some("Shortcuts"));
    let resolved_scope = host
        .found_scope
        .borrow()
        .clone()
        .expect("the probe's build must have run");
    assert!(
        Rc::ptr_eq(&resolved_scope, manager.root_scope()),
        "the presentation traversal Focus belongs to the root scope"
    );

    harness.swap_root(FocusOfHost {
        show: false,
        ..host
    });
}

/// A descendant's `Focus::maybe_of` resolves the one enclosing `Focus`'s
/// own node.
#[test]
fn focus_maybe_of_returns_the_nearest_enclosing_focus_node() {
    let host = focus_of_host(FocusOfShape::OneFocus);
    let mut harness = mount(host.clone());

    let resolved = host
        .found_node
        .borrow()
        .clone()
        .expect("Focus::maybe_of must find the enclosing Focus's node");
    assert!(
        Rc::ptr_eq(&resolved, &host.outer_node),
        "Focus::maybe_of must resolve THIS Focus's own node"
    );

    harness.swap_root(FocusOfHost {
        show: false,
        ..host
    });
}

/// Oracle: `'Focus.of stops at the nearest Focus widget.'`
/// (`focus_scope_test.dart`, tag `3.44.0`) — nesting two plain `Focus`
/// widgets, a descendant's lookup must resolve the INNER one, never
/// reaching past it to the outer one.
#[test]
fn focus_maybe_of_nearest_wins_over_an_outer_focus() {
    let host = focus_of_host(FocusOfShape::NestedFocus);
    let mut harness = mount(host.clone());

    let resolved = host
        .found_node
        .borrow()
        .clone()
        .expect("Focus::maybe_of must find the nearest enclosing Focus's node");
    assert!(
        Rc::ptr_eq(&resolved, &host.inner_node),
        "the NEAREST Focus must win"
    );
    assert!(
        !Rc::ptr_eq(&resolved, &host.outer_node),
        "must not resolve the outer Focus instead of the inner one"
    );

    harness.swap_root(FocusOfHost {
        show: false,
        ..host
    });
}

/// Oracle: `'Focus.of stops at the nearest Focus widget.'`
/// (`focus_scope_test.dart`, tag `3.44.0`) — the `Focus.maybeOf(element2),
/// isNull` assertion: a bare enclosing `FocusScope` (no plain `Focus` in
/// between) does not satisfy `Focus::maybe_of` (`scopeOk: false`), even
/// though `FocusScope::of` still resolves the scope itself.
#[test]
fn focus_maybe_of_returns_none_for_a_bare_enclosing_scope() {
    let host = focus_of_host(FocusOfShape::BareScope);
    let mut harness = mount(host.clone());

    assert!(
        host.found_node.borrow().is_none(),
        "a bare enclosing FocusScope must not satisfy Focus::maybe_of — \
             only a plain Focus counts"
    );
    let resolved_scope = host
        .found_scope
        .borrow()
        .clone()
        .expect("the probe's build must have run");
    assert!(
        Rc::ptr_eq(&resolved_scope, &host.scope),
        "FocusScope::of must still resolve the enclosing scope itself"
    );

    harness.swap_root(FocusOfHost {
        show: false,
        ..host
    });
}

/// `FocusScope::of` walks past an intervening plain `Focus` to the
/// nearest enclosing SCOPE — Flutter's `.nearestScope` — rather than
/// stopping at (or being refused by) the plain `Focus` the way
/// `Focus::maybe_of` would be.
#[test]
fn focus_scope_of_walks_up_past_a_plain_focus_to_the_nearest_scope() {
    let host = focus_of_host(FocusOfShape::ScopeThenFocus);
    let mut harness = mount(host.clone());

    let resolved_node = host
        .found_node
        .borrow()
        .clone()
        .expect("Focus::maybe_of must find the plain Focus between the scope and the probe");
    assert!(Rc::ptr_eq(&resolved_node, &host.outer_node));
    let resolved_scope = host
        .found_scope
        .borrow()
        .clone()
        .expect("the probe's build must have run");
    assert!(
        Rc::ptr_eq(&resolved_scope, &host.scope),
        "FocusScope::of must walk past the plain Focus to the enclosing scope"
    );

    harness.swap_root(FocusOfHost {
        show: false,
        ..host
    });
}

/// A stateless leaf that runs an arbitrary `on_build` closure once —
/// mirrors `overlay/tests.rs`'s own `Peek`, kept file-local since only
/// this one test needs a caller-supplied closure (the others above reuse
/// `FocusOfHost`/`FocusOfProbe`).
#[derive(Clone)]
struct Peek<F: Fn(&dyn BuildContext) + Clone + 'static>(F);

impl<F: Fn(&dyn BuildContext) + Clone + 'static> View for Peek<F> {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl<F: Fn(&dyn BuildContext) + Clone + 'static> StatelessView for Peek<F> {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        (self.0)(ctx);
        SizedBox::new(1.0, 1.0)
    }
}

/// `FocusRoot` makes `Focus::of` total for every normal presentation by
/// installing the standard traversal focus above application content.
#[test]
fn focus_of_resolves_the_presentation_traversal_focus() {
    let resolved: Rc<RefCell<Option<Rc<FocusNode>>>> = Rc::new(RefCell::new(None));
    let resolved_for_probe = Rc::clone(&resolved);
    let probe = Peek(move |ctx: &dyn BuildContext| {
        let _prev = resolved_for_probe.borrow_mut().replace(Focus::of(ctx));
    });

    let _harness = mount(probe);

    let node = resolved
        .borrow()
        .clone()
        .expect("the probe's build resolves the root traversal Focus");
    assert_eq!(node.debug_label(), Some("Shortcuts"));
}

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
#[test]
fn tab_traversal_follows_geometry_not_attach_order() {
    let scope = FocusScopeNode::with_debug_label("traversal-scope");
    let a = FocusNode::with_debug_label("a-middle");
    let b = FocusNode::with_debug_label("b-top");
    let c = FocusNode::with_debug_label("c-bottom");

    let positioned = |top: f32, node: &Rc<FocusNode>| {
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
        b.rect().min_y().0,
        0.0,
        "sanity: the provider measures committed layout"
    );
    assert_eq!(a.rect().min_y().0, 50.0);

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
