//! [`Focus`], [`FocusScope`] and [`ExcludeFocus`] against a mounted tree:
//! attachment, autofocus, focus-change callbacks, exclusion and
//! geometry-ordered traversal.

use std::cell::Cell;
use std::rc::Rc;

use flui_interaction::focus::{FocusNode, FocusScopeNode};
use flui_view::ViewExt;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::interaction::{Focus, FocusChangeHandler, FocusScope};
use flui_widgets::{Directionality, Positioned, SizedBox, Stack};

use crate::common::harness::mount;

fn traversal_field(node: &Rc<FocusNode>, x: f64, y: f64) -> BoxedView {
    Positioned::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(node)))
        .left(x)
        .top(y)
        .width(10.0)
        .height(10.0)
        .into_view()
        .boxed()
}

pub(crate) fn traversal_groups_order_blocks_without_creating_focus_scopes() {
    use flui_interaction::{FocusTraversalPolicy, ReadingOrderPolicy, TraversalEdgeBehavior};
    use flui_painting::typography::TextDirection;
    use flui_widgets::interaction::FocusTraversalGroup;
    #[derive(Debug)]
    struct Reverse;
    impl FocusTraversalPolicy for Reverse {
        fn order(&self, nodes: &mut [Rc<FocusNode>], direction: TextDirection) {
            ReadingOrderPolicy.order(nodes, direction);
            nodes.reverse();
        }
    }
    let scope = FocusScopeNode::new();
    let nodes: Vec<_> = (0..4).map(|_| FocusNode::new()).collect();
    let group = FocusTraversalGroup::new(Stack::new(vec![
        traversal_field(&nodes[1], 20.0, 0.0),
        traversal_field(&nodes[2], 40.0, 0.0),
    ]))
    .policy(Rc::new(Reverse))
    .edge_behavior(TraversalEdgeBehavior::ParentScope);
    let harness = mount(FocusScope::with_external_node(
        Rc::clone(&scope),
        Stack::new(vec![
            traversal_field(&nodes[0], 0.0, 0.0),
            group.into_view().boxed(),
            traversal_field(&nodes[3], 60.0, 0.0),
        ]),
    ));
    assert!(
        Rc::ptr_eq(&nodes[1].enclosing_scope().expect("existing scope"), &scope),
        "a policy group does not introduce focus history or autofocus scope semantics"
    );
    let manager = harness.focus_manager();
    let _ = nodes[0].request_focus();
    for index in [2, 1, 3, 0] {
        assert!(manager.dispatch_key_event(&tab_event(false)).is_handled());
        assert!(
            nodes[index].has_primary_focus(),
            "forward group target {index}"
        );
    }
    for index in [3, 1, 2, 0] {
        assert!(manager.dispatch_key_event(&tab_event(true)).is_handled());
        assert!(
            nodes[index].has_primary_focus(),
            "reverse group target {index}"
        );
    }
}

pub(crate) fn nested_scope_edges_visit_the_containing_group_and_reuse_policy_order() {
    use flui_interaction::{
        FocusTraversalPolicy, KeyEventResult, ReadingOrderPolicy, TraversalEdgeBehavior,
    };
    use flui_painting::typography::TextDirection;
    use flui_widgets::interaction::FocusTraversalGroup;
    use std::cell::Cell;
    #[derive(Debug)]
    struct CountingPolicy(Rc<Cell<usize>>);
    impl FocusTraversalPolicy for CountingPolicy {
        fn order(&self, nodes: &mut [Rc<FocusNode>], direction: TextDirection) {
            self.0.set(self.0.get() + 1);
            ReadingOrderPolicy.order(nodes, direction);
        }
    }
    for edge in [
        TraversalEdgeBehavior::Stop,
        TraversalEdgeBehavior::ParentScope,
    ] {
        let source = FocusNode::new();
        let outside = FocusNode::new();
        let calls = Rc::new(Cell::new(0));
        let inner = FocusTraversalGroup::new(
            Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&source)),
        )
        .policy(Rc::new(CountingPolicy(Rc::clone(&calls))))
        .edge_behavior(TraversalEdgeBehavior::ParentScope);
        let outer = FocusTraversalGroup::new(
            FocusScope::new(inner).edge_behavior(TraversalEdgeBehavior::ParentScope),
        )
        .edge_behavior(edge);
        let harness = mount(Stack::new(vec![
            Positioned::new(outer)
                .left(0.0)
                .top(0.0)
                .width(10.0)
                .height(10.0)
                .into_view()
                .boxed(),
            traversal_field(&outside, 100.0, 0.0),
        ]));
        let manager = harness.focus_manager();
        let _ = source.request_focus();
        let result = manager.dispatch_key_event(&tab_event(false));
        assert_eq!(
            calls.get(),
            1,
            "parent retries reuse the inner group's in-flight policy order"
        );
        match edge {
            TraversalEdgeBehavior::Stop => {
                assert_eq!(result, KeyEventResult::SkipRemainingHandlers);
                assert!(source.has_primary_focus());
            }
            TraversalEdgeBehavior::ParentScope => {
                assert_eq!(result, KeyEventResult::Handled);
                assert!(outside.has_primary_focus());
            }
            _ => unreachable!(),
        }
    }
}

