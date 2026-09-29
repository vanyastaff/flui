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
#[test]
fn merge_semantics_collapses_its_descendants_in_the_a11y_tree() {
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

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use flui_testing::{Action, ActionData, ActionRequest, NodeId, TreeId, invoke_semantics_action};

/// Mounts `semantics` with semantics enabled and returns the single node
/// carrying `label`, together with a live view of the a11y tree.
///
/// The tree is returned alongside its own description so a failing assertion
/// can show what was actually there, matching this file's other a11y tests.
fn pump_labelled(
    semantics: Semantics,
) -> (
    flui_widgets::testing::LaidOut,
    flui_testing::A11yTree,
    NodeId,
) {
    let mut laid = lay_out(semantics, loose(200.0));
    laid.enable_semantics();
    laid.pump();
    let tree = laid
        .a11y_tree()
        .expect("semantics enabled before the frame");
    let node_id = tree
        .find_by_label(LABEL)
        .unwrap_or_else(|error| panic!("expected one node labelled {LABEL:?}: {error}"))
        .id();
    (laid, tree, node_id)
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
#[test]
fn a_tap_handler_round_trips_from_a_platform_click_to_the_callback() {
    let activations = Arc::new(AtomicU32::new(0));
    let counted = Arc::clone(&activations);

    let (laid, tree, node_id) = pump_labelled(
        Semantics::new()
            .container(true)
            .label(LABEL)
            .on_tap(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            })
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

    invoke_semantics_action(
        &laid.pipeline_owner(),
        request(Action::Click, node_id, None),
    )
    .expect("a click on a node advertising one must resolve");

    assert_eq!(
        activations.load(Ordering::SeqCst),
        1,
        "the handler must have run exactly once",
    );
}

/// A set-text request that arrives without its payload is dropped, not emptied.
///
/// `""` is a thing a platform can legitimately mean — clear the field — so
/// synthesizing one for a request that lost its payload would turn a failure to
/// route into a silent erasure of whatever the field held. The request itself
/// resolves: this is the payload being dropped, not the action being refused,
/// which is why the outcome is asserted positive as well.
#[test]
fn a_set_text_request_without_a_payload_is_dropped_rather_than_emptied() {
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);

    let (laid, _tree, node_id) = pump_labelled(
        Semantics::new()
            .container(true)
            .label(LABEL)
            .on_set_text(move |text| {
                sink.lock()
                    .expect("this test never poisons its own lock")
                    .push(text.to_owned());
            })
            .child(SizedBox::new(40.0, 20.0)),
    );

    // `SetValue` with no `data`: the action routes, the payload does not.
    invoke_semantics_action(
        &laid.pipeline_owner(),
        request(Action::SetValue, node_id, None),
    )
    .expect(
        "the request must resolve — the node advertises the action, so a \
         rejection here would mean this test never reached the payload",
    );

    assert!(
        seen.lock()
            .expect("this test never poisons its own lock")
            .is_empty(),
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
#[test]
fn the_actions_the_platform_cannot_reach_are_exactly_the_documented_drop_set() {
    let reachable: Vec<SemanticsAction> = FLUI_ACTIONS
        .iter()
        .copied()
        .filter(|action| {
            // Reachable if any platform action translates back to it, which is
            // the live table's answer rather than a restatement of it.
            PLATFORM_ACTIONS
                .iter()
                .copied()
                .any(|platform| semantics_action_for(platform) == Some(*action))
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
#[test]
fn the_exhaustive_routing_list_agrees_with_the_translation_table() {
    for &action in PLATFORM_ACTIONS {
        assert_eq!(
            flui_routes(action),
            semantics_action_for(action).is_some(),
            "the exhaustive list and the production table disagree about {action:?}",
        );
    }
}
