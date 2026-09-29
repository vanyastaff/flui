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
            focus = focus.on_focus_change(move |cx, focused| handler(cx, focused));
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
        on_focus_change: Some(Rc::new(move |_cx, focused| {
            recorded.borrow_mut().push(focused);
        })),
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
            let handler = Rc::clone(handler);
            focus = focus.on_key_event(move |_cx, event| handler(event));
        }
        if let Some(handler) = &self.on_focus_change {
            let handler = Rc::clone(handler);
            focus = focus.on_focus_change(move |cx, focused| handler(cx, focused));
        }
        FocusScope::with_external_node(Rc::clone(&self.scope), focus)
            .into_view()
            .boxed()
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

// ------------------------------------------------------------------
// Focus::of / Focus::maybe_of / FocusScope::of
// ------------------------------------------------------------------

/// Which tree shape a [`FocusOfProbe`] is mounted under — one reusable
/// host below instead of a bespoke type per shape.
#[derive(Clone, Copy)]
enum FocusOfShape {
    /// A single plain `Focus` directly wrapping the probe.
    OneFocus,
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
            FocusOfShape::OneFocus => Focus::new(probe)
                .focus_node(Rc::clone(&self.outer_node))
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
        found_node: Rc::new(RefCell::new(None)),
        found_scope: Rc::new(RefCell::new(None)),
    }
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
mod event_cx {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
    use flui_interaction::routing::{FocusNode, KeyEventResult};
    use flui_view::prelude::*;
    use flui_widgets::SizedBox;
    use flui_widgets::interaction::Focus;

    use crate::common::harness::mount;
    use crate::common::{ProbeSignals, SignalProbe};

    fn key_a() -> KeyEvent {
        KeyEvent {
            state: KeyState::Down,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            ..KeyEvent::default()
        }
    }

    #[test]
    fn replacing_a_focused_node_delivers_a_writable_loss_and_new_gain() {
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

    #[test]
    fn a_focus_edge_writes_a_signal_and_rebuilds_its_reader() {
        let node = FocusNode::with_debug_label("probe");
        let probe_node = Rc::clone(&node);
        let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
            Focus::new(SizedBox::new(10.0, 10.0))
                .focus_node(Rc::clone(&probe_node))
                .on_focus_change(move |cx, focused| count.set(cx, u32::from(focused)))
        });
        let mut harness = mount(probe.view());

        node.request_focus();
        assert_eq!(probe.value(), Ok(1), "the gained edge wrote");
        harness.tick();
        assert_eq!(probe.reads().last(), Some(&1), "the reader rebuilt");

        node.unfocus();
        assert_eq!(probe.value(), Ok(0), "the lost edge wrote");
    }

    #[test]
    fn a_key_handler_writes_a_signal_and_keeps_its_decision() {
        let node = FocusNode::with_debug_label("probe");
        let probe_node = Rc::clone(&node);
        let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
            Focus::new(SizedBox::new(10.0, 10.0))
                .focus_node(Rc::clone(&probe_node))
                .on_key_event(move |cx, _event| {
                    count.update(cx, |n| *n += 1).report();
                    KeyEventResult::Handled
                })
        });
        let harness = mount(probe.view());
        node.request_focus();

        assert!(
            harness.focus_manager().dispatch_key_event(&key_a()),
            "the handler's Handled consumed the key"
        );
        assert_eq!(probe.value(), Ok(1));
    }

    #[test]
    fn a_refused_write_in_a_focus_edge_is_reported_not_panicked() {
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