pub(crate) fn typed_focus_overrides_fall_back_after_target_invalidation() {
    use flui_interaction::FocusTraversalOverrides;
    let nodes = [FocusNode::new(), FocusNode::new(), FocusNode::new()];
    let scope = FocusScopeNode::new();
    let first = Positioned::new(
        Focus::new(SizedBox::new(10.0, 10.0))
            .focus_node(Rc::clone(&nodes[0]))
            .traversal_overrides(FocusTraversalOverrides::default().with_next(&nodes[2])),
    )
    .left(0.0)
    .top(0.0)
    .width(10.0)
    .height(10.0)
    .into_view()
    .boxed();
    let harness = mount(FocusScope::with_external_node(
        scope,
        Stack::new(vec![
            first,
            traversal_field(&nodes[1], 20.0, 0.0),
            traversal_field(&nodes[2], 40.0, 0.0),
        ]),
    ));
    let manager = harness.focus_manager();
    let _ = nodes[0].request_focus();
    assert!(manager.dispatch_key_event(&tab_event(false)).is_handled());
    assert!(
        nodes[2].has_primary_focus(),
        "explicit weak target wins over reading order"
    );
    nodes[2].set_can_request_focus(false);
    let _ = nodes[0].request_focus();
    assert!(manager.dispatch_key_event(&tab_event(false)).is_handled());
    assert!(
        nodes[1].has_primary_focus(),
        "disabled override falls back to policy"
    );
    let foreign = FocusNode::new();
    let foreign_manager = flui_interaction::FocusManager::new();
    let _attachment = foreign_manager
        .root_scope()
        .attach_node(&foreign)
        .expect("foreign attachment");
    let _replacement = nodes[0]
        .register_traversal_overrides(FocusTraversalOverrides::default().with_next(&foreign));
    let _ = nodes[0].request_focus();
    assert!(manager.dispatch_key_event(&tab_event(false)).is_handled());
    assert!(nodes[1].has_primary_focus());
    assert!(
        !foreign.has_primary_focus(),
        "an override cannot cross presentation ownership"
    );
}

pub(crate) fn arrow_traversal_prefers_the_beam_and_respects_group_edges() {
    use flui_interaction::{KeyEventResult, TraversalEdgeBehavior};
    use flui_platform::{
        EventTime,
        keyboard::{Code, Key, KeyEvent, KeyState, NamedKey},
    };
    use flui_widgets::interaction::FocusTraversalGroup;
    let origin = FocusNode::new();
    let diagonal = FocusNode::new();
    let beam = FocusNode::new();
    let outside = FocusNode::new();
    let group = FocusTraversalGroup::new(Stack::new(vec![
        traversal_field(&origin, 0.0, 0.0),
        traversal_field(&diagonal, 20.0, 30.0),
        traversal_field(&beam, 80.0, 0.0),
    ]))
    .edge_behavior(TraversalEdgeBehavior::Stop);
    let harness = mount(Stack::new(vec![
        group.into_view().boxed(),
        traversal_field(&outside, 120.0, 0.0),
    ]));
    let manager = harness.focus_manager();
    let right = KeyEvent::new(
        KeyState::Down,
        Key::Named(NamedKey::ArrowRight),
        Code::ArrowRight,
        EventTime::from_nanos(0),
    );
    let left = KeyEvent::new(
        KeyState::Down,
        Key::Named(NamedKey::ArrowLeft),
        Code::ArrowLeft,
        EventTime::from_nanos(0),
    );
    let _ = origin.request_focus();
    assert_eq!(manager.dispatch_key_event(&right), KeyEventResult::Handled);
    assert!(
        beam.has_primary_focus(),
        "beam overlap beats a closer diagonal candidate"
    );
    assert_eq!(
        manager.dispatch_key_event(&right),
        KeyEventResult::SkipRemainingHandlers
    );
    assert!(
        beam.has_primary_focus(),
        "Stop does not leave the group or consume native default handling"
    );
    assert_eq!(manager.dispatch_key_event(&left), KeyEventResult::Handled);
    assert!(origin.has_primary_focus());
}

