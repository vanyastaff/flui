//! Widget-level coverage for the accessibility semantics wrappers.

use crate::common::{lay_out, loose};
use flui_rendering::semantics::{SemanticsAction, semantics_action_for};
use flui_widgets::{MergeSemantics, Semantics, SizedBox};

// ===========================================================================
// RenderParagraph — plain text publishes its own label
// ===========================================================================

/// Merging collapses its descendants in the **accessibility tree**, not just in
/// the render tree.
///
/// The sibling tests above assert that `RenderMergeSemantics` mounts. That
/// proves the render object exists; it says nothing about what a screen reader
/// is handed, and the two are separated by the whole assemble-and-translate
/// path. This asserts the end product.
///
/// The premise runs first and is load-bearing: without the wrapper both labels
/// must be separately findable. A test that only checked "the children are not
/// findable" would pass just as well against a tree where they never
/// contributed semantics at all — which is the ordinary state of affairs for a
/// bare `SizedBox`, and why the children here carry real `Semantics`.
pub(crate) fn merge_semantics_collapses_its_descendants_in_the_a11y_tree() {
    use flui_view::ViewExt as _;
    use flui_widgets::Column;

    let labelled = |text: &'static str| {
        Semantics::new()
            .container(true)
            .label(text)
            .child(SizedBox::new(40.0, 20.0))
    };

    // Premise: unmerged, each child is its own node.
    let mut apart = lay_out(
        Column::new((labelled("first").boxed(), labelled("second").boxed())),
        loose(200.0),
    );
    apart.enable_semantics();
    apart.pump();
    let apart_tree = apart.a11y_tree().expect("semantics enabled");
    assert!(
        apart_tree.find_by_label("first").is_ok() && apart_tree.find_by_label("second").is_ok(),
        "premise: without merging, both labels are separately findable — \
         otherwise the assertion below proves nothing. Tree was:\n{}",
        apart_tree.describe()
    );

    let mut merged = lay_out(
        MergeSemantics::new().child(Column::new((
            labelled("first").boxed(),
            labelled("second").boxed(),
        ))),
        loose(200.0),
    );
    merged.enable_semantics();
    merged.pump();
    let merged_tree = merged.a11y_tree().expect("semantics enabled");

    // The combined label is the assertion that distinguishes merging from
    // DESTROYING. "neither label is findable" plus "fewer nodes" would both
    // hold if the merge path dropped its descendants outright, or kept only
    // one of them — which is exclusion's contract, not merging's.
    assert!(
        merged_tree.find_by_label("first second").is_ok(),
        "merging must carry its descendants' labels into one node, combined \
         and in order. Tree was:\n{}",
        merged_tree.describe()
    );
    assert!(
        merged_tree.find_by_label("first").is_err() && merged_tree.find_by_label("second").is_err(),
        "and neither descendant may remain addressable on its own; a reader \
         should meet one node, not three. Tree was:\n{}",
        merged_tree.describe()
    );
    assert!(
        merged_tree.len() < apart_tree.len(),
        "the merged tree must have strictly fewer nodes than the unmerged one \
         ({} vs {})",
        merged_tree.len(),
        apart_tree.len()
    );
}

// ===========================================================================
// Actions: a platform request, routed back to the widget's own callback
// ===========================================================================

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_rendering::semantics::{AccessibilityNodeId, SemanticsActionRequest};
use flui_testing::widgets::LaidOut;
use flui_testing::{Action, ActionData, ActionRequest, NodeId, TreeId};
use flui_view::{SignalWriteExt as _, View};

use crate::common::{ProbeSignals, SignalProbe};

