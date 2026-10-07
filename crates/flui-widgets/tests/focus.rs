//! [`Focus`], [`FocusScope`] and [`ExcludeFocus`] against a mounted tree:
//! attachment, autofocus, focus-change callbacks, exclusion and
//! geometry-ordered traversal.

use std::rc::Rc;

use flui_interaction::routing::{FocusNode, FocusScopeNode};
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
        let inner = FocusTraversalGroup::new(traversal_field(&source, 0.0, 0.0))
            .policy(Rc::new(CountingPolicy(Rc::clone(&calls))))
            .edge_behavior(TraversalEdgeBehavior::ParentScope);
        let outer = FocusTraversalGroup::new(
            FocusScope::new(inner).edge_behavior(TraversalEdgeBehavior::ParentScope),
        )
        .edge_behavior(edge);
        let harness = mount(Stack::new(vec![
            outer.into_view().boxed(),
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
    use flui_platform_api::{
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

fn tab_event(backward: bool) -> flui_platform_api::keyboard::KeyEvent {
    use flui_platform_api::{
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