pub(crate) fn widget_scope_edge_configuration_reaches_the_tab_path() {
    use flui_interaction::{KeyEventResult, TraversalEdgeBehavior};
    for edge in [
        TraversalEdgeBehavior::Stop,
        TraversalEdgeBehavior::LeaveView,
        TraversalEdgeBehavior::ClosedLoop,
    ] {
        let nodes = [FocusNode::new(), FocusNode::new()];
        let harness = mount(
            FocusScope::new(Stack::new(vec![
                traversal_field(&nodes[0], 0.0, 0.0),
                traversal_field(&nodes[1], 20.0, 0.0),
            ]))
            .edge_behavior(edge),
        );
        let manager = harness.focus_manager();
        let _ = nodes[1].request_focus();
        let result = manager.dispatch_key_event(&tab_event(false));
        match edge {
            TraversalEdgeBehavior::ClosedLoop => {
                assert_eq!(result, KeyEventResult::Handled);
                assert!(nodes[0].has_primary_focus());
            }
            TraversalEdgeBehavior::Stop => {
                assert_eq!(result, KeyEventResult::SkipRemainingHandlers);
                assert!(nodes[1].has_primary_focus());
            }
            TraversalEdgeBehavior::LeaveView => {
                assert_eq!(result, KeyEventResult::SkipRemainingHandlers);
                assert!(manager.primary_focus().is_none());
            }
            TraversalEdgeBehavior::ParentScope => unreachable!(),
        }
    }
}

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
    // Disposal publishes nothing; the frame's settling step moves the focus
    // to a successor, and with no candidate left it releases it.
    assert!(
        manager.primary_focus().is_none(),
        "a disposed focused widget releases the primary focus"
    );
}

/// A keyed focusable field: its key keeps its element, and so its widget-owned
/// focus node, across a rebuild that reorders the fields.
#[derive(Clone)]
struct Field {
    key: flui_foundation::ValueKey<&'static str>,
    on_focus_change: Option<FocusChangeHandler>,
}

impl Field {
    fn new(name: &'static str) -> Self {
        Self {
            key: flui_foundation::ValueKey::new(name),
            on_focus_change: None,
        }
    }
}

impl View for Field {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.key)
    }
}

impl StatelessView for Field {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let mut focus = Focus::new(SizedBox::new(10.0, 10.0)).debug_label(self.key.value());
        if let Some(handler) = &self.on_focus_change {
            let handler = Rc::clone(handler);
            focus = focus.on_focus_change(move |cx, focused| handler(cx, focused));
        }
        focus
    }
}