/// Retained overlay state remains live while its page is structurally absent
/// from both assembled semantics and the actual delivered update stream.
pub(crate) fn a_covered_retained_form_stays_absent_after_a_late_controller_update() {
    use flui_interaction::FocusNode;
    use flui_testing::A11yTree;
    use flui_testing::a11y::Role;
    use flui_view::ViewExt as _;
    use flui_widgets::{
        Column, Form, InsertPosition, Overlay, OverlayEntry, OverlayHandle, RawTextFormField, Text,
        TextEditingController,
    };
    use std::sync::{Arc, Mutex};

    let controller = TextEditingController::with_text("retained draft");
    let focus = FocusNode::new();
    let calls = Rc::new(Cell::new(0_u32));
    let lower_controller = controller.clone();
    let lower_focus = Rc::clone(&focus);
    let lower_calls = Rc::clone(&calls);
    let lower = OverlayEntry::new(move |_cx| {
        let calls = Rc::clone(&lower_calls);
        Form::new(Column::new((
            Text::new("Retained page").boxed(),
            RawTextFormField::new(lower_controller.clone())
                .focus_node(Rc::clone(&lower_focus))
                .boxed(),
            Semantics::new()
                .container(true)
                .button(true)
                .label("Back")
                .on_tap(move |_cx| calls.set(calls.get() + 1))
                .child(SizedBox::new(80.0, 40.0))
                .boxed(),
        )))
        .boxed()
    });
    lower.set_maintain_state(true);
    let upper = OverlayEntry::new(move |_cx| {
        Column::new((
            Semantics::new()
                .container(true)
                .label("Active page")
                .child(SizedBox::new(80.0, 30.0))
                .boxed(),
            Semantics::new()
                .container(true)
                .button(true)
                .label("Back")
                .on_tap(|_cx| {})
                .child(SizedBox::new(80.0, 40.0))
                .boxed(),
        ))
        .boxed()
    });
    upper.set_opaque(true);
    let overlay = OverlayHandle::new();
    overlay.insert(&lower, &InsertPosition::Top);
    let mut laid = lay_out(Overlay::new(overlay.clone()), loose(300.0));
    laid.enable_semantics();
    laid.tick();
    let delivered = Arc::new(Mutex::new(
        laid.a11y_tree().expect("semantics enabled").raw().clone(),
    ));
    let sink = Arc::clone(&delivered);
    laid.pipeline_owner().with_mut(|owner| {
        owner.set_semantics_update_callback(Arc::new(move |update| {
            let ids: std::collections::HashSet<_> =
                update.nodes.iter().map(|(id, _)| *id).collect();
            assert_eq!(
                ids.len(),
                update.nodes.len(),
                "published packet repeats an identity"
            );
            let mut current = sink.lock().expect("publication mirror lock");
            if let Some(tree) = &update.tree {
                current.tree = Some(tree.clone());
            }
            for (id, node) in &update.nodes {
                if let Some((_, previous)) = current.nodes.iter_mut().find(|(key, _)| key == id) {
                    previous.clone_from(node);
                } else {
                    current.nodes.push((*id, node.clone()));
                }
            }
            current.focus = update.focus;
            current.tree_id = update.tree_id;
        }));
    });
    let published = || A11yTree::new(delivered.lock().expect("publication mirror lock").clone());
    let before = published();
    let field = before
        .find(Role::TextInput)
        .expect("premise: lower Edit published");
    assert_eq!(field.value(), Some("retained draft"));
    assert_eq!(
        before.find_all(Role::Form).len(),
        1,
        "premise: lower Form published"
    );
    let lower_back = before
        .find_by_label("Back")
        .expect("premise: lower Back published")
        .id();
    laid.invoke_semantics_action(request(Action::Click, lower_back, None))
        .expect("lower action initially works");
    assert_eq!(calls.get(), 1);
    let bounds = field.bounds().expect("lower editor laid out");
    laid.dispatch_pointer_down(bounds.x0 + 2.0, f64::midpoint(bounds.y0, bounds.y1));
    laid.dispatch_pointer_up(bounds.x0 + 2.0, f64::midpoint(bounds.y0, bounds.y1));
    laid.tick();
    assert!(
        focus.has_primary_focus(),
        "premise: retained editor really focused"
    );

    overlay.insert(&upper, &InsertPosition::Top);
    laid.tick();
    let covered = |laid: &LaidOut| {
        for (source, tree) in [
            ("assembled", laid.a11y_tree().expect("semantics enabled")),
            ("delivered", published()),
        ] {
            assert!(
                tree.find_by_label("Active page").is_ok(),
                "{source}: active page published"
            );
            let back = tree
                .find_by_label("Back")
                .expect("one active Back, no covered duplicate");
            assert_ne!(
                back.id(),
                lower_back,
                "{source}: Back belongs to active page"
            );
            assert!(
                tree.find_all(Role::Form).is_empty(),
                "{source}: covered Form disconnected"
            );
            assert!(
                tree.find_all(Role::TextInput).is_empty(),
                "{source}: covered Edit disconnected"
            );
        }
    };
    covered(&laid);
    assert!(
        laid.invoke_semantics_action(request(Action::Click, lower_back, None))
            .is_err(),
        "covered lower action refused even though its element is retained"
    );
    // This is a real public controller notification to the retained producer,
    // not input evidence or a forced root rebuild. Its old rect must not
    // authorize republishing the covered Form's cached semantic anchor.
    controller.set_text("late retained draft");
    for _ in 0..3 {
        laid.tick();
    }
    covered(&laid);
    assert_eq!(calls.get(), 1, "refused action had no callback effect");
    upper.remove();
    laid.tick();
    for tree in [laid.a11y_tree().expect("semantics enabled"), published()] {
        assert!(
            tree.find_all_by_label("Active page").is_empty(),
            "removed upper page disconnected"
        );
        assert_eq!(
            tree.find(Role::TextInput).expect("restored Edit").value(),
            Some("late retained draft")
        );
        assert_eq!(tree.find_all(Role::Form).len(), 1);
        assert_eq!(
            tree.find_by_label("Back")
                .expect("restored lower Back")
                .id(),
            lower_back
        );
    }
    laid.invoke_semantics_action(request(Action::Click, lower_back, None))
        .expect("restored lower action works");
    assert_eq!(calls.get(), 2);
}