/// The fields of one screen, top to bottom.
#[derive(Clone)]
struct Fields(Vec<&'static str>);

impl View for Fields {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for Fields {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        flui_widgets::Column::new(
            self.0
                .iter()
                .map(|&name| Field::new(name).into_view().boxed())
                .collect::<Vec<_>>(),
        )
    }
}

/// Where focus is after `after` replaces `before` with the field named
/// `focused` in focus, and every focus edge the replacement published, by
/// debug label.
type Edge = (Option<String>, Option<String>);

fn focus_after_rebuild(
    before: &[&'static str],
    focused: &str,
    after: &[&'static str],
) -> (Option<String>, Vec<Edge>) {
    let label = |node: Option<Rc<FocusNode>>| node.and_then(|n| n.debug_label().map(String::from));
    let mut harness = mount(Fields(before.to_vec()));
    let manager = harness.focus_manager();
    while label(manager.primary_focus()).as_deref() != Some(focused) {
        assert!(manager.focus_next(), "Tab reaches {focused}");
    }
    let edges = Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = Rc::clone(&edges);
    let _listener = manager.add_listener(Rc::new(move |old, new| {
        seen.borrow_mut().push((label(old), label(new)));
    }));
    harness.swap_root(Fields(after.to_vec()));
    let edges = edges.borrow().clone();
    (label(manager.primary_focus()), edges)
}

pub(crate) fn rebuild_with_the_same_keys_keeps_focus() {
    let (focused, edges) = focus_after_rebuild(&["a", "b", "c"], "b", &["c", "a", "b"]);
    assert_eq!(focused.as_deref(), Some("b"));
    assert!(edges.is_empty(), "no edge for a kept field: {edges:?}");
}

pub(crate) fn removal_moves_focus_to_the_next_field() {
    let (focused, edges) = focus_after_rebuild(&["a", "b", "c"], "b", &["a", "c"]);
    assert_eq!(focused.as_deref(), Some("c"));
    assert_eq!(
        edges,
        [(Some("b".to_owned()), Some("c".to_owned()))],
        "one edge, never through None"
    );
}

pub(crate) fn removing_the_last_field_moves_focus_to_the_first() {
    let (focused, edges) = focus_after_rebuild(&["a", "b", "c"], "c", &["a", "b"]);
    assert_eq!(focused.as_deref(), Some("a"));
    assert_eq!(edges, [(Some("c".to_owned()), Some("a".to_owned()))]);
}

pub(crate) fn reload_of_every_key_publishes_one_edge() {
    let (focused, edges) = focus_after_rebuild(&["a", "b", "c"], "b", &["d", "e", "f"]);
    assert!(
        matches!(focused.as_deref(), Some("d" | "e" | "f")),
        "focus lands on a reloaded field, got {focused:?}"
    );
    assert_eq!(edges.len(), 1, "one edge for the whole reload: {edges:?}");
    assert_eq!(edges[0].0.as_deref(), Some("b"));
    assert_eq!(edges[0].1, focused);
}

pub(crate) fn removing_every_field_releases_focus() {
    let (focused, edges) = focus_after_rebuild(&["a", "b"], "a", &[]);
    assert_eq!(focused, None);
    assert_eq!(edges, [(Some("a".to_owned()), None)]);
}

/// The successor's focus callback writes state; the frame that removed the
/// focused field rebuilds that state's reader before it paints, once: when
/// that rebuild removes the successor too, the next frame moves focus on.
pub(crate) fn the_successor_rebuilds_in_the_frame_that_moved_focus() {
    use crate::common::{ProbeSignals, SignalProbe};
    use std::cell::Cell;

    let show_first = Rc::new(Cell::new(true));
    let shown = Rc::clone(&show_first);
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        let mut second = Field::new("b");
        // Only the gain writes: teardown's loss arrives after the owner closed.
        second.on_focus_change = Some(Rc::new(move |cx, focused| {
            if focused {
                count
                    .set(cx, 1)
                    .expect("the focus edge may write the probe signal");
            }
        }));
        let mut fields = Vec::new();
        if shown.get() {
            fields.push(Field::new("a").into_view().boxed());
        }
        fields.push(second.into_view().boxed());
        flui_widgets::Column::new(fields)
    });
    let mut harness = mount(probe.view());
    let manager = harness.focus_manager();
    assert!(manager.focus_next());
    assert_eq!(probe.reads(), [0]);

    show_first.set(false);
    harness.swap_root(probe.view());
    assert_eq!(
        probe.reads(),
        [0, 0, 1],
        "one frame: the removal's build, then the successor's rebuild"
    );

    // The successor's rebuild removes the successor itself: the frame has had
    // its one repeat, so focus leaves it only in the next frame.
    let label = |node: Option<Rc<FocusNode>>| node.and_then(|n| n.debug_label().map(String::from));
    let (show_a, show_b) = (Rc::new(Cell::new(true)), Rc::new(Cell::new(true)));
    let (shown_a, shown_b) = (Rc::clone(&show_a), Rc::clone(&show_b));
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        let mut fields = Vec::new();
        if shown_a.get() {
            fields.push(Field::new("a").into_view().boxed());
        }
        if shown_b.get() {
            let mut second = Field::new("b");
            let hide = Rc::clone(&shown_b);
            second.on_focus_change = Some(Rc::new(move |cx, focused| {
                if focused && hide.replace(false) {
                    count
                        .set(cx, 1)
                        .expect("the focus edge may write the probe signal");
                }
            }));
            fields.push(second.into_view().boxed());
        }
        fields.push(Field::new("c").into_view().boxed());
        flui_widgets::Column::new(fields)
    });
    let mut harness = mount(probe.view());
    let manager = harness.focus_manager();
    assert!(manager.focus_next());
    assert_eq!(label(manager.primary_focus()).as_deref(), Some("a"));
    let edges = Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = Rc::clone(&edges);
    let _listener = manager.add_listener(Rc::new(move |old, new| {
        seen.borrow_mut().push((label(old), label(new)));
    }));
    let pair = |old: &str, new: &str| (Some(old.to_owned()), Some(new.to_owned()));

    show_a.set(false);
    harness.swap_root(probe.view());
    assert!(!show_b.get(), "the successor's focus callback removed it");
    assert_eq!(
        *edges.borrow(),
        [pair("a", "b")],
        "the frame repeats once: the successor's own removal waits"
    );
    harness.tick();
    assert_eq!(
        *edges.borrow(),
        [pair("a", "b"), pair("b", "c")],
        "the next frame moves focus on"
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

    let _ = a.request_focus();

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

fn reading_order_tree(
    direction: flui_painting::typography::TextDirection,
    scope: &Rc<FocusScopeNode>,
    nodes: &[Rc<FocusNode>],
    geometry: &[(f64, f64, f64, f64)],
) -> Directionality {
    assert_eq!(
        nodes.len(),
        geometry.len(),
        "each focus stop has real positioned geometry"
    );
    let fields: Vec<_> = nodes
        .iter()
        .zip(geometry)
        .map(|(node, &(left, top, width, height))| {
            Positioned::new(Focus::new(SizedBox::new(width, height)).focus_node(node.clone()))
                .left(left)
                .top(top)
                .width(width)
                .height(height)
                .boxed()
        })
        .collect();
    Directionality::new(
        direction,
        FocusScope::with_external_node(scope.clone(), Stack::new(fields)),
    )
}

fn tab_event(backward: bool) -> flui_platform::keyboard::KeyEvent {
    use flui_platform::{
        EventTime,
        keyboard::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey},
    };
    KeyEvent::new(
        KeyState::Down,
        Key::Named(NamedKey::Tab),
        Code::Tab,
        EventTime::from_nanos(0),
    )
    .with_modifiers(if backward {
        Modifiers::SHIFT
    } else {
        Modifiers::NONE
    })
}

fn assert_widget_reading_order(
    direction: flui_painting::typography::TextDirection,
    geometry: &[(f64, f64, f64, f64)],
    expected: &[usize],
) {
    let scope = FocusScopeNode::with_debug_label("spatial-tab-scope");
    let nodes: Vec<_> = (0..geometry.len())
        .map(|index| FocusNode::with_debug_label(format!("spatial-stop-{index}")))
        .collect();
    let harness = mount(reading_order_tree(direction, &scope, &nodes, geometry));
    let manager = harness.focus_manager();
    let order = harness.enter_owner_scope(|| scope.sorted_traversal_order(None));
    assert_eq!(
        order.iter().map(|node| node.id()).collect::<Vec<_>>(),
        expected
            .iter()
            .map(|&index| nodes[index].id())
            .collect::<Vec<_>>(),
        "the mounted geometry determines the complete order, including its first stop"
    );
    let _ = harness.enter_owner_scope(|| nodes[expected[0]].request_focus());
    for &next in expected.iter().skip(1).chain(expected.iter().take(1)) {
        assert!(
            harness
                .enter_owner_scope(|| manager.dispatch_key_event(&tab_event(false)).is_handled()),
            "Tab is consumed by the mounted action chain"
        );
        assert!(
            nodes[next].has_primary_focus(),
            "Tab lands on spatial stop {next}"
        );
    }
    for &previous in expected.iter().rev() {
        assert!(
            harness.enter_owner_scope(|| manager.dispatch_key_event(&tab_event(true)).is_handled()),
            "Shift+Tab is consumed by the mounted action chain"
        );
        assert!(
            nodes[previous].has_primary_focus(),
            "Shift+Tab reverses the same order at spatial stop {previous}"
        );
    }
}

pub(crate) fn tab_groups_vertically_overlapping_widgets_into_one_reading_row() {
    assert_widget_reading_order(
        flui_painting::typography::TextDirection::Ltr,
        &[
            (0.0, 1.0, 10.0, 10.0),
            (30.0, 0.0, 10.0, 10.0),
            (0.0, 30.0, 10.0, 10.0),
        ],
        &[0, 1, 2],
    );
}

pub(crate) fn tab_reads_an_rtl_scope_from_its_inherited_directionality() {
    assert_widget_reading_order(
        flui_painting::typography::TextDirection::Rtl,
        &[
            (0.0, 0.0, 10.0, 10.0),
            (30.0, 0.0, 10.0, 10.0),
            (0.0, 30.0, 10.0, 10.0),
        ],
        &[1, 0, 2],
    );
}

pub(crate) fn a_tall_widget_cannot_bridge_disjoint_reading_rows() {
    assert_widget_reading_order(
        flui_painting::typography::TextDirection::Ltr,
        &[
            (30.0, 0.0, 10.0, 30.0),
            (0.0, 1.0, 10.0, 9.0),
            (10.0, 20.0, 10.0, 10.0),
        ],
        &[1, 0, 2],
    );
}