/// Mounts `root` with semantics enabled and returns the single node carrying
/// `label`, together with a live view of the a11y tree.
///
/// The tree is returned alongside its own description so a failing assertion
/// can show what was actually there, matching this file's other a11y tests.
fn pump_labelled(root: impl View) -> (LaidOut, flui_testing::A11yTree, NodeId) {
    let mut laid = lay_out(root, loose(200.0));
    laid.enable_semantics();
    laid.pump();
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    let node_id = labelled_node(&tree);
    (laid, tree, node_id)
}

/// The node carrying [`LABEL`] in `tree`.
fn labelled_node(tree: &flui_testing::A11yTree) -> NodeId {
    tree.find_by_label(LABEL)
        .unwrap_or_else(|error| {
            panic!(
                "expected one node labelled {LABEL:?}: {error}\n{}",
                tree.describe()
            )
        })
        .id()
}

/// A container node labelled [`LABEL`] around a fixed-size box, for the
/// action builders to decorate.
fn host() -> Semantics {
    Semantics::new().container(true).label(LABEL)
}

/// The label every action test mounts under; unique within its own tree.
const LABEL: &str = "Action Host";

/// Builds an `ActionRequest` addressed at `node_id`.
fn request(action: Action, node_id: NodeId, data: Option<ActionData>) -> ActionRequest {
    ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: node_id,
        data,
    }
}

/// A tap handler reaches its callback through a platform click request.
///
/// Two halves, and both are necessary: the node must *tell* the platform the
/// action exists, and pressing it must *do* something. A node that passes only
/// the first is the dead control this whole surface exists to rule out — an
/// action advertised outbound that nothing routes inbound.
pub(crate) fn a_tap_handler_round_trips_from_a_platform_click_to_the_callback() {
    let activations = Rc::new(Cell::new(0_u32));
    let counted = Rc::clone(&activations);

    let (laid, tree, node_id) = pump_labelled(
        host()
            .on_tap(move |_cx| counted.set(counted.get() + 1))
            .child(SizedBox::new(40.0, 20.0)),
    );

    assert!(
        tree.find_by_label(LABEL)
            .expect("node was located a moment ago")
            .supports_action(Action::Click),
        "a node with an on_tap handler must advertise a click, or no assistive \
         technology can reach it. Tree was:\n{}",
        tree.describe()
    );

    laid.invoke_semantics_action(request(Action::Click, node_id, None))
        .expect("a click on a node advertising one must resolve");

    assert_eq!(
        activations.get(),
        1,
        "the handler must have run exactly once"
    );
}

/// An action handler receives the dispatch's `EventCx`: its signal write
/// lands, and the signal's reader rebuilds on the next frame.
pub(crate) fn an_action_handler_writes_a_signal_and_rebuilds_its_reader() {
    let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
        host()
            .on_increase(move |cx| count.update(cx, |n| *n += 1))
            .child(SizedBox::new(40.0, 20.0))
    });
    let (mut laid, _tree, node_id) = pump_labelled(probe.view());

    laid.invoke_semantics_action(request(Action::Increment, node_id, None))
        .expect("an increment on a node advertising one must resolve");
    assert_eq!(probe.value(), Ok(1), "the handler wrote through its cx");

    laid.tick();
    assert_eq!(
        probe.reads().last(),
        Some(&1),
        "the reader rebuilt with the write"
    );
}

/// A handler whose write is refused (its signal's slot is released) is
/// reported at the dispatch boundary, and the next action still runs.
pub(crate) fn a_refused_write_in_an_action_handler_is_reported_not_panicked() {
    let probe = SignalProbe::new(|ProbeSignals { count, released }| {
        host()
            .on_decrease(move |cx| released.set(cx, 1))
            .on_increase(move |cx| count.set(cx, 1))
            .child(SizedBox::new(40.0, 20.0))
    });
    let (laid, _tree, node_id) = pump_labelled(probe.view());

    let (outcome, log) = flui_testing::log_capture::capture(|| {
        laid.invoke_semantics_action(request(Action::Decrement, node_id, None))
    });
    outcome.expect("a decrement on a node advertising one must resolve");
    assert!(
        log.contains("an event callback's signal write was refused"),
        "the refusal is logged at the dispatch boundary: {log}"
    );

    laid.invoke_semantics_action(request(Action::Increment, node_id, None))
        .expect("the node still resolves after a refused write");
    assert_eq!(probe.value(), Ok(1), "the next action still wrote");
}

/// A handler is owner-local, so an action invoked with no realm entered — a
/// caller holding a `SemanticsActionInvocation` outside the owner's
/// dispatch — has nowhere to run it: the action is dropped with a warning,
/// and nothing panics.
pub(crate) fn an_action_invoked_outside_its_realm_is_dropped_with_a_warning() {
    let activations = Rc::new(Cell::new(0_u32));
    let counted = Rc::clone(&activations);
    let (laid, _tree, node_id) = pump_labelled(
        host()
            .on_tap(move |_cx| counted.set(counted.get() + 1))
            .child(SizedBox::new(40.0, 20.0)),
    );

    let invocation = laid
        .pipeline_owner()
        .with(|owner| {
            owner.resolve_semantics_action(SemanticsActionRequest {
                node_id: AccessibilityNodeId::from_u64(node_id.0)
                    .expect("an exported node id is non-zero"),
                action: SemanticsAction::Tap,
                arguments: None,
            })
        })
        .expect("the node advertises a tap");
    let ((), log) = flui_testing::log_capture::capture(|| invocation.invoke());

    assert_eq!(activations.get(), 0, "the handler did not run");
    assert!(
        log.contains("a semantics action was dropped"),
        "the drop is reported: {log}"
    );

    laid.invoke_semantics_action(request(Action::Click, node_id, None))
        .expect("the same node resolves inside its realm");
    assert_eq!(activations.get(), 1, "inside the realm the handler runs");
}

/// A rebuild that hands the node a fresh closure — the ordinary case, a
/// closure literal in `build` — leaves the mounted configuration equal, so it
/// raises no semantics update, and the action runs the rebuilt closure rather
/// than the first one.
pub(crate) fn rebuilding_with_fresh_handlers_keeps_the_configuration_and_runs_the_new_one() {
    let ran = Rc::new(Cell::new(0_u32));
    let tree_for = |version: u32| {
        let ran = Rc::clone(&ran);
        host()
            .on_tap(move |_cx| ran.set(version))
            .child(SizedBox::new(40.0, 20.0))
    };
    let (mut laid, _tree, node_id) = pump_labelled(tree_for(1));
    let before = mounted_configuration(&laid);

    laid.pump_widget(tree_for(2));
    let after = mounted_configuration(&laid);
    assert!(
        before == after,
        "a rebuild that changes only the closure must keep the configuration \
         equal, or every rebuild re-assembles this node's semantics"
    );

    laid.invoke_semantics_action(request(Action::Click, node_id, None))
        .expect("the rebuilt node still resolves a click");
    assert_eq!(ran.get(), 2, "the rebuilt closure ran, not the first one");
}

/// Unmounting a node releases its action table from the owner lane, so the
/// state a handler captures is dropped with the node rather than kept until
/// the realm closes. The action is delivered through `Harness`, whose realm
/// entry is what lets the handler run at all.
pub(crate) fn unmounting_a_node_releases_its_action_table() {
    let captured = Rc::new(());
    let witness = Rc::downgrade(&captured);
    let activations = Rc::new(Cell::new(0_u32));
    let counted = Rc::clone(&activations);
    let mut harness = crate::common::harness::mount(
        host()
            .on_tap(move |_cx| {
                std::hint::black_box(&captured);
                counted.set(counted.get() + 1);
            })
            .child(SizedBox::new(40.0, 20.0)),
    );
    harness.enable_semantics();
    harness.tick();
    let tree = harness
        .a11y_tree()
        .expect("semantics enabled before the frame");
    harness
        .invoke_semantics_action(request(Action::Click, labelled_node(&tree), None))
        .expect("a click on a node advertising one must resolve");
    assert_eq!(activations.get(), 1, "the handler ran inside the realm");

    harness.swap_root(SizedBox::shrink());
    assert!(
        witness.upgrade().is_none(),
        "the unmounted node's handler, and what it captured, must leave the \
         owner lane with the node"
    );
}

/// A node mounted with no owner advertises none of its actions: nothing
/// could run them, and an advertised action nothing runs is a dead control.
pub(crate) fn a_detached_mount_advertises_no_actions() {
    use flui_view::RenderView as _;

    let render = host()
        .on_tap(|_cx| {})
        .create_render_object(&flui_view::RenderObjectContext::detached());
    let configuration = render.configuration();
    assert!(
        configuration.label().is_some(),
        "the detached node still carries its own configuration"
    );
    assert!(
        !configuration.has_action(SemanticsAction::Tap),
        "a detached node must not advertise a tap it cannot run"
    );
}

/// The configuration the single `Semantics` wrapper has mounted.
fn mounted_configuration(laid: &LaidOut) -> flui_rendering::semantics::SemanticsConfiguration {
    let [id] = laid.find_semantics_wrappers()[..] else {
        panic!("one Semantics wrapper is mounted");
    };
    laid.pipeline_owner().with_mut(|owner| {
        owner
            .render_tree_mut()
            .get_mut(id)
            .and_then(|node| {
                node.downcast_render_object_mut::<flui_objects::RenderSemanticsAnnotations>()
            })
            .map(|render| render.configuration().clone())
            .expect("the wrapper is a RenderSemanticsAnnotations")
    })
}