pub(crate) fn spatial_tab_preserves_geometric_ties_and_row_boundaries() {
    use flui_painting::typography::TextDirection::{Ltr, Rtl};
    for (case, direction, geometry, expected) in [
        (
            "fractional vertical intersection",
            Ltr,
            &[
                (30.0, 0.25, 10.0, 0.5),
                (0.0, 0.5, 10.0, 0.5),
                (0.0, 2.0, 10.0, 1.0),
            ][..],
            &[1, 0, 2][..],
        ),
        (
            "touching row boundaries",
            Ltr,
            &[
                (30.0, 0.0, 10.0, 10.0),
                (0.0, 10.0, 10.0, 10.0),
                (60.0, 0.0, 10.0, 10.0),
            ][..],
            &[0, 2, 1][..],
        ),
        (
            "rtl leading right edge",
            Rtl,
            &[
                (0.0, 0.0, 80.0, 10.0),
                (50.0, 0.0, 20.0, 10.0),
                (0.0, 30.0, 10.0, 10.0),
            ][..],
            &[0, 1, 2][..],
        ),
        (
            "equal leading edge at different tops",
            Ltr,
            &[
                (30.0, 1.0, 10.0, 10.0),
                (30.0, 0.0, 10.0, 10.0),
                (0.0, 30.0, 10.0, 10.0),
            ][..],
            &[0, 1, 2][..],
        ),
        (
            "zero-sized fallback",
            Ltr,
            &[
                (0.0, -10.0, 0.0, 10.0),
                (40.0, 0.0, 10.0, 10.0),
                (0.0, 0.0, 10.0, 10.0),
                (0.0, 30.0, 10.0, 10.0),
            ][..],
            &[2, 1, 3, 0][..],
        ),
        (
            "signed-zero geometric tie",
            Ltr,
            &[
                (0.0, 0.0, 10.0, 10.0),
                (-0.0, 0.0, 10.0, 10.0),
                (30.0, 0.0, 10.0, 10.0),
            ][..],
            &[0, 1, 2][..],
        ),
    ] {
        let result = std::panic::catch_unwind(|| {
            assert_widget_reading_order(direction, geometry, expected);
        });
        assert!(result.is_ok(), "spatial geometry case failed: {case}");
    }
}

pub(crate) fn a_directionality_update_changes_tab_order_without_replacing_focus_nodes() {
    use flui_painting::typography::TextDirection::{Ltr, Rtl};
    let scope = FocusScopeNode::with_debug_label("changing-direction-tab-scope");
    let nodes = [
        FocusNode::with_debug_label("left"),
        FocusNode::with_debug_label("middle"),
        FocusNode::with_debug_label("right"),
    ];
    let geometry = [
        (0.0, 0.0, 10.0, 10.0),
        (30.0, 0.0, 10.0, 10.0),
        (60.0, 0.0, 10.0, 10.0),
    ];
    let mut harness = mount(reading_order_tree(Ltr, &scope, &nodes, &geometry));
    let manager = harness.focus_manager();
    let _ = harness.enter_owner_scope(|| nodes[0].request_focus());
    assert!(
        harness.enter_owner_scope(|| manager.dispatch_key_event(&tab_event(false)).is_handled())
    );
    assert!(nodes[1].has_primary_focus());
    harness.swap_root(reading_order_tree(Rtl, &scope, &nodes, &geometry));
    assert!(
        nodes[1].has_primary_focus(),
        "the focused external node survives the inherited update"
    );
    assert!(
        harness.enter_owner_scope(|| manager.dispatch_key_event(&tab_event(false)).is_handled())
    );
    assert!(
        nodes[0].has_primary_focus(),
        "the existing scope consumes the new RTL direction"
    );
    assert!(
        harness.enter_owner_scope(|| manager.dispatch_key_event(&tab_event(true)).is_handled())
    );
    assert!(nodes[1].has_primary_focus());
}

/// Event context (ADR-0086): the focus edge and the key handler run inside a
/// write the `Focus` opens from the writer source it acquired in
/// `init_state`.
pub(crate) mod event_cx {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_interaction::focus::FocusNode;
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
        let _ = old_node.request_focus();
        assert_eq!(probe.value(), Ok(1));