/// A set-text request that arrives without its payload is dropped, not emptied.
///
/// `""` is a thing a platform can legitimately mean — clear the field — so
/// synthesizing one for a request that lost its payload would turn a failure to
/// route into a silent erasure of whatever the field held. The request itself
/// resolves: this is the payload being dropped, not the action being refused,
/// which is why the outcome is asserted positive as well.
pub(crate) fn a_set_text_request_without_a_payload_is_dropped_rather_than_emptied() {
    let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);

    let (laid, _tree, node_id) = pump_labelled(
        host()
            .on_set_text(move |_cx, text| sink.borrow_mut().push(text.to_owned()))
            .child(SizedBox::new(40.0, 20.0)),
    );

    // `SetValue` with no `data`: the action routes, the payload does not.
    laid.invoke_semantics_action(request(Action::SetValue, node_id, None))
        .expect(
            "the request must resolve — the node advertises the action, so a \
         rejection here would mean this test never reached the payload",
        );

    assert!(
        seen.borrow().is_empty(),
        "the handler must not run: an empty string is an edit the platform \
         never asked for",
    );
}

/// Every FLUI `SemanticsAction` variant, once.
///
/// `SemanticsAction::ALL` is generated in `flui-protocol` from the same list
/// as the enum, so it cannot miss a variant or repeat one; the drop-set
/// assertion below therefore reasons about every action the framework has.
const FLUI_ACTIONS: &[SemanticsAction] = SemanticsAction::ALL;

/// Every `accesskit::Action` variant, with FLUI's routing answer, written once.
///
/// The macro below emits **both** the enumerated `PLATFORM_ACTIONS` and the
/// wildcard-free `flui_routes` match from this single invocation, so a variant
/// cannot be classified without also joining the list. Two separate hand-written
/// artifacts could: the agreement and drop-set tests above reason over
/// `PLATFORM_ACTIONS`, while the compile-time gate is the match — so an arm
/// added for a new upstream variant classified `true` would join neither the
/// list nor either test's reasoning, and both would keep reasoning about the
/// old count while the shipped framework routed one more action.
///
/// The match is deliberately wildcard-free over a foreign enum.
/// `accesskit::Action` is not `#[non_exhaustive]`, so an upstream release that
/// adds a variant stops this matching function from compiling; the arm the
/// compiler then demands is written inside this same invocation, which is what
/// puts it in the list. That gate is the only one that exists — the translation
/// table's own `_ => None` arm would absorb a new platform action silently, and
/// a maintainer would never be prompted to decide whether FLUI should route it.
///
/// A variant listed twice is caught at compile time, not at run time: the
/// duplicate pattern makes the generated match emit `unreachable_patterns`,
/// which the workspace lints deny.
macro_rules! platform_actions {
    ($($variant:ident => $routes:literal),+ $(,)?) => {
        /// Every `accesskit::Action` variant, so the tests here have a complete
        /// denominator to reason over.
        const PLATFORM_ACTIONS: &[Action] = &[$(Action::$variant),+];

        /// Whether FLUI routes `action` back from the platform.
        fn flui_routes(action: Action) -> bool {
            match action {
                $(Action::$variant => $routes,)+
            }
        }
    };
}

platform_actions! {
    Click => true,
    ShowContextMenu => true,
    ScrollLeft => true,
    ScrollRight => true,
    ScrollUp => true,
    ScrollDown => true,
    Increment => true,
    Decrement => true,
    ScrollIntoView => true,
    SetTextSelection => true,
    SetValue => true,
    SetScrollOffset => true,
    Focus => true,
    Blur => true,
    CustomAction => true,
    Collapse => true,
    Expand => true,
    HideTooltip => false,
    ShowTooltip => false,
    ReplaceSelectedText => false,
    ScrollToPoint => false,
    SetSequentialFocusNavigationStartingPoint => false,
}

/// The set of FLUI actions the platform cannot reach is exactly the documented
/// one — and the one hole in it is asserted, not left to prose.
///
/// This is the "shipped seams never wired" guard. A builder that lets an author
/// register a handler nothing can ever invoke is a lying API, and the only way
/// to keep that from happening by accident is to name the unreachable set and
/// fail when it changes.
pub(crate) fn the_actions_the_platform_cannot_reach_are_exactly_the_documented_drop_set() {
    let reachable: Vec<SemanticsAction> = FLUI_ACTIONS
        .iter()
        .copied()
        .filter(|action| {
            // Reachability examines whole requests, including the numeric
            // SetValue payload whose action-only route is necessarily textual.
            PLATFORM_ACTIONS.iter().copied().any(|platform| {
                [None, Some(ActionData::NumericValue(0.0))]
                    .into_iter()
                    .any(|data| {
                        flui_rendering::semantics::semantics_action_request_for(&request(
                            platform,
                            NodeId(1),
                            data,
                        ))
                        .is_some_and(|translated| translated.action == *action)
                    })
            })
        })
        .collect();

    let unreachable: Vec<SemanticsAction> = FLUI_ACTIONS
        .iter()
        .copied()
        .filter(|action| !reachable.contains(action))
        .collect();

    // Eight are dropped on purpose — the four cursor moves and copy/cut/paste,
    // none of which the platform vocabulary FLUI targets exposes, plus Dismiss.
    // The ninth is the live defect: `DidGainAccessibilityFocus` is advertised
    // outbound (folded into the platform's Focus action) but nothing inbound
    // ever produces it, so a node registering only it advertises a control that
    // does nothing. Asserted here so the hole stays visible until it is closed.
    let expected = [
        SemanticsAction::MoveCursorForwardByCharacter,
        SemanticsAction::MoveCursorBackwardByCharacter,
        SemanticsAction::MoveCursorForwardByWord,
        SemanticsAction::MoveCursorBackwardByWord,
        SemanticsAction::Copy,
        SemanticsAction::Cut,
        SemanticsAction::Paste,
        SemanticsAction::Dismiss,
        SemanticsAction::DidGainAccessibilityFocus,
    ];

    assert_eq!(
        unreachable.len(),
        expected.len(),
        "the unreachable set changed size: {unreachable:?}"
    );
    for action in expected {
        assert!(
            unreachable.contains(&action),
            "{action:?} is no longer unreachable — if the translation table gained \
             a route for it, remove it from this list deliberately. Unreachable: {unreachable:?}"
        );
    }
}

/// The test's own `flui_routes` must agree with the production table.
///
/// Without this, `flui_routes` is a second, drifting copy of the translation
/// table — a test that reimplements the predicate is not a pin. The list it runs
/// over and the match answering it are expanded from one invocation above, so
/// this asserts a property of the production table rather than of two
/// hand-maintained copies of it.
pub(crate) fn the_exhaustive_routing_list_agrees_with_the_translation_table() {
    for &action in PLATFORM_ACTIONS {
        assert_eq!(
            flui_routes(action),
            semantics_action_for(action).is_some(),
            "the exhaustive list and the production table disagree about {action:?}",
        );
    }
}

/// Platform actions preserve direction and exact values across queued delivery.
/// The signal write forces the real frame producer to republish the result.
pub(crate) fn queued_directional_actions_and_numeric_values_reach_the_frame_producer() {
    use flui_rendering::semantics::NumericRange;

    let state = Rc::new(Cell::new(false));
    let described = Rc::clone(&state);
    let expanded = SignalProbe::new(move |ProbeSignals { count, .. }| {
        let expand = Rc::clone(&described);
        let collapse = Rc::clone(&described);
        let toggle = Rc::clone(&described);
        host()
            .expandable(
                described.get(),
                move |cx| {
                    expand.set(true);
                    count.set(cx, 1)
                },
                move |cx| {
                    collapse.set(false);
                    count.set(cx, 0)
                },
            )
            .on_tap(move |cx| {
                let next = !toggle.get();
                toggle.set(next);
                count.set(cx, u32::from(next))
            })
            .child(SizedBox::new(40.0, 20.0))
    });
    let (mut laid, _, node_id) = pump_labelled(expanded.view());
    let listener = laid
        .accessibility_action_listener()
        .expect("production platform listener");
    listener(request(Action::Expand, node_id, None));
    listener(request(Action::Expand, node_id, None));
    laid.tick();
    assert_eq!(expanded.value(), Ok(1), "two expands must not toggle twice");
    let tree = laid.a11y_tree().expect("published expanded frame");
    let node = tree.find_by_label(LABEL).expect("control remains live");
    assert_eq!(node.raw().is_expanded(), Some(true));
    assert!(node.supports_action(Action::Collapse));
    assert!(!node.supports_action(Action::Expand));
    listener(request(Action::Collapse, node_id, None));
    listener(request(Action::Collapse, node_id, None));
    laid.tick();
    assert_eq!(expanded.value(), Ok(0), "two collapses remain collapsed");
    listener(request(Action::Click, node_id, None));
    listener(request(Action::Expand, node_id, None));
    laid.tick();
    assert_eq!(
        expanded.value(),
        Ok(1),
        "a pending pointer toggle cannot reverse an explicit expand"
    );

    let value = Rc::new(Cell::new(0.0_f64));
    let current = Rc::clone(&value);
    let numeric = SignalProbe::new(move |ProbeSignals { count, .. }| {
        let changed = Rc::clone(&current);
        host()
            .numeric_range(
                NumericRange::new(current.get(), 0.0, 10.0, 1.0).expect("finite fixture"),
            )
            .on_set_numeric_value(move |cx, exact| {
                changed.set(exact);
                count.update(cx, |n| *n += 1)
            })
            .child(SizedBox::new(40.0, 20.0))
    });
    let (mut laid, _, node_id) = pump_labelled(numeric.view());
    let listener = laid
        .accessibility_action_listener()
        .expect("production platform listener");
    listener(request(
        Action::SetValue,
        node_id,
        Some(ActionData::NumericValue(2.375)),
    ));
    laid.tick();
    assert_eq!(
        value.get(),
        2.375,
        "the step must not round the requested value"
    );
    assert_eq!(numeric.value(), Ok(1));
    let tree = laid.a11y_tree().expect("numeric frame republished");
    let node = tree
        .find_by_label(LABEL)
        .expect("range control remains live");
    assert_eq!(node.raw().numeric_value(), Some(2.375));
    assert_eq!(node.raw().min_numeric_value(), Some(0.0));
    assert_eq!(node.raw().max_numeric_value(), Some(10.0));
    assert_eq!(node.raw().numeric_value_step(), Some(1.0));
    for rejected in [f64::NAN, f64::INFINITY, -0.5, 10.5] {
        listener(request(
            Action::SetValue,
            node_id,
            Some(ActionData::NumericValue(rejected)),
        ));
        laid.tick();
        assert_eq!(
            value.get(),
            2.375,
            "invalid numeric payload reached the callback"
        );
        assert_eq!(
            numeric.value(),
            Ok(1),
            "rejected request must not perform a signal write"
        );
    }
    listener(request(
        Action::SetValue,
        node_id,
        Some(ActionData::NumericValue(10.0)),
    ));
    laid.tick();
    assert_eq!(
        value.get(),
        10.0,
        "the next valid request survives rejected values"
    );
    assert_eq!(numeric.value(), Ok(2));
}