        *selected.borrow_mut() = Rc::clone(&new_node);
        harness.swap_root(probe.view());
        assert!(!old_node.has_focus());
        assert_eq!(probe.value(), Ok(0), "replacement delivered its loss");
        let _ = new_node.request_focus();
        assert_eq!(probe.value(), Ok(1));
        assert_eq!(*edges.borrow(), [true, false, true]);
        let _ = old_node.request_focus();
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

/// A rebuild that swaps a host's caller-owned node for one of its own while a
/// focus notification runs is refused, and the next rebuild asking for the
/// same swap makes it: the caller's node leaves the tree then. `Focus` and
/// `FocusScope` each host the node under test beside a trigger whose focus
/// gain rebuilds the tree from inside the notification.
pub(crate) fn a_refused_node_replacement_is_made_by_the_next_rebuild() {
    use std::cell::RefCell;

    use crate::common::{LaidOut, lay_out, loose};

    type Host = Rc<dyn Fn() -> BoxedView>;
    fn field() -> SizedBox {
        SizedBox::new(10.0, 10.0)
    }
    let caller_node = FocusNode::with_debug_label("caller-node");
    let caller_scope = FocusScopeNode::with_debug_label("caller-scope");
    let rows: [(&str, Host, Host, Rc<FocusNode>); 2] = [
        (
            "Focus",
            {
                let node = Rc::clone(&caller_node);
                Rc::new(move || {
                    Focus::new(field())
                        .focus_node(Rc::clone(&node))
                        .into_view()
                        .boxed()
                })
            },
            Rc::new(|| Focus::new(field()).into_view().boxed()),
            Rc::clone(&caller_node),
        ),
        (
            "FocusScope",
            {
                let scope = Rc::clone(&caller_scope);
                Rc::new(move || {
                    FocusScope::with_external_node(Rc::clone(&scope), field())
                        .into_view()
                        .boxed()
                })
            },
            Rc::new(|| FocusScope::new(field()).into_view().boxed()),
            Rc::clone(caller_scope.as_focus_node()),
        ),
    ];
    for (label, before, after, caller) in rows {
        let trigger = FocusNode::with_debug_label("trigger");
        let tree = {
            let trigger = Rc::clone(&trigger);
            move |host: &Host| {
                Stack::new(vec![
                    host(),
                    Focus::new(field())
                        .focus_node(Rc::clone(&trigger))
                        .into_view()
                        .boxed(),
                ])
            }
        };
        let laid: Rc<RefCell<Option<LaidOut>>> =
            Rc::new(RefCell::new(Some(lay_out(tree(&before), loose(100.0)))));
        let manager = laid.borrow().as_ref().expect("mounted").focus_manager();
        assert!(caller.is_attached(), "{label}: the caller's node is hosted");

        let armed = Rc::new(Cell::new(true));
        let (listener_laid, listener_armed, listener_after, listener_tree) = (
            Rc::clone(&laid),
            Rc::clone(&armed),
            Rc::clone(&after),
            tree.clone(),
        );
        manager.add_listener(Rc::new(move |_, _| {
            if listener_armed.replace(false) {
                let mut laid = listener_laid.borrow_mut();
                let laid = laid.as_mut().expect("mounted");
                laid.pump_widget(listener_tree(&listener_after));
            }
        }));
        let _ = trigger.request_focus();
        assert!(!armed.get(), "{label}: the trigger's gain rebuilt the tree");
        assert!(
            caller.is_attached(),
            "{label}: the replacement was refused inside the notification"
        );

        let mut laid = laid.borrow_mut();
        let laid = laid.as_mut().expect("mounted");
        laid.pump_widget(tree(&after));
        assert!(
            !caller.is_attached(),
            "{label}: the next rebuild asking for the swap made it"
        );
    }
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

/// Counts how often its state is created, which is how often the subtree
/// under it was torn down and mounted again.
#[derive(Clone, StatefulView)]
struct MountProbe {
    mounts: Rc<Cell<usize>>,
}

struct MountProbeState {
    mounts: Rc<Cell<usize>>,
}

impl std::fmt::Debug for MountProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MountProbe")
    }
}

impl StatefulView for MountProbe {
    type State = MountProbeState;

    fn create_state(&self) -> Self::State {
        MountProbeState {
            mounts: Rc::clone(&self.mounts),
        }
    }
}

impl ViewState<MountProbe> for MountProbeState {
    fn init_state(&mut self, _ctx: &dyn LifecycleContext) {
        self.mounts.set(self.mounts.get() + 1);
    }