/// Native range metadata cannot admit non-finite values or reversed bounds.
pub(crate) fn numeric_range_admission_and_owner_payload_validation() {
    use flui_rendering::semantics::{ActionArgs, NumericRange, NumericRangeError};
    for (value, min, max, step, error) in [
        (f64::NAN, 0.0, 1.0, 1.0, NumericRangeError::NonFinite),
        (
            0.0,
            f64::NEG_INFINITY,
            1.0,
            1.0,
            NumericRangeError::NonFinite,
        ),
        (0.0, 1.0, 0.0, 1.0, NumericRangeError::ReversedBounds),
        (2.0, 0.0, 1.0, 1.0, NumericRangeError::ValueOutOfRange),
        (0.0, 0.0, 1.0, 0.0, NumericRangeError::NonPositiveStep),
    ] {
        assert_eq!(NumericRange::new(value, min, max, step), Err(error));
    }
    let count = Rc::new(Cell::new(0));
    let changed = Rc::clone(&count);
    let (laid, tree, node_id) = pump_labelled(
        host()
            .numeric_range(NumericRange::new(4.0, 4.0, 4.0, 1.0).expect("zero-span range is valid"))
            .on_set_numeric_value(move |_cx, value| {
                assert_eq!(value, 4.0);
                changed.set(changed.get() + 1);
            })
            .child(SizedBox::new(40.0, 20.0)),
    );
    assert!(
        tree.find_by_label(LABEL)
            .expect("range exists")
            .supports_action(Action::SetValue)
    );
    for arguments in [
        None,
        Some(ActionArgs::SetText { text: "4".into() }),
        Some(ActionArgs::SetNumericValue { value: 4.1 }),
    ] {
        let refused = laid.pipeline_owner().with(|owner| {
            owner.resolve_semantics_action(SemanticsActionRequest {
                node_id: AccessibilityNodeId::from_u64(node_id.0).expect("published identity"),
                action: SemanticsAction::SetNumericValue,
                arguments,
            })
        });
        assert!(refused.is_err(), "malformed numeric request was admitted");
    }
    assert_eq!(count.get(), 0);
    laid.invoke_semantics_action(request(
        Action::SetValue,
        node_id,
        Some(ActionData::NumericValue(4.0)),
    ))
    .expect("exact zero-span value is admitted");
    assert_eq!(count.get(), 1);
}

/// A screen reader scrolls a list through the same position a drag drives:
/// the scrollable advertises scroll-down/up, and each request moves the
/// offset by most of a viewport, clamped to the extents.
pub(crate) fn assistive_scroll_actions_move_a_scrollable() {
    use flui_widgets::{ScrollController, Scrollable};

    let scroll = ScrollController::new();
    scroll.update_dimensions(100.0, 0.0, 900.0);
    let (mut laid, tree, _host) = pump_labelled(
        host().child(
            Scrollable::new()
                .controller(scroll.clone())
                .child(SizedBox::new(100.0, 1000.0)),
        ),
    );
    let scroller = tree
        .nodes()
        .find(|node| node.supports_action(Action::ScrollDown))
        .unwrap_or_else(|| {
            panic!(
                "a scrollable must advertise scroll-down to assistive technology. \
                 Tree was:\n{}",
                tree.describe()
            )
        });
    assert!(scroller.supports_action(Action::ScrollUp));
    let scroller = scroller.id();

    laid.invoke_semantics_action(request(Action::ScrollDown, scroller, None))
        .expect("scroll-down resolves");
    laid.tick();
    let after_down = scroll.pixels();
    assert!(
        after_down > 0.0,
        "scroll-down moved the list, got {after_down}"
    );

    laid.invoke_semantics_action(request(Action::ScrollUp, scroller, None))
        .expect("scroll-up resolves");
    laid.tick();
    assert_eq!(scroll.pixels(), 0.0, "scroll-up returns to the start");
}

// ===========================================================================
// Published bounds are physical pixels
// ===========================================================================

/// Where an AccessKit adapter places the labelled node: its bounds under
/// its own transform and every ancestor's, which AccessKit defines as
/// physical pixels relative to the window's client area.
fn effective_bounds(tree: &flui_testing::A11yTree, label: &str) -> flui_testing::a11y::A11yRect {
    let update = tree.raw();
    let parent_of = |id| {
        update
            .nodes
            .iter()
            .find(|(_, node)| node.children().contains(&id))
            .map(|(key, node)| (*key, node))
    };
    let (target, node) = update
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label))
        .expect("the labelled node is published");
    let mut rect = node.bounds().expect("the labelled node has bounds");
    if let Some(transform) = node.transform() {
        rect = transform.transform_rect_bbox(rect);
    }
    let mut current = *target;
    while let Some((parent, parent_node)) = parent_of(current) {
        if let Some(transform) = parent_node.transform() {
            rect = transform.transform_rect_bbox(rect);
        }
        current = parent;
    }
    rect
}

fn assert_rect_near(
    actual: flui_testing::a11y::A11yRect,
    expected: (f64, f64, f64, f64),
    what: &str,
) {
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(
        close(actual.x0, expected.0)
            && close(actual.y0, expected.1)
            && close(actual.x1, expected.2)
            && close(actual.y1, expected.3),
        "{what}: published {actual:?}, expected {expected:?}"
    );
}

/// A window at 150% publishes physical bounds, and a scale change
/// republishes them.
///
/// The button sits 30 logical pixels down a column inside a scrollable
/// scrolled by 10, so its logical rect starts at y = 20: the
/// scroll offset and the parent offset reach the published rect before the
/// window's scale multiplies it. Assistive technology hit-tests and draws its
/// highlight from these numbers; at the logical size it lands on two thirds
/// of the control.
pub(crate) fn published_bounds_are_physical_and_follow_the_scale_factor() {
    use flui_view::ViewExt as _;
    use flui_widgets::{Column, ScrollController, Scrollable};
    use std::sync::{Arc, Mutex};

    let controller = ScrollController::new();
    let content = Column::new((
        SizedBox::new(200.0, 30.0).boxed(),
        Semantics::new()
            .container(true)
            .button(true)
            .label("Physical")
            .on_tap(|_cx| {})
            .child(SizedBox::new(200.0, 40.0))
            .boxed(),
        SizedBox::new(200.0, 600.0).boxed(),
    ));
    let mut laid = lay_out(
        Scrollable::new()
            .controller(controller.clone())
            .child(content.boxed()),
        crate::common::tight(200.0, 200.0),
    );

    let delivered: Arc<Mutex<Option<flui_testing::A11yTree>>> = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&delivered);
    laid.pipeline_owner().with_mut(|owner| {
        owner.set_device_pixel_ratio(1.5);
        owner.set_semantics_update_callback(Arc::new(move |update| {
            let mut mirror = sink.lock().expect("publication mirror lock");
            let Some(current) = mirror.as_ref() else {
                *mirror = Some(flui_testing::A11yTree::new(update.clone()));
                return;
            };
            let mut merged = current.raw().clone();
            for (id, node) in &update.nodes {
                if let Some((_, previous)) = merged.nodes.iter_mut().find(|(key, _)| key == id) {
                    previous.clone_from(node);
                } else {
                    merged.nodes.push((*id, node.clone()));
                }
            }
            *mirror = Some(flui_testing::A11yTree::new(merged));
        }));
    });
    laid.enable_semantics();
    laid.tick();
    controller.jump_to(10.0);
    laid.tick();

    let published = || {
        delivered
            .lock()
            .expect("publication mirror lock")
            .clone()
            .expect("the adapter was published to")
    };
    let logical = || {
        published()
            .find_by_label("Physical")
            .expect("premise: the button is published")
            .bounds()
            .expect("premise: the button has bounds")
    };
    // The column offset (30) less the scroll offset (10) reached the rect.
    let before = logical();
    assert!(
        (before.y0 - 20.0).abs() < 1e-9 && (before.height() - 40.0).abs() < 1e-9,
        "premise: the semantics rect is logical and scrolled, got {before:?}"
    );
    let scaled = |rect: flui_testing::a11y::A11yRect, ratio: f64| {
        (
            rect.x0 * ratio,
            rect.y0 * ratio,
            rect.x1 * ratio,
            rect.y1 * ratio,
        )
    };
    assert_rect_near(
        effective_bounds(&published(), "Physical"),
        scaled(before, 1.5),
        "at a ratio of 1.5",
    );

    // Moving the window to a 200% monitor: the adapter must hear the new
    // scale.
    laid.pipeline_owner()
        .with_mut(|owner| owner.set_device_pixel_ratio(2.0));
    laid.tick();
    assert_rect_near(
        effective_bounds(&published(), "Physical"),
        scaled(logical(), 2.0),
        "after the ratio changed to 2.0",
    );
}