    fn build(&self, _view: &MountProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// Whether a `Focus` can take focus changes nothing about the elements under
/// it: a popup route that makes the page below unfocusable, or an owner that
/// disables one field, must not tear the subtree down and mount it again
/// (which cancels a gesture in flight under it).
///
/// Red-check: wrap the child in `Semantics` only while the node can take
/// focus — the probe mounts a second time on every row.
pub(crate) fn a_focus_keeps_its_subtree_when_it_stops_being_focusable() {
    let own = FocusNode::with_debug_label("own flag");
    let page = FocusScopeNode::new();
    type Flip = Box<dyn Fn(bool)>;
    let rows: [(&str, Flip, Rc<Cell<usize>>, BoxedView); 2] = {
        let (own_mounts, page_mounts) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let own_flip = Rc::clone(&own);
        let page_flip = Rc::clone(&page);
        [
            (
                "the node's own flag",
                Box::new(move |can| own_flip.set_can_request_focus(can)),
                Rc::clone(&own_mounts),
                Focus::new(MountProbe { mounts: own_mounts })
                    .focus_node(Rc::clone(&own))
                    .into_view()
                    .boxed(),
            ),
            (
                "the page's scope above it",
                Box::new(move |can| page_flip.as_focus_node().set_can_request_focus(can)),
                Rc::clone(&page_mounts),
                FocusScope::with_external_node(
                    Rc::clone(&page),
                    Focus::new(MountProbe {
                        mounts: page_mounts,
                    }),
                )
                .into_view()
                .boxed(),
            ),
        ]
    };
    for (name, flip, mounts, view) in rows {
        let mut harness = mount(view);
        assert_eq!(mounts.get(), 1, "{name}: mounted once");
        for can in [false, true, false] {
            flip(can);
            harness.tick();
            assert_eq!(
                mounts.get(),
                1,
                "{name}: the subtree survives can_request_focus = {can}"
            );
        }
    }
}

/// A stateless root that makes its scroll controller inside `build`.
#[derive(Clone, Debug, StatelessView)]
struct ControllerInBuild;

impl StatelessView for ControllerInBuild {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        use flui_widgets::{ListView, ScrollController, Scrollable, Text};

        Scrollable::new()
            .controller(ScrollController::new())
            .viewport_builder(Rc::new(|position| {
                let rows: Vec<_> = (0..40)
                    .map(|row| {
                        SizedBox::height(50.0)
                            .child(Text::new(format!("row {row}")))
                            .into_view()
                            .boxed()
                    })
                    .collect();
                ListView::new(50.0, rows)
                    .position(position)
                    .into_view()
                    .boxed()
            }))
    }
}

/// The first focus gain, which a drag's start causes by focusing its scroll
/// view, flips `has_focus` on every enclosing scope. They update their
/// dependents and nothing else: a root that makes a scroll controller in
/// `build` is not built again under the drag, so its position is the one the
/// contact moves and every step of a held drag paints new content.
///
/// Red-check: have an element's update rebuild its child whether or not the
/// child's view is the one it already holds — the frames freeze after the
/// first step.
pub(crate) fn the_first_focus_gain_does_not_rebuild_the_app_under_a_drag() {
    use crate::common::{lay_out, tight};

    let mut laid = lay_out(ControllerInBuild, tight(400.0, 300.0));
    let row_top = |laid: &crate::common::LaidOut| {
        let row = laid.find_text("row 3").expect("the fourth row is in view");
        laid.absolute_offset(row).dy
    };
    laid.dispatch_pointer_down(200.0, 250.0);
    let mut previous = row_top(&laid);
    for step in 1..=8 {
        laid.dispatch_pointer_move(200.0, 250.0 - 15.0 * f64::from(step));
        laid.tick();
        let top = row_top(&laid);
        if step > 1 {
            assert!(
                top < previous,
                "step {step} of the held drag left the row at {top}, where step {} had it at \
                 {previous}",
                step - 1
            );
        }
        previous = top;
    }
    laid.dispatch_pointer_up(200.0, 130.0);
}

/// A frame request that panics while focus moves still lets every focus
/// widget hear its edge: the failure is reported, not unwound into the
/// listener, the losing widget's `on_focus_change` runs, and the owed wake
/// is retried.
///
/// Red-check: let a scheduled rebuild's wake unwind into its caller (the
/// losing widget never hears it lost focus).
pub(crate) fn a_failing_frame_request_still_delivers_every_focus_edge() {
    use std::cell::RefCell;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use crate::common::{lay_out, loose};
    use flui_widgets::Column;

    // An element keeps the frame-request hook of its mount, so the hook is
    // installed before the widgets mount and armed once they have.
    let armed = Arc::new(AtomicBool::new(false));
    let woken = Arc::new(AtomicUsize::new(0));
    let woke = Arc::clone(&woken);
    let mut laid = lay_out(SizedBox::new(1.0, 1.0), loose(100.0));
    let arm = Arc::clone(&armed);
    laid.with_build_owner_mut(|owner| {
        owner.set_on_build_scheduled(move || {
            assert!(!arm.load(Ordering::SeqCst), "the frame request failed");
            woke.fetch_add(1, Ordering::SeqCst);
        });
    });
    let first = FocusNode::with_debug_label("first");
    let second = FocusNode::with_debug_label("second");
    let edges = Rc::new(RefCell::new(Vec::new()));
    let field = |node: &Rc<FocusNode>, name: &'static str| {
        let edges = Rc::clone(&edges);
        Focus::new(SizedBox::new(10.0, 10.0))
            .focus_node(Rc::clone(node))
            .on_focus_change(move |_cx, focused| edges.borrow_mut().push((name, focused)))
    };
    laid.pump_widget(Column::new((
        field(&first, "first"),
        field(&second, "second"),
    )));
    laid.tick();
    let _ = first.request_focus();
    laid.tick();
    edges.borrow_mut().clear();

    armed.store(true, Ordering::SeqCst);
    let woken_before = woken.load(Ordering::SeqCst);
    let ((), log) = flui_testing::log_capture::capture(|| {
        let _ = second.request_focus();
    });
    armed.store(false, Ordering::SeqCst);
    assert!(
        log.contains("a frame request failed"),
        "the frame request's failure is reported: {log}"
    );
    let mut heard = edges.borrow().clone();
    heard.sort_unstable();
    assert_eq!(
        heard,
        [("first", false), ("second", true)],
        "both widgets heard their edge"
    );
    laid.tick();
    assert!(second.has_primary_focus());
    assert!(
        woken.load(Ordering::SeqCst) > woken_before,
        "the owed wake was retried once the hook healed"
    );
}
