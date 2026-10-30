//! The incremental-publish contract of `SemanticsOwner::flush`.
//!
//! The flush serializes only changed nodes into the platform update and
//! returns immediately when nothing is dirty. FLUI rebuilds its semantics arena every assembly pass (flui-semantics ARCHITECTURE.md, semantics assembly),
//! so "dirty" alone cannot distinguish a real change from a rebuild that
//! reproduced the same tree — the flush diffs per-node payloads, keyed by
//! stable [`AccessibilityNodeId`], against the last delivered update. These
//! tests pin the observable halves of that contract:
//!
//! - a pass that changes nothing publishes **nothing**;
//! - a pass that changes one node publishes **that node**, not the tree.
//!
//! Every fixture drives the owner exactly the way the pipeline does
//! (`rebuild_semantics_owner`): clear the arena, reinsert every node, root
//! it, flush. The tests deliberately use only API that predates the diff so
//! they can run — and fail — against the pre-diff implementation.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::RenderId;
use flui_semantics::{SemanticsNode, SemanticsOwner, TreeUpdate};
use parking_lot::Mutex;

/// A render-backed node: production assembly always attaches a source
/// render id, and it is what makes a node publishable at all. Generation 7
/// keeps the stable id space visibly distinct from arena positions.
fn node(index: u32, label: &str) -> SemanticsNode {
    let mut node = SemanticsNode::new().with_source_render_id(RenderId::new_gen(
        index,
        core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
    ));
    node.config_mut().set_label(label);
    node
}

/// Rebuild the owner's arena the way `run_semantics` does every pass:
/// clear, reinsert, root. `labels[i]` becomes child `i` of the root.
fn rebuild(owner: &mut SemanticsOwner, labels: &[&str]) {
    owner.clear();
    let root = owner.insert(node(0, "root"));
    for (index, label) in labels.iter().enumerate() {
        let child = owner.insert(node(
            u32::try_from(index).expect("fixture fits u32") + 1,
            label,
        ));
        owner.add_child(root, child);
    }
    owner.set_root(Some(root));
}

/// An owner whose callback records every delivered update.
fn recording_owner() -> (SemanticsOwner, Arc<Mutex<Vec<TreeUpdate>>>) {
    let received: Arc<Mutex<Vec<TreeUpdate>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&received);
    let owner = SemanticsOwner::new(Arc::new(move |update: &TreeUpdate| {
        sink.lock().push(update.clone());
    }));
    (owner, received)
}

/// **The incremental-publish contract.** An identical rebuild publishes
/// nothing (the idle contract); one changed label publishes exactly that node,
/// not its siblings or the unchanged root (the O(dirty) contract).
#[test]
fn a_rebuild_publishes_only_what_changed() {
    let (mut owner, received) = recording_owner();

    rebuild(&mut owner, &["alpha", "beta", "gamma"]);
    owner.flush();
    assert_eq!(
        received.lock().len(),
        1,
        "the first publish is the full tree"
    );

    rebuild(&mut owner, &["alpha", "beta", "gamma"]);
    owner.flush();
    assert_eq!(
        received.lock().len(),
        1,
        "an identical rebuild must not deliver a second update"
    );

    rebuild(&mut owner, &["alpha", "CHANGED", "gamma"]);
    owner.flush();

    let updates = received.lock();
    assert_eq!(updates.len(), 2, "the change itself must be delivered");
    let diff = &updates[1];
    assert_eq!(
        diff.nodes.len(),
        1,
        "one changed node publishes one node, not the tree: got {:?}",
        diff.nodes
            .iter()
            .map(|(id, node)| (id.0, node.label().map(str::to_owned)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        diff.nodes[0].1.label(),
        Some("CHANGED"),
        "and it is the node whose content changed"
    );
}

/// The published tree is in physical pixels, and a ratio change on an
/// otherwise idle tree republishes it.
///
/// AccessKit reads a node's bounds under its ancestors' transforms as
/// physical pixels relative to the window. The tree keeps logical rects, the
/// root carries the scale, and a changed ratio resends the root alone: the
/// descendants inherit the new transform without being re-serialized.
#[test]
fn the_published_root_scales_logical_bounds_to_physical_pixels() {
    use flui_foundation::geometry::Rect as LogicalRect;

    let (mut owner, received) = recording_owner();
    owner.set_device_pixel_ratio(1.5);
    owner.clear();
    let root = owner.insert(node(0, "root"));
    let mut button = node(1, "button");
    button.set_rect(LogicalRect::from_ltrb(10.0, 20.0, 78.0, 36.0));
    let button = owner.insert(button);
    owner.add_child(root, button);
    owner.set_root(Some(root));
    owner.flush();

    let effective = |update: &TreeUpdate, root_update: &TreeUpdate| {
        let root_id = root_update.tree.as_ref().expect("a full update").root;
        let root_transform = update
            .nodes
            .iter()
            .chain(&root_update.nodes)
            .find(|(id, _)| *id == root_id)
            .and_then(|(_, node)| node.transform().copied())
            .unwrap_or(accesskit::Affine::IDENTITY);
        let (_, button) = root_update
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("button"))
            .expect("the button is published");
        root_transform.transform_rect_bbox(button.bounds().expect("the button has bounds"))
    };

    let first = received.lock()[0].clone();
    let button_bounds = first
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("button"))
        .and_then(|(_, node)| node.bounds())
        .expect("the button is published with bounds");
    assert_eq!(
        button_bounds,
        accesskit::Rect::new(10.0, 20.0, 78.0, 36.0),
        "node bounds stay logical; the scale lives on the root"
    );
    assert_eq!(
        effective(&first, &first),
        accesskit::Rect::new(15.0, 30.0, 117.0, 54.0),
        "at 150% a 68x16 control is 102x24 physical pixels"
    );

    owner.set_device_pixel_ratio(2.0);
    owner.flush();
    let updates = received.lock();
    assert_eq!(updates.len(), 2, "a ratio change must be delivered");
    let change = &updates[1];
    assert_eq!(
        change.nodes.len(),
        1,
        "only the root carries the scale, so only the root is resent"
    );
    assert_eq!(
        effective(change, &first),
        accesskit::Rect::new(20.0, 40.0, 156.0, 72.0),
        "after the ratio changed to 2.0"
    );
}

#[test]
fn detaching_checks_the_actual_parent_and_preserves_reparenting() {
    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let first = owner.insert(node(1, "first parent"));
    let second = owner.insert(node(2, "second parent"));
    let child = owner.insert(node(3, "child"));
    owner.set_root(Some(root));
    owner.add_child(root, first);
    owner.add_child(root, second);
    owner.add_child(first, child);
    owner.flush();

    owner.remove_child(second, child);
    owner.remove_child(flui_foundation::SemanticsId::new(99), child);
    assert_eq!(owner.tree().parent(child), Some(first));
    assert_eq!(owner.tree().children(first), Some(&[child][..]));
    owner.flush();
    assert_eq!(received.lock().len(), 1, "a refused detach changes nothing");

    owner.add_child(second, child);
    assert_eq!(owner.tree().parent(child), Some(second));
    assert_eq!(owner.tree().children(first), Some(&[][..]));
    assert_eq!(owner.tree().children(second), Some(&[child][..]));
    owner.flush();
    {
        let updates = received.lock();
        let moved = &updates[1];
        let first_id = owner
            .get(first)
            .expect("first parent")
            .accessibility_id()
            .expect("render-backed");
        let second_id = owner
            .get(second)
            .expect("second parent")
            .accessibility_id()
            .expect("render-backed");
        let child_id = owner
            .get(child)
            .expect("child")
            .accessibility_id()
            .expect("render-backed");
        let published = |id| {
            &moved
                .nodes
                .iter()
                .find(|(node_id, _)| node_id.0 == id)
                .expect("changed parent published")
                .1
        };
        assert_eq!(published(first_id.as_u64()).children(), []);
        assert_eq!(
            published(second_id.as_u64()).children(),
            &[accesskit::NodeId(child_id.as_u64())]
        );
    }

    owner.remove_child(second, child);
    assert_eq!(owner.tree().parent(child), None);
    assert_eq!(owner.tree().children(second), Some(&[][..]));
    owner.add_child(first, child);
    assert_eq!(owner.tree().parent(child), Some(first));
    assert_eq!(owner.tree().children(first), Some(&[child][..]));
}

fn failed_label_delivery_retries_unchanged_input() {
    assert_delivery_recovers(|owner, _, child| {
        owner
            .get_mut(child)
            .expect("live child")
            .config_mut()
            .set_label("changed");
    });
}

fn failed_focus_delivery_retries_unchanged_input() {
    assert_delivery_recovers(|owner, _, child| {
        owner
            .get_mut(child)
            .expect("live child")
            .config_mut()
            .set_focused(true);
    });
}

fn failed_removal_delivery_retries_unchanged_input() {
    assert_delivery_recovers(|owner, _, child| {
        drop(owner.remove(child));
    });
}

fn assert_delivery_recovers(
    change: fn(&mut SemanticsOwner, flui_foundation::SemanticsId, flui_foundation::SemanticsId),
) {
    let fail_delivery = Arc::new(AtomicBool::new(false));
    let received = Arc::new(Mutex::new(Vec::<TreeUpdate>::new()));
    let sink = Arc::clone(&received);
    let fail = Arc::clone(&fail_delivery);
    let mut owner = SemanticsOwner::new(Arc::new(move |update| {
        assert!(
            !fail.load(Ordering::Relaxed),
            "delivery failed before acceptance"
        );
        sink.lock().push(update.clone());
    }));
    let root = owner.insert(node(0, "root"));
    let child = owner.insert(node(1, "original"));
    owner.add_child(root, child);
    owner.set_root(Some(root));
    assert_eq!(owner.flush(), 2);
    change(&mut owner, root, child);
    let expected = owner.to_accesskit_tree_update(None).expect("rooted tree");

    fail_delivery.store(true, Ordering::Relaxed);
    let failure = catch_unwind(AssertUnwindSafe(|| owner.flush()))
        .expect_err("the callback rejects this attempt");
    let is_delivery_failure = failure
        .downcast_ref::<String>()
        .is_some_and(|message| message.contains("delivery failed before acceptance"))
        || failure
            .downcast_ref::<&str>()
            .is_some_and(|message| message.contains("delivery failed before acceptance"));
    std::mem::forget(failure);
    assert!(
        is_delivery_failure,
        "the original delivery failure remains authoritative"
    );
    assert!(owner.needs_flush(), "failed delivery keeps work pending");
    assert_eq!(received.lock().len(), 1);

    fail_delivery.store(false, Ordering::Relaxed);
    owner.flush();
    {
        let updates = received.lock();
        assert_eq!(updates.len(), 2, "unchanged input must retry delivery");
        let mut actual = updates[0]
            .nodes
            .iter()
            .cloned()
            .collect::<std::collections::HashMap<_, _>>();
        for (id, node) in &updates[1].nodes {
            actual.insert(*id, node.clone());
        }
        // Removal is conveyed by the parent's child list, not a tombstone.
        for (id, node) in &expected.nodes {
            assert_eq!(
                actual.get(id),
                Some(node),
                "retry delivers the current node"
            );
        }
        assert_eq!(
            updates[1].focus, expected.focus,
            "retry delivers current focus"
        );
    }
    assert!(!owner.needs_flush());
    assert_eq!(owner.flush(), 0);
    assert_eq!(
        received.lock().len(),
        2,
        "accepted retry leaves no duplicate debt"
    );

    owner
        .get_mut(root)
        .expect("root remains live")
        .config_mut()
        .set_label("next operation");
    assert_eq!(owner.flush(), 1);
    assert_eq!(
        received.lock().len(),
        3,
        "the next operation still progresses"
    );
}

#[test]
fn failed_incremental_delivery_preserves_retry_and_progress() {
    let cases: &[(&str, fn())] = &[
        (
            "failed_label_delivery_retries_unchanged_input",
            failed_label_delivery_retries_unchanged_input,
        ),
        (
            "failed_focus_delivery_retries_unchanged_input",
            failed_focus_delivery_retries_unchanged_input,
        ),
        (
            "failed_removal_delivery_retries_unchanged_input",
            failed_removal_delivery_retries_unchanged_input,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, run) in cases {
        if let Err(payload) = catch_unwind(run) {
            failures.push(name);
            std::mem::forget(payload);
        }
    }
    assert!(
        failures.is_empty(),
        "delivery recovery rows failed: {failures:?}"
    );
}

/// A public owner fixture exposing one native numeric control.
struct NumericControl {
    owner: SemanticsOwner,
    id: flui_foundation::SemanticsId,
    target: flui_semantics::AccessibilityNodeId,
    received: Arc<Mutex<Vec<f64>>>,
}

impl NumericControl {
    fn new() -> Self {
        use flui_semantics::{ActionCall, NumericRange, SemanticsAction};
        let (mut owner, _) = recording_owner();
        let mut control = node(1, "numeric control");
        let target = control.accessibility_id().expect("render-backed identity");
        let received = Arc::new(Mutex::new(Vec::new()));
        let numeric = Arc::clone(&received);
        control
            .config_mut()
            .set_numeric_range(NumericRange::try_new(0.0, 0.0, 10.0, 1.0).expect("finite fixture"));
        control.config_mut().add_action(
            SemanticsAction::SetNumericValue,
            Rc::new(move |call| {
                let ActionCall::SetNumericValue(value) = call else {
                    panic!("the numeric handler received {call:?}");
                };
                numeric.lock().push(value);
            }),
        );
        let id = owner.insert(control);
        owner.set_root(Some(id));
        owner.flush();
        Self {
            owner,
            id,
            target,
            received,
        }
    }

    fn resolve(
        &self,
        call: flui_semantics::ActionCall,
    ) -> Result<flui_semantics::SemanticsActionInvocation, flui_semantics::SemanticsActionError>
    {
        self.owner
            .resolve_action(flui_semantics::SemanticsActionRequest::new(
                self.target,
                call,
            ))
    }

    fn set(
        &self,
        value: f64,
    ) -> Result<flui_semantics::SemanticsActionInvocation, flui_semantics::SemanticsActionError>
    {
        self.resolve(flui_semantics::ActionCall::SetNumericValue(value))
    }

    fn narrow_and_publish(&mut self) {
        self.owner
            .get_mut(self.id)
            .expect("control remains live")
            .config_mut()
            .set_numeric_range(
                flui_semantics::NumericRange::try_new(0.0, 0.0, 5.0, 1.0)
                    .expect("finite narrower range"),
            );
        self.owner.mark_dirty(self.id);
        assert_eq!(
            self.owner.flush(),
            1,
            "the narrower metadata reaches the adapter"
        );
    }
}

fn an_exact_value_inside_the_current_range_reaches_the_handler() {
    let mut control = NumericControl::new();
    control.narrow_and_publish();
    control
        .set(4.375)
        .expect("the current range admits an exact value off the step")
        .invoke();
    assert_eq!(*control.received.lock(), [4.375]);
}

fn a_value_outside_the_current_range_is_refused() {
    use flui_semantics::SemanticsActionError;
    let mut control = NumericControl::new();
    control
        .set(9.0)
        .expect("the published range admits 9")
        .invoke();
    control.narrow_and_publish();
    assert_eq!(
        control.set(9.0).err(),
        Some(SemanticsActionError::InvalidNumericValue {
            node_id: control.target
        })
    );
    assert_eq!(*control.received.lock(), [9.0]);
}

fn non_finite_or_out_of_range_values_are_refused() {
    use flui_semantics::{ActionCall, SemanticsActionError};
    let control = NumericControl::new();
    let refused = Some(SemanticsActionError::InvalidNumericValue {
        node_id: control.target,
    });
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 10.5] {
        assert_eq!(control.set(value).err(), refused, "{value} was admitted");
    }
    // A text that is no number has no handler to reach on a numeric-only
    // control; one that is a number outside the range is refused as one.
    assert!(matches!(
        control.resolve(ActionCall::SetValue("five".into())),
        Err(SemanticsActionError::UnsupportedAction { .. })
    ));
    assert_eq!(
        control.resolve(ActionCall::SetValue("11".into())).err(),
        refused
    );
    assert!(control.received.lock().is_empty());
}

/// A numeric setter is admitted only with a finite value inside the node's
/// current range; a refused value never reaches the handler.
#[test]
fn numeric_setters_are_checked_against_the_current_range() {
    let cases: &[(&str, fn())] = &[
        (
            "exact_value_in_range",
            an_exact_value_inside_the_current_range_reaches_the_handler,
        ),
        (
            "value_outside_narrowed_range",
            a_value_outside_the_current_range_is_refused,
        ),
        (
            "non_finite_or_out_of_range",
            non_finite_or_out_of_range_values_are_refused,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, run) in cases {
        if let Err(payload) = catch_unwind(run) {
            failures.push(name);
            std::mem::forget(payload);
        }
    }
    assert!(
        failures.is_empty(),
        "numeric range admission rows failed: {failures:?}"
    );
}

struct RevealFixture {
    owner: SemanticsOwner,
    outer: flui_foundation::SemanticsId,
    inner: flui_foundation::SemanticsId,
    target: flui_foundation::SemanticsId,
    identity: flui_semantics::AccessibilityNodeId,
    calls: Arc<Mutex<Vec<&'static str>>>,
    failures: [Arc<AtomicBool>; 2],
    retired: Arc<std::sync::atomic::AtomicUsize>,
}

struct RevealCapture {
    retired: Arc<std::sync::atomic::AtomicUsize>,
    panic_on_drop: bool,
    failure: &'static str,
}

impl Drop for RevealCapture {
    fn drop(&mut self) {
        self.retired.fetch_add(1, Ordering::SeqCst);
        if self.panic_on_drop {
            std::panic::panic_any(self.failure);
        }
    }
}

impl RevealFixture {
    fn new(panic_on_drop: bool) -> Self {
        Self::with_retirement_failures([panic_on_drop, false])
    }

    fn with_retirement_failures(retirement_failures: [bool; 2]) -> Self {
        use flui_foundation::geometry::Rect;
        use flui_semantics::{ActionCall, SemanticsAction};
        let (mut owner, _) = recording_owner();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let failures = [
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        ];
        let retired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut make_owner = |index, name, failure: &Arc<AtomicBool>, panic_on_drop| {
            let mut ancestor = node(index, name);
            ancestor.set_reveal_rect(Rect::new(0.0, 0.0, 200.0, 200.0));
            let sink = Arc::clone(&calls);
            let failure = Arc::clone(failure);
            let capture = RevealCapture {
                retired: Arc::clone(&retired),
                panic_on_drop,
                failure: if name == "inner" {
                    "inner capture retirement"
                } else {
                    "outer capture retirement"
                },
            };
            ancestor.config_mut().add_action(
                SemanticsAction::ScrollIntoView,
                Rc::new(move |call| {
                    let _capture = &capture;
                    assert!(matches!(call, ActionCall::ScrollIntoView(Some(_))));
                    sink.lock().push(name);
                    assert!(!failure.load(Ordering::SeqCst), "{name} reveal failure");
                }),
            );
            owner.insert(ancestor)
        };
        let outer = make_owner(0, "outer", &failures[1], retirement_failures[1]);
        let inner = make_owner(1, "inner", &failures[0], retirement_failures[0]);
        let mut target_node = node(2, "reveal target");
        target_node.set_reveal_rect(Rect::new(0.0, 600.0, 40.0, 640.0));
        target_node.config_mut().set_has_reveal_ancestor(true);
        let identity = target_node
            .accessibility_id()
            .expect("render-backed target");
        let target = owner.insert(target_node);
        owner.add_child(outer, inner);
        owner.add_child(inner, target);
        owner.set_root(Some(outer));
        Self {
            owner,
            outer,
            inner,
            target,
            identity,
            calls,
            failures,
            retired,
        }
    }

    fn resolve(&self) -> flui_semantics::SemanticsActionInvocation {
        self.owner
            .resolve_action(flui_semantics::SemanticsActionRequest::new(
                self.identity,
                flui_semantics::ActionCall::ScrollIntoView(None),
            ))
            .expect("rooted descendant reveal")
    }
}

fn reveal_stale_snapshot_is_refused_and_fresh_replacement_recovers() {
    for replace_target in [true, false] {
        let mut fixture = RevealFixture::new(false);
        let held = fixture.resolve();
        let id = if replace_target {
            fixture.target
        } else {
            fixture.inner
        };
        let replacement = fixture.owner.get(id).expect("live node").clone();
        assert!(fixture.owner.tree_mut().replace_node(id, replacement));
        held.invoke();
        assert!(
            fixture.calls.lock().is_empty(),
            "replaced membership is stale"
        );
        fixture.resolve().invoke();
        assert_eq!(*fixture.calls.lock(), ["inner", "outer"]);
    }
}

fn reveal_removed_and_reparented_paths_do_not_deliver_old_geometry() {
    for remove in [true, false] {
        let mut fixture = RevealFixture::new(false);
        let held = fixture.resolve();
        if remove {
            fixture.owner.remove(fixture.target);
        } else {
            fixture.owner.add_child(fixture.outer, fixture.target);
        }
        held.invoke();
        assert!(fixture.calls.lock().is_empty());
        if !remove {
            fixture.resolve().invoke();
            assert_eq!(*fixture.calls.lock(), ["outer"]);
        }
    }
}

fn reveal_first_body_failure_keeps_live_outer_progress_and_recovery() {
    for competing in [false, true] {
        let fixture = RevealFixture::new(false);
        fixture.failures[0].store(true, Ordering::SeqCst);
        fixture.failures[1].store(competing, Ordering::SeqCst);
        let first = catch_unwind(AssertUnwindSafe(|| fixture.resolve().invoke()))
            .expect_err("inner callback fails");
        assert_eq!(
            first.downcast_ref::<String>().map(String::as_str),
            Some("inner reveal failure")
        );
        assert_eq!(*fixture.calls.lock(), ["inner", "outer"]);
        fixture.failures[0].store(false, Ordering::SeqCst);
        fixture.failures[1].store(false, Ordering::SeqCst);
        fixture.resolve().invoke();
        assert_eq!(*fixture.calls.lock(), ["inner", "outer", "inner", "outer"]);
        let retired = Arc::clone(&fixture.retired);
        drop(fixture);
        assert_eq!(
            retired.load(Ordering::SeqCst),
            0,
            "failed snapshot retains captures after final owner release"
        );
    }
}

fn reveal_healthy_capture_retirement_and_failed_retirement_keep_authority() {
    let fixture = RevealFixture::new(false);
    fixture.resolve().invoke();
    let retired = Arc::clone(&fixture.retired);
    drop(fixture);
    assert_eq!(retired.load(Ordering::SeqCst), 2);

    let mut fixture = RevealFixture::new(true);
    let held = fixture.resolve();
    fixture.owner.clear();
    let first =
        catch_unwind(AssertUnwindSafe(|| held.invoke())).expect_err("capture retirement fails");
    assert_eq!(
        first.downcast_ref::<&str>().copied(),
        Some("inner capture retirement")
    );
    assert!(
        fixture.calls.lock().is_empty(),
        "stale geometry never invokes a callback"
    );
    assert_eq!(
        fixture.retired.load(Ordering::SeqCst),
        1,
        "first retirement failure retains the remaining callback"
    );
}

#[test]
fn configuration_capture_retirement_preserves_first_failure_and_healthy_destruction() {
    use flui_foundation::panic::PanicRecovery;
    use flui_semantics::{SemanticsAction, SemanticsActionHandler, SemanticsConfiguration};
    for (regular_fails, custom_fails) in
        [(false, false), (true, false), (true, true), (false, true)]
    {
        let retired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let handler = |panic_on_drop, failure| -> SemanticsActionHandler {
            let capture = RevealCapture {
                retired: retired.clone(),
                panic_on_drop,
                failure,
            };
            Rc::new(move |_| {
                let _capture = &capture;
            })
        };
        let mut configuration = SemanticsConfiguration::new();
        configuration.add_action(
            SemanticsAction::Click,
            handler(regular_fails, "regular capture"),
        );
        let _ = configuration.add_custom_action("custom", handler(custom_fails, "custom capture"));
        let mut recovery = PanicRecovery::new();
        configuration.retire(&mut recovery.scope());
        let result = catch_unwind(AssertUnwindSafe(|| recovery.finish()));
        if regular_fails || custom_fails {
            let first = result.expect_err("capture retirement propagates its first failure");
            let expected = if regular_fails {
                "regular capture"
            } else {
                "custom capture"
            };
            assert_eq!(first.downcast_ref::<&str>().copied(), Some(expected));
        } else {
            result.expect("healthy captures retire normally");
        }
        let expected = if regular_fails { 1 } else { 2 };
        assert_eq!(
            retired.load(Ordering::SeqCst),
            expected,
            "the opaque tail is retained after failure"
        );

        let mut fresh = SemanticsConfiguration::new();
        fresh.add_action(SemanticsAction::Click, handler(false, "healthy regular"));
        let _ = fresh.add_custom_action("healthy custom", handler(false, "healthy custom"));
        let mut recovery = PanicRecovery::new();
        fresh.retire(&mut recovery.scope());
        recovery.finish();
        assert_eq!(retired.load(Ordering::SeqCst), expected + 2);
    }
}

fn unused_reveal_snapshot_retirement_worker(competing: bool) {
    let mut fixture = RevealFixture::with_retirement_failures([true, competing]);
    let held = fixture.resolve();
    fixture.owner.clear();
    let first = catch_unwind(AssertUnwindSafe(|| drop(held)))
        .expect_err("unused snapshot capture retirement fails");
    assert_eq!(
        first.downcast_ref::<&str>().copied(),
        Some("inner capture retirement")
    );
    assert_eq!(
        fixture.retired.load(Ordering::SeqCst),
        1,
        "first failed retirement retains remaining snapshot ownership"
    );
    assert!(
        fixture.calls.lock().is_empty(),
        "intentionally dropped snapshot never delivers callbacks"
    );
    let healthy = RevealFixture::new(false);
    healthy.resolve().invoke();
    assert_eq!(*healthy.calls.lock(), ["inner", "outer"]);
    let retired = Arc::clone(&healthy.retired);
    drop(healthy);
    assert_eq!(
        retired.load(Ordering::SeqCst),
        2,
        "fresh healthy ownership still retires"
    );
}

fn unused_reveal_snapshot_contains_single_and_competing_capture_retirement() {
    // The baseline owns both failing captures in a Vec: its implicit Drop can
    // double-panic before an outer catch is reached. Keep that causal failure
    // isolated without introducing a hostile panic-payload destructor.
    let executable = std::env::current_exe().expect("integration test executable");
    let mut failures = Vec::new();
    for competing in [false, true] {
        let result = std::process::Command::new(&executable)
            .args([
                "--exact",
                "descendant_reveal_snapshots_preserve_identity_failure_and_recovery",
                "--nocapture",
            ])
            .env(
                "FLUI_REVEAL_RETIREMENT_WORKER",
                if competing { "competing" } else { "single" },
            )
            .env("RUST_BACKTRACE", "0")
            .output()
            .expect("start isolated capture-retirement witness");
        if !result.status.success() {
            failures.push(competing);
            eprintln!(
                "unused snapshot competing={competing}: {}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    assert!(
        failures.is_empty(),
        "unused reveal snapshot retirement failed for competing={failures:?}"
    );
}

#[test]
fn descendant_reveal_snapshots_preserve_identity_failure_and_recovery() {
    if let Ok(mode) = std::env::var("FLUI_REVEAL_RETIREMENT_WORKER") {
        unused_reveal_snapshot_retirement_worker(mode == "competing");
        return;
    }
    let cases: &[(&str, fn())] = &[
        (
            "replaced membership and recovery",
            reveal_stale_snapshot_is_refused_and_fresh_replacement_recovers,
        ),
        (
            "removed and reparented geometry",
            reveal_removed_and_reparented_paths_do_not_deliver_old_geometry,
        ),
        (
            "single and competing body failure",
            reveal_first_body_failure_keeps_live_outer_progress_and_recovery,
        ),
        (
            "healthy and failed capture retirement",
            reveal_healthy_capture_retirement_and_failed_retirement_keep_authority,
        ),
        (
            "unused snapshot single and competing retirement",
            unused_reveal_snapshot_contains_single_and_competing_capture_retirement,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, run) in cases {
        if let Err(payload) = catch_unwind(run) {
            failures.push(name);
            std::mem::forget(payload);
        }
    }
    assert!(
        failures.is_empty(),
        "descendant reveal rows failed: {failures:?}"
    );
}

/// A relation resolves to the published node declaring its anchor, on the full
/// path and incrementally: when the target remounts under a new identity while
/// its source is unchanged, the source is republished pointing at the new
/// node; when no node declares the anchor, the relation is dropped (ADR-0205).
#[test]
fn relations_resolve_through_anchors_across_publication() {
    use flui_semantics::{SemanticsAnchor, SemanticsRelation};

    let render = |index: u32| {
        RenderId::new_gen(
            index,
            core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
        )
    };
    let published = |index: u32| {
        accesskit::NodeId(flui_semantics::AccessibilityNodeId::from(render(index)).as_u64())
    };
    let title = SemanticsAnchor::new();
    let unknown = SemanticsAnchor::new();

    let titled = |index: u32, anchored: bool| {
        let mut title_node = node(index, "Delete file?");
        if anchored {
            title_node.config_mut().add_anchor(title.clone());
        }
        title_node
    };
    let dialog_in = |update: &TreeUpdate| {
        update
            .nodes
            .iter()
            .find(|(id, _)| *id == published(2))
            .map(|(_, node)| node.clone())
    };

    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let first_title = owner.insert(titled(1, true));
    let mut dialog = SemanticsNode::new().with_source_render_id(render(2));
    dialog
        .config_mut()
        .add_relation(SemanticsRelation::LabelledBy, title.clone());
    dialog
        .config_mut()
        .add_relation(SemanticsRelation::Controls, unknown);
    let dialog = owner.insert(dialog);
    owner.add_child(root, first_title);
    owner.add_child(root, dialog);
    owner.set_root(Some(root));
    owner.flush();
    let full = dialog_in(&received.lock()[0]).expect("the full update carries the dialog");
    assert_eq!(full.labelled_by(), &[published(1)]);
    assert!(
        full.controls().is_empty(),
        "a relation to an anchor no node declares is dropped"
    );

    // Only the title remounts; the dialog's own node stays clean, as it does
    // under a mark-scoped graft that leaves its subtree alone.
    owner.remove_child(root, first_title);
    owner.remove(first_title);
    let second_title = owner.insert(titled(3, true));
    owner.add_child(root, second_title);
    owner.flush();
    let moved = dialog_in(received.lock().last().expect("an update"))
        .expect("the unchanged dialog is republished when its target moves");
    assert_eq!(moved.labelled_by(), &[published(3)]);

    owner.remove_child(root, second_title);
    owner.remove(second_title);
    let unanchored = owner.insert(titled(4, false));
    owner.add_child(root, unanchored);
    owner.flush();
    let dropped = dialog_in(received.lock().last().expect("an update"))
        .expect("the dialog is republished when its target goes away");
    assert_eq!(dropped.labelled_by(), &[] as &[accesskit::NodeId]);
}

/// Each relation reaches its AccessKit property through the same anchor
/// resolution as `labelled_by`. A single-target property takes the first
/// target in identity order, whichever node declared the anchor first, and a
/// relation to an anchor no node declares is dropped.
#[test]
fn every_relation_resolves_to_its_accesskit_property() {
    use flui_semantics::{SemanticsAnchor, SemanticsRelation};

    let render = |index: u32| {
        RenderId::new_gen(
            index,
            core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
        )
    };
    let published = |index: u32| {
        accesskit::NodeId(flui_semantics::AccessibilityNodeId::from(render(index)).as_u64())
    };
    let help = SemanticsAnchor::new();
    let details = SemanticsAnchor::new();
    let next = SemanticsAnchor::new();
    let error = SemanticsAnchor::new();
    let option = SemanticsAnchor::new();
    let unknown = SemanticsAnchor::new();

    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    // The error anchor is declared by two nodes, the later one first.
    let targets = [
        (5, "Must contain @", error.clone()),
        (1, "Your work address", help),
        (2, "Address book", details),
        (3, "Password", next),
        (4, "Required", error),
        (6, "name@example.com", option),
    ];
    let mut field = SemanticsNode::new().with_source_render_id(render(9));
    for (kind, anchor) in [
        (SemanticsRelation::DescribedBy, &targets[1].2),
        (SemanticsRelation::Details, &targets[2].2),
        (SemanticsRelation::FlowTo, &targets[3].2),
        (SemanticsRelation::ErrorMessage, &targets[0].2),
        (SemanticsRelation::ActiveDescendant, &targets[5].2),
        (SemanticsRelation::Controls, &unknown),
    ] {
        field.config_mut().add_relation(kind, anchor.clone());
    }
    for (index, label, anchor) in targets {
        let mut target = node(index, label);
        target.config_mut().add_anchor(anchor);
        let target = owner.insert(target);
        owner.add_child(root, target);
    }
    let field = owner.insert(field);
    owner.add_child(root, field);
    owner.set_root(Some(root));
    owner.flush();

    let update = &received.lock()[0];
    let published_field = update
        .nodes
        .iter()
        .find(|(id, _)| *id == published(9))
        .map(|(_, node)| node.clone())
        .expect("the full update carries the field");
    assert_eq!(published_field.described_by(), &[published(1)]);
    assert_eq!(published_field.details(), &[published(2)]);
    assert_eq!(published_field.flow_to(), &[published(3)]);
    assert_eq!(
        published_field.error_message(),
        Some(published(4)),
        "the first target in identity order, not in declaration order"
    );
    assert_eq!(published_field.active_descendant(), Some(published(6)));
    assert_eq!(
        published_field.controls(),
        &[] as &[accesskit::NodeId],
        "a relation to an anchor no node declares is dropped"
    );
}

/// A node whose only annotation is its selection still reaches the platform:
/// an annotation consisting of one declared state is not an empty one.
#[test]
fn a_selection_only_annotation_reaches_the_platform() {
    let mut config = flui_semantics::SemanticsConfiguration::new();
    config.set_selected(true);
    let (mut owner, updates) = recording_owner();
    let root = owner.insert(node(0, "").with_config(config));
    owner.set_root(Some(root));
    owner.flush();
    let updates = updates.lock();
    assert_eq!(
        updates.len(),
        1,
        "selection-only annotation must reach the platform"
    );
    assert_eq!(updates[0].nodes[0].1.is_selected(), Some(true));
}

/// Two live nodes sharing one identity are an assembly defect AccessKit
/// cannot express — one node per id, and its consumer panics on an id two
/// parents list. Nothing is published while the defect stands, the tree
/// stays dirty, and the first flush after it clears delivers a valid tree.
#[test]
fn an_aliased_identity_publishes_nothing_until_it_clears() {
    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let left = owner.insert(node(1, "left"));
    let right = owner.insert(node(2, "right"));
    let first = owner.insert(node(5, "first"));
    let second = owner.insert(node(5, "second"));
    owner.add_child(root, left);
    owner.add_child(root, right);
    owner.add_child(left, first);
    owner.add_child(right, second);
    owner.set_root(Some(root));
    assert_eq!(owner.flush(), 0, "an aliased tree is not published");
    assert!(received.lock().is_empty());
    let aliased = owner
        .get(first)
        .and_then(SemanticsNode::accessibility_id)
        .expect("render-backed identity");
    assert_eq!(
        owner
            .resolve_action(flui_semantics::SemanticsActionRequest::new(
                aliased,
                flui_semantics::ActionCall::Click,
            ))
            .err(),
        Some(flui_semantics::SemanticsActionError::AmbiguousNode { node_id: aliased }),
        "a request cannot pick one of two nodes sharing its identity"
    );

    owner.remove_child(right, second);
    owner.remove(second);
    assert!(
        owner.flush() > 0,
        "the tree publishes once the alias is gone"
    );
    let update = received.lock()[0].clone();
    let _ = accesskit_consumer::Tree::new(update, true);
}

/// An unattached node sharing a published identity is never published, so it
/// neither withholds the tree nor has its payload diffed out under the
/// rooted node's id.
#[test]
fn an_unattached_alias_does_not_withhold_the_tree() {
    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let child = owner.insert(node(1, "child"));
    owner.add_child(root, child);
    let _orphan = owner.insert(node(1, "orphan"));
    owner.set_root(Some(root));
    assert!(
        owner.flush() > 0,
        "the rooted tree publishes beside an orphan alias"
    );

    let _late = owner.insert(node(1, "late orphan"));
    owner.flush();
    let updates = received.lock();
    for update in updates.iter() {
        let _ = accesskit_consumer::Tree::new(update.clone(), true);
        for (_, published) in &update.nodes {
            assert!(
                !matches!(published.label(), Some("orphan" | "late orphan")),
                "an unattached alias never reaches the adapter"
            );
        }
    }
}

/// A cycle built through the public node APIs is refused: AccessKit assumes a
/// tree, so a full publish that reaches a node twice publishes nothing.
#[test]
fn a_cyclic_tree_is_not_published() {
    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let child = owner.insert(node(1, "child"));
    owner.add_child(root, child);
    owner
        .tree_mut()
        .get_mut(child)
        .expect("the child is live")
        .add_child(root);
    owner.set_root(Some(root));
    assert_eq!(owner.flush(), 0, "a cyclic tree is not published");
    assert!(received.lock().is_empty());
}

/// A cycle introduced after a valid publication is not diffed out to the
/// adapter: the incremental path sees the edge does not link back and the
/// full path refuses the shape.
#[test]
fn a_cycle_after_publication_is_not_published() {
    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let child = owner.insert(node(1, "child"));
    owner.add_child(root, child);
    owner.set_root(Some(root));
    assert!(owner.flush() > 0);
    owner
        .tree_mut()
        .get_mut(child)
        .expect("the child is live")
        .add_child(root);
    assert_eq!(owner.flush(), 0, "the cyclic edge is not published");
    assert_eq!(received.lock().len(), 1);
}

/// A full update carries only what the root reaches: AccessKit rejects a full
/// update with a node no parent lists.
#[test]
fn a_full_update_leaves_out_unattached_nodes() {
    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let child = owner.insert(node(1, "child"));
    let _orphan = owner.insert(node(2, "orphan"));
    owner.add_child(root, child);
    owner.set_root(Some(root));
    owner.flush();
    let update = received.lock()[0].clone();
    assert_eq!(update.nodes.len(), 2, "the orphan is not published");
    let _ = accesskit_consumer::Tree::new(update, true);
}

/// An unaddressable node is not published and its parent lists none of its
/// children, so its whole subtree is left out — before and after a change
/// under it.
#[test]
fn an_unaddressable_node_leaves_out_its_subtree() {
    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let unaddressable = owner.insert(SemanticsNode::new());
    let grandchild = owner.insert(node(2, "grandchild"));
    owner.add_child(root, unaddressable);
    owner.add_child(unaddressable, grandchild);
    owner.set_root(Some(root));
    owner.flush();
    owner
        .tree_mut()
        .get_mut(grandchild)
        .expect("the grandchild is live")
        .config_mut()
        .set_label("renamed");
    owner.flush();
    for update in received.lock().iter() {
        assert!(
            update
                .nodes
                .iter()
                .all(|(_, published)| published.label() == Some("root")),
            "only the root is published"
        );
        let _ = accesskit_consumer::Tree::new(update.clone(), true);
    }
}

/// A node moved under an unaddressable one leaves the published tree, so a
/// request still naming it is stale and reaches no handler.
#[test]
fn an_action_under_an_unaddressable_node_is_not_found() {
    use flui_semantics::{
        ActionCall, SemanticsAction, SemanticsActionError, SemanticsActionRequest,
    };

    let (mut owner, _) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let mut button = node(1, "button");
    let target = button.accessibility_id().expect("render-backed identity");
    let calls = Arc::new(Mutex::new(0_u32));
    let counter = Arc::clone(&calls);
    button.config_mut().add_action(
        SemanticsAction::Click,
        Rc::new(move |_| *counter.lock() += 1),
    );
    let button = owner.insert(button);
    owner.add_child(root, button);
    owner.set_root(Some(root));
    owner.flush();
    let request = || SemanticsActionRequest::new(target, ActionCall::Click);
    owner
        .resolve_action(request())
        .expect("the published button is actionable")
        .invoke();

    let unaddressable = owner.insert(SemanticsNode::new());
    owner.remove_child(root, button);
    owner.add_child(root, unaddressable);
    owner.add_child(unaddressable, button);
    owner.flush();
    assert_eq!(
        owner.resolve_action(request()).err(),
        Some(SemanticsActionError::NodeNotFound { node_id: target })
    );
    assert_eq!(*calls.lock(), 1, "the stale request reached no handler");
}

/// An unattached node that declares a relation is never published, not even
/// by the incremental flush that re-translates relation sources: the adapter
/// would receive a node no parent lists.
#[test]
fn an_unattached_relation_source_stays_unpublished() {
    use flui_semantics::{SemanticsAnchor, SemanticsRelation};

    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let title = SemanticsAnchor::new();
    let mut titled = node(1, "Title");
    titled.config_mut().add_anchor(title.clone());
    let titled = owner.insert(titled);
    owner.add_child(root, titled);
    let mut orphan = node(9, "orphan");
    orphan
        .config_mut()
        .add_relation(SemanticsRelation::LabelledBy, title);
    let _orphan = owner.insert(orphan);
    owner.set_root(Some(root));
    owner.flush();
    owner.mark_dirty(titled);
    owner.flush();

    let orphan_id = accesskit::NodeId(
        flui_semantics::AccessibilityNodeId::from(RenderId::new_gen(
            9,
            core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
        ))
        .as_u64(),
    );
    for update in received.lock().iter() {
        assert!(
            update.nodes.iter().all(|(id, _)| *id != orphan_id),
            "the unattached source is never published"
        );
    }
}

/// A child given a second parent through the public node APIs, while the
/// first still lists it, is not diffed out to the adapter: a reshaping
/// change goes through the full publish, which refuses the shape.
#[test]
fn a_child_under_two_parents_is_not_published() {
    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let old_parent = owner.insert(node(1, "old"));
    let new_parent = owner.insert(node(2, "new"));
    let child = owner.insert(node(3, "child"));
    owner.add_child(root, old_parent);
    owner.add_child(root, new_parent);
    owner.add_child(old_parent, child);
    owner.set_root(Some(root));
    assert!(owner.flush() > 0);

    owner
        .tree_mut()
        .get_mut(new_parent)
        .expect("live")
        .add_child(child);
    owner
        .tree_mut()
        .get_mut(child)
        .expect("live")
        .set_parent(Some(new_parent));
    assert_eq!(owner.flush(), 0, "two parents are not published");
    assert_eq!(received.lock().len(), 1);
}

/// A relation never names a node the adapter does not hold: a rooted source
/// whose anchor sits on an unattached node publishes no target for it, on the
/// full flush or an incremental one.
#[test]
fn a_relation_to_an_unattached_target_names_nothing() {
    use flui_semantics::{SemanticsAnchor, SemanticsRelation};

    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let anchor = SemanticsAnchor::new();
    let mut source = node(1, "dialog");
    source
        .config_mut()
        .add_relation(SemanticsRelation::LabelledBy, anchor.clone());
    let source = owner.insert(source);
    owner.add_child(root, source);
    let mut orphan = node(9, "title");
    orphan.config_mut().add_anchor(anchor);
    let _orphan = owner.insert(orphan);
    owner.set_root(Some(root));
    owner.flush();
    owner.mark_dirty(source);
    owner.flush();

    for update in received.lock().iter() {
        for (_, published) in &update.nodes {
            assert!(
                published.labelled_by().is_empty(),
                "the unattached target is never named"
            );
        }
    }
}

/// Focus is claimed only among published nodes: an unattached node marked
/// focused does not make the published focused control ambiguous, on the full
/// flush or an incremental one.
#[test]
fn an_unattached_focus_claim_does_not_steal_focus() {
    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let mut field = node(1, "field");
    field.config_mut().set_focused(true);
    let field = owner.insert(field);
    owner.add_child(root, field);
    let mut orphan = node(9, "orphan");
    orphan.config_mut().set_focused(true);
    let _orphan = owner.insert(orphan);
    owner.set_root(Some(root));
    owner.flush();
    owner.mark_dirty(field);
    owner.flush();

    let field_id = accesskit::NodeId(
        flui_semantics::AccessibilityNodeId::from(RenderId::new_gen(
            1,
            core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
        ))
        .as_u64(),
    );
    for update in received.lock().iter() {
        assert_eq!(update.focus, field_id, "the published field keeps focus");
    }
}

/// The custom actions recorded for a published identity are the rooted
/// node's: an unattached alias of it, with actions of its own, neither
/// replaces them nor makes the rooted node's action stale.
#[test]
fn an_unattached_alias_does_not_replace_published_custom_actions() {
    use flui_semantics::{ActionCall, SemanticsActionRequest};

    let (mut owner, _) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let mut item = node(1, "item");
    let target = item.accessibility_id().expect("render-backed identity");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let ran = Arc::clone(&calls);
    let archive = item
        .config_mut()
        .add_custom_action("Archive", Rc::new(move |_| ran.lock().push("item")));
    let item = owner.insert(item);
    owner.add_child(root, item);
    owner.set_root(Some(root));
    let mut orphan = node(1, "orphan");
    orphan
        .config_mut()
        .add_custom_action("Delete", Rc::new(|_| panic!("the orphan is never run")));
    let _orphan = owner.insert(orphan);
    owner.flush();

    owner
        .resolve_action(SemanticsActionRequest::new(
            target,
            ActionCall::CustomAction(archive),
        ))
        .expect("the published node's action resolves")
        .invoke();
    assert_eq!(*calls.lock(), ["item"]);
}

/// A node named by another through `labelled_by` reads that node's text as
/// its name through `accesskit_consumer`, as every adapter computes it: a
/// field labelled by a separate static text is announced by that text. A role
/// ARIA forbids a name, a paragraph, gets no name from it: the labelling
/// node's text leads its description instead, and follows that text when it
/// changes under a clean paragraph.
#[test]
fn a_labelled_by_target_names_its_source_through_the_consumer() {
    use flui_semantics::SemanticsRole;

    let (_, received, _) = labelled_by_caption(SemanticsRole::TextInput, |anchor| {
        caption_node("Email", anchor)
    });
    let field = read_source(&received.lock()[0], accesskit::Role::TextInput);
    assert_eq!(field, (Some("Email".into()), None));

    let (mut paragraph, received, (caption, anchor)) =
        labelled_by_caption(SemanticsRole::Paragraph, |anchor| {
            caption_node("Email", anchor)
        });
    let read = read_source(&received.lock()[0], accesskit::Role::Paragraph);
    assert_eq!(
        read,
        (None, Some("Email".into())),
        "a paragraph takes no name; the caption's text describes it"
    );

    paragraph
        .tree_mut()
        .replace_node(caption, caption_node("Work email", &anchor));
    paragraph.flush();
    let update = received
        .lock()
        .last()
        .cloned()
        .expect("the change is published");
    let renamed = update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == accesskit::Role::Paragraph)
        .and_then(|(_, node)| node.description().map(str::to_owned));
    assert_eq!(
        renamed.as_deref(),
        Some("Work email"),
        "the clean paragraph follows its caption's text"
    );

    // A target's value names nothing: a password field labelling a paragraph
    // does not hand it the password.
    let (_, received, _) = labelled_by_caption(SemanticsRole::Paragraph, |anchor| {
        let mut password = SemanticsNode::new().with_source_render_id(RenderId::new_gen(
            1,
            core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
        ));
        password.config_mut().set_role(SemanticsRole::PasswordInput);
        password.config_mut().set_value("hunter2");
        password.config_mut().add_anchor(anchor.clone());
        password
    });
    let read = read_source(&received.lock()[0], accesskit::Role::Paragraph);
    assert_eq!(read, (None, None), "the password stays on its own node");

    // A paragraph names nothing in an adapter's eyes, so a field it labels
    // is named from its text here.
    let (_, received, _) = labelled_by_caption(SemanticsRole::TextInput, |anchor| {
        let mut paragraph = caption_node("Email", anchor);
        paragraph.config_mut().set_role(SemanticsRole::Paragraph);
        paragraph
    });
    let field = read_source(&received.lock()[0], accesskit::Role::TextInput);
    assert_eq!(
        field.0.as_deref(),
        Some("Email"),
        "the paragraph names the field"
    );

    // A static text is named by its label and value together; a paragraph it
    // labels carries both.
    let (_, received, _) = labelled_by_caption(SemanticsRole::Paragraph, |anchor| {
        let mut total = caption_node("Total", anchor);
        total.config_mut().set_value("42");
        total
    });
    let read = read_source(&received.lock()[0], accesskit::Role::Paragraph);
    assert_eq!(read, (None, Some("Total\n42".into())));
}

/// A static text reading `text` that declares `anchor`.
fn caption_node(text: &str, anchor: &flui_semantics::SemanticsAnchor) -> SemanticsNode {
    let mut label = node(1, text);
    label.config_mut().add_anchor(anchor.clone());
    label
}

/// Publishes a node of `role` labelled by a separate node, `caption`, built
/// around the anchor; returns the owner, what it delivered, and the caption's
/// node and anchor.
fn labelled_by_caption(
    role: flui_semantics::SemanticsRole,
    caption: impl FnOnce(&flui_semantics::SemanticsAnchor) -> SemanticsNode,
) -> (
    SemanticsOwner,
    Arc<Mutex<Vec<TreeUpdate>>>,
    (
        flui_foundation::SemanticsId,
        flui_semantics::SemanticsAnchor,
    ),
) {
    use flui_semantics::{SemanticsAnchor, SemanticsRelation};

    let (mut owner, received) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let anchor = SemanticsAnchor::new();
    let label = owner.insert(caption(&anchor));
    let mut field = SemanticsNode::new().with_source_render_id(RenderId::new_gen(
        2,
        core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
    ));
    field.config_mut().set_role(role);
    field
        .config_mut()
        .add_relation(SemanticsRelation::LabelledBy, anchor.clone());
    let field = owner.insert(field);
    owner.add_child(root, label);
    owner.add_child(root, field);
    owner.set_root(Some(root));
    owner.flush();
    (owner, received, (label, anchor))
}

/// The name and description `accesskit_consumer` reads for the node of
/// `role` in `update`.
fn read_source(update: &TreeUpdate, role: accesskit::Role) -> (Option<String>, Option<String>) {
    let tree = accesskit_consumer::Tree::new(update.clone(), true);
    let state = tree.state();
    let source = state
        .root()
        .children()
        .find(|node| node.role() == role)
        .expect("the source is published");
    (source.label(), source.description())
}

/// An owner whose adapter rejects updates while `failing` is set: its
/// callback panics before accepting one, so the owner's record of what was
/// delivered stays where the last accepted update left it.
struct RejectingAdapter {
    owner: SemanticsOwner,
    failing: Arc<AtomicBool>,
}

impl RejectingAdapter {
    fn new() -> Self {
        let failing = Arc::new(AtomicBool::new(false));
        let reject = Arc::clone(&failing);
        let owner = SemanticsOwner::new(Arc::new(move |_: &TreeUpdate| {
            assert!(
                !reject.load(Ordering::Relaxed),
                "the adapter rejected the update"
            );
        }));
        Self { owner, failing }
    }

    /// Roots the tree at `root` and delivers it.
    fn root(&mut self, root: SemanticsNode) -> flui_foundation::SemanticsId {
        let id = self.owner.insert(root);
        self.owner.set_root(Some(id));
        self.deliver();
        id
    }

    fn deliver(&mut self) {
        assert!(self.owner.flush() > 0, "the adapter accepts the update");
    }

    /// Flushes the pending change into an update the adapter rejects.
    fn reject(&mut self) {
        self.failing.store(true, Ordering::Relaxed);
        let failure = catch_unwind(AssertUnwindSafe(|| self.owner.flush()))
            .expect_err("the adapter rejects the update");
        self.failing.store(false, Ordering::Relaxed);
        let rejected = failure.downcast_ref::<&str>().map_or_else(
            || failure.downcast_ref::<String>().map(String::as_str),
            |message| Some(*message),
        ) == Some("the adapter rejected the update");
        assert!(rejected, "the rejection is the failure");
        assert!(
            self.owner.needs_flush(),
            "the rejected change stays pending"
        );
    }

    fn resolve(
        &self,
        target: flui_semantics::AccessibilityNodeId,
        call: flui_semantics::ActionCall,
    ) -> Result<flui_semantics::SemanticsActionInvocation, flui_semantics::SemanticsActionError>
    {
        self.owner
            .resolve_action(flui_semantics::SemanticsActionRequest::new(target, call))
    }
}

fn unsupported(
    target: flui_semantics::AccessibilityNodeId,
    action: flui_semantics::SemanticsAction,
) -> flui_semantics::SemanticsActionError {
    flui_semantics::SemanticsActionError::UnsupportedAction {
        node_id: target,
        action,
    }
}

/// A collapsed disclosure delivered, then expanded in an update the adapter
/// rejected: neither transition is admitted until the expanded one is
/// delivered.
fn a_disclosure_admits_the_transition_both_states_allow() {
    use flui_semantics::{ActionCall, SemanticsAction, SemanticsRole};
    let clicks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let disclosure = |expanded: bool| {
        let mut node = node(1, "Details");
        node.config_mut().set_role(SemanticsRole::Button);
        node.config_mut().set_expanded(expanded);
        let clicks = Arc::clone(&clicks);
        node.config_mut().add_action(
            SemanticsAction::Click,
            Rc::new(move |_| {
                clicks.fetch_add(1, Ordering::SeqCst);
            }),
        );
        node
    };
    let collapsed = disclosure(false);
    let target = collapsed
        .accessibility_id()
        .expect("render-backed identity");
    let mut adapter = RejectingAdapter::new();
    let id = adapter.root(collapsed);

    adapter.owner.tree_mut().replace_node(id, disclosure(true));
    adapter.reject();
    assert_eq!(
        adapter.resolve(target, ActionCall::Expand).err(),
        Some(unsupported(target, SemanticsAction::Expand)),
        "only the delivered node offered Expand"
    );
    assert_eq!(
        adapter.resolve(target, ActionCall::Collapse).err(),
        Some(unsupported(target, SemanticsAction::Collapse)),
        "only the rejected node offers Collapse"
    );

    adapter.deliver();
    adapter
        .resolve(target, ActionCall::Collapse)
        .expect("the delivered expanded node offers Collapse")
        .invoke();
    assert_eq!(clicks.load(Ordering::SeqCst), 1);
}

/// An expandable button delivered, then replaced by a paragraph with a
/// context menu in an update the adapter rejected: only the click both offer
/// is admitted.
fn a_role_change_admits_only_the_actions_both_nodes_offer() {
    use flui_semantics::{ActionCall, SemanticsAction, SemanticsRole};
    let with_click = |role, label: &str| {
        let mut node = node(1, label);
        node.config_mut().set_role(role);
        node.config_mut().set_expanded(false);
        node.config_mut()
            .add_action(SemanticsAction::Click, Rc::new(|_| {}));
        node
    };
    let button = with_click(SemanticsRole::Button, "More");
    let target = button.accessibility_id().expect("render-backed identity");
    let mut adapter = RejectingAdapter::new();
    let id = adapter.root(button);

    let mut paragraph = with_click(SemanticsRole::Paragraph, "More");
    paragraph
        .config_mut()
        .add_action(SemanticsAction::ShowContextMenu, Rc::new(|_| {}));
    adapter.owner.tree_mut().replace_node(id, paragraph);
    adapter.reject();
    assert_eq!(
        adapter.resolve(target, ActionCall::Expand).err(),
        Some(unsupported(target, SemanticsAction::Expand)),
        "only the delivered button offered Expand"
    );
    assert_eq!(
        adapter.resolve(target, ActionCall::ShowContextMenu).err(),
        Some(unsupported(target, SemanticsAction::ShowContextMenu)),
        "only the rejected paragraph offers a context menu"
    );
    adapter
        .resolve(target, ActionCall::Click)
        .expect("both nodes offer Click")
        .invoke();

    adapter.deliver();
    adapter
        .resolve(target, ActionCall::ShowContextMenu)
        .expect("the delivered paragraph offers a context menu")
        .invoke();
}

/// A range delivered as 0..=10, then moved to 5..=20 in an update the
/// adapter rejected: a value is admitted only inside both.
fn a_numeric_write_is_admitted_inside_both_ranges() {
    use flui_semantics::{ActionCall, NumericRange, SemanticsAction, SemanticsActionError};
    let received = Arc::new(Mutex::new(Vec::new()));
    let slider = |min, max| {
        let mut node = node(1, "Volume");
        node.config_mut()
            .set_numeric_range(NumericRange::try_new(min, min, max, 1.0).expect("finite range"));
        let sink = Arc::clone(&received);
        node.config_mut().add_action(
            SemanticsAction::SetNumericValue,
            Rc::new(move |call| {
                let ActionCall::SetNumericValue(value) = call else {
                    panic!("the numeric handler received {call:?}");
                };
                sink.lock().push(value);
            }),
        );
        node
    };
    let delivered = slider(0.0, 10.0);
    let target = delivered
        .accessibility_id()
        .expect("render-backed identity");
    let mut adapter = RejectingAdapter::new();
    let id = adapter.root(delivered);

    adapter.owner.tree_mut().replace_node(id, slider(5.0, 20.0));
    adapter.reject();
    let invalid = Some(SemanticsActionError::InvalidNumericValue { node_id: target });
    for (call, why) in [
        (
            ActionCall::SetNumericValue(2.0),
            "only the delivered range admits 2",
        ),
        (
            ActionCall::SetNumericValue(15.0),
            "only the rejected range admits 15",
        ),
        (
            ActionCall::SetValue("15".into()),
            "a numeric text is checked the same way",
        ),
    ] {
        assert_eq!(adapter.resolve(target, call).err(), invalid, "{why}");
    }
    adapter
        .resolve(target, ActionCall::SetNumericValue(7.0))
        .expect("both ranges admit 7")
        .invoke();

    adapter.deliver();
    adapter
        .resolve(target, ActionCall::SetNumericValue(15.0))
        .expect("the delivered range admits 15")
        .invoke();
    assert_eq!(*received.lock(), [7.0, 15.0]);
}

/// A text field delivered, then turned into a numeric-only range in an update
/// the adapter rejected: a text write meant for the field is not read as a
/// number until the range is delivered.
fn a_text_write_is_read_as_a_number_only_where_a_range_was_delivered() {
    use flui_semantics::{ActionCall, NumericRange, SemanticsAction, SemanticsRole};
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut field = node(1, "Amount");
    field.config_mut().set_role(SemanticsRole::TextInput);
    field.config_mut().set_value("draft");
    field
        .config_mut()
        .add_action(SemanticsAction::SetValue, Rc::new(|_| {}));
    let target = field.accessibility_id().expect("render-backed identity");
    let mut adapter = RejectingAdapter::new();
    let id = adapter.root(field);

    let mut range = node(1, "Amount");
    range.config_mut().set_role(SemanticsRole::Slider);
    range
        .config_mut()
        .set_numeric_range(NumericRange::try_new(0.0, 0.0, 10.0, 1.0).expect("finite range"));
    let sink = Arc::clone(&received);
    range.config_mut().add_action(
        SemanticsAction::SetNumericValue,
        Rc::new(move |call| {
            let ActionCall::SetNumericValue(value) = call else {
                panic!("the numeric handler received {call:?}");
            };
            sink.lock().push(value);
        }),
    );
    adapter.owner.tree_mut().replace_node(id, range);
    adapter.reject();
    assert_eq!(
        adapter
            .resolve(target, ActionCall::SetValue("5".into()))
            .err(),
        Some(unsupported(target, SemanticsAction::SetValue)),
        "the text was written to the delivered field"
    );

    adapter.deliver();
    adapter
        .resolve(target, ActionCall::SetValue("5".into()))
        .expect("the delivered range reads the text as a number")
        .invoke();
    assert_eq!(*received.lock(), [5.0]);
}

/// A child added in an update the adapter rejected was offered nothing.
fn a_node_the_adapter_was_not_given_is_not_found() {
    use flui_semantics::{ActionCall, SemanticsAction, SemanticsActionError};
    let mut adapter = RejectingAdapter::new();
    let root = adapter.root(node(0, "root"));

    let mut button = node(1, "Send");
    button
        .config_mut()
        .add_action(SemanticsAction::Click, Rc::new(|_| {}));
    let target = button.accessibility_id().expect("render-backed identity");
    let button = adapter.owner.insert(button);
    adapter.owner.add_child(root, button);
    adapter.reject();
    assert_eq!(
        adapter.resolve(target, ActionCall::Click).err(),
        Some(SemanticsActionError::NodeNotFound { node_id: target }),
        "the adapter does not hold the child"
    );

    adapter.deliver();
    adapter
        .resolve(target, ActionCall::Click)
        .expect("the delivered child offers Click")
        .invoke();
}

/// A reveal target whose visibility changed in an update the adapter
/// rejected is revealed only while both nodes are visible, whether its
/// scrollable ancestors or its own handler would serve the reveal.
fn a_reveal_is_admitted_only_while_both_nodes_are_visible() {
    use flui_foundation::geometry::Rect;
    use flui_semantics::{ActionCall, SemanticsAction};
    for (delivered_hidden, own_handler) in
        [(true, false), (false, false), (true, true), (false, true)]
    {
        let target_node = |hidden: bool| {
            let mut node = node(1, "Row");
            node.set_reveal_rect(Rect::new(0.0, 600.0, 40.0, 640.0));
            node.config_mut().set_has_reveal_ancestor(true);
            node.config_mut().set_hidden(hidden);
            if own_handler {
                node.config_mut()
                    .add_action(SemanticsAction::ScrollIntoView, Rc::new(|_| {}));
            }
            node
        };
        let mut scroller = node(0, "List");
        scroller.set_reveal_rect(Rect::new(0.0, 0.0, 200.0, 200.0));
        scroller
            .config_mut()
            .add_action(SemanticsAction::ScrollIntoView, Rc::new(|_| {}));
        let mut adapter = RejectingAdapter::new();
        let scroller = adapter.owner.insert(scroller);
        let delivered = target_node(delivered_hidden);
        let target = delivered
            .accessibility_id()
            .expect("render-backed identity");
        let row = adapter.owner.insert(delivered);
        adapter.owner.add_child(scroller, row);
        adapter.owner.set_root(Some(scroller));
        adapter.deliver();

        adapter
            .owner
            .tree_mut()
            .replace_node(row, target_node(!delivered_hidden));
        adapter.reject();
        assert_eq!(
            adapter
                .resolve(target, ActionCall::ScrollIntoView(None))
                .err(),
            Some(unsupported(target, SemanticsAction::ScrollIntoView)),
            "one of the two nodes is hidden \
             (delivered hidden: {delivered_hidden}, own handler: {own_handler})"
        );

        adapter.deliver();
        assert_eq!(
            adapter
                .resolve(target, ActionCall::ScrollIntoView(None))
                .is_ok(),
            delivered_hidden,
            "only a visible node is revealed once both agree (own handler: {own_handler})"
        );
    }
}

/// A child removed in an update the adapter rejected is still held by it: a
/// child restored before the retry stays actionable.
fn a_removal_the_adapter_rejected_leaves_the_child_it_holds() {
    use flui_semantics::{ActionCall, SemanticsAction};
    let button = || {
        let mut button = node(1, "Send");
        button
            .config_mut()
            .add_action(SemanticsAction::Click, Rc::new(|_| {}));
        button
    };
    let mut adapter = RejectingAdapter::new();
    let root = adapter.owner.insert(node(0, "root"));
    let child = button();
    let target = child.accessibility_id().expect("render-backed identity");
    let child = adapter.owner.insert(child);
    adapter.owner.add_child(root, child);
    adapter.owner.set_root(Some(root));
    adapter.deliver();

    drop(adapter.owner.remove(child));
    adapter.reject();
    let restored = adapter.owner.insert(button());
    adapter.owner.add_child(root, restored);
    adapter
        .resolve(target, ActionCall::Click)
        .expect("the adapter and the tree both hold the child")
        .invoke();

    adapter.owner.flush();
    adapter
        .resolve(target, ActionCall::Click)
        .expect("still held after the next flush")
        .invoke();
}

/// A range showing text whose only value handler changed kind in an update
/// the adapter rejected: the delivered handler decided what a text write
/// meant, so it reaches neither handler until the change is delivered, in
/// either direction. The handler kind is not in the payload, so the update
/// also renames the range for the adapter to have something to reject.
fn a_text_write_follows_the_delivered_value_handler() {
    use flui_semantics::{ActionCall, NumericRange, SemanticsAction, SemanticsRole};
    for delivered_numeric in [false, true] {
        let received = Arc::new(Mutex::new(Vec::<ActionCall>::new()));
        let range = |numeric: bool| {
            let mut node = node(1, if numeric { "Level" } else { "Volume" });
            node.config_mut().set_role(SemanticsRole::Slider);
            node.config_mut().set_numeric_range(
                NumericRange::try_new(0.0, 0.0, 10.0, 1.0).expect("finite range"),
            );
            node.config_mut().set_value("zero");
            let sink = Arc::clone(&received);
            node.config_mut().add_action(
                if numeric {
                    SemanticsAction::SetNumericValue
                } else {
                    SemanticsAction::SetValue
                },
                Rc::new(move |call| sink.lock().push(call)),
            );
            node
        };
        let delivered = range(delivered_numeric);
        let target = delivered
            .accessibility_id()
            .expect("render-backed identity");
        let mut adapter = RejectingAdapter::new();
        let id = adapter.root(delivered);

        adapter
            .owner
            .tree_mut()
            .replace_node(id, range(!delivered_numeric));
        adapter.reject();
        assert_eq!(
            adapter
                .resolve(target, ActionCall::SetValue("5".into()))
                .err(),
            Some(unsupported(target, SemanticsAction::SetValue)),
            "the text was meant for the delivered handler (numeric: {delivered_numeric})"
        );

        adapter.deliver();
        adapter
            .resolve(target, ActionCall::SetValue("5".into()))
            .expect("the delivered handler serves the text")
            .invoke();
        let expected = if delivered_numeric {
            ActionCall::SetValue("5".into())
        } else {
            ActionCall::SetNumericValue(5.0)
        };
        assert_eq!(*received.lock(), [expected]);
    }
}

/// A replacement callback that rejects its first update installs nothing: the
/// previous adapter still holds the last delivered tree, and requests are
/// checked against it.
fn a_rejected_callback_replacement_keeps_the_delivered_tree() {
    use flui_semantics::{ActionCall, SemanticsAction};
    let button = |menu: bool| {
        let mut button = node(1, "Send");
        button
            .config_mut()
            .add_action(SemanticsAction::Click, Rc::new(|_| {}));
        if menu {
            button
                .config_mut()
                .add_action(SemanticsAction::ShowContextMenu, Rc::new(|_| {}));
        }
        button
    };
    let delivered = button(false);
    let target = delivered
        .accessibility_id()
        .expect("render-backed identity");
    let mut adapter = RejectingAdapter::new();
    let id = adapter.root(delivered);

    adapter.owner.tree_mut().replace_node(id, button(true));
    let replacement = catch_unwind(AssertUnwindSafe(|| {
        adapter.owner.set_callback(Arc::new(|_: &TreeUpdate| {
            panic!("the replacement adapter rejected the update");
        }));
    }));
    assert!(replacement.is_err(), "the replacement rejects its update");
    assert_eq!(
        adapter.resolve(target, ActionCall::ShowContextMenu).err(),
        Some(unsupported(target, SemanticsAction::ShowContextMenu)),
        "the installed adapter was never offered a context menu"
    );

    adapter.deliver();
    adapter
        .resolve(target, ActionCall::ShowContextMenu)
        .expect("the installed adapter now offers a context menu")
        .invoke();
}

/// After an update the adapter rejected, a request is admitted only when the
/// current node serves it and the node as last delivered offered it: an
/// action only the delivered node offered and one only the rejected node
/// offers are both refused, and the next accepted delivery admits what it
/// carried.
#[test]
fn a_failed_publication_admits_only_what_both_nodes_offer() {
    let cases: &[(&str, fn())] = &[
        (
            "disclosure_transition",
            a_disclosure_admits_the_transition_both_states_allow,
        ),
        (
            "role_change",
            a_role_change_admits_only_the_actions_both_nodes_offer,
        ),
        (
            "numeric_range",
            a_numeric_write_is_admitted_inside_both_ranges,
        ),
        (
            "text_to_number",
            a_text_write_is_read_as_a_number_only_where_a_range_was_delivered,
        ),
        (
            "undelivered_node",
            a_node_the_adapter_was_not_given_is_not_found,
        ),
        (
            "reveal_visibility",
            a_reveal_is_admitted_only_while_both_nodes_are_visible,
        ),
        (
            "rejected_removal",
            a_removal_the_adapter_rejected_leaves_the_child_it_holds,
        ),
        (
            "delivered_value_handler",
            a_text_write_follows_the_delivered_value_handler,
        ),
        (
            "rejected_callback_replacement",
            a_rejected_callback_replacement_keeps_the_delivered_tree,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, run) in cases {
        if let Err(payload) = catch_unwind(run) {
            failures.push(name);
            std::mem::forget(payload);
        }
    }
    assert!(
        failures.is_empty(),
        "failed-publication admission rows failed: {failures:?}"
    );
}

/// A disabled control refuses a platform request the way it refuses an agent
/// one; focus and blur still reach it, and enabling it again restores the
/// click.
#[test]
fn a_disabled_node_takes_no_user_action_from_the_platform() {
    use flui_semantics::{
        ActionCall, SemanticsAction, SemanticsActionError, SemanticsActionRequest,
    };

    let (mut owner, _) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let mut button = node(1, "button");
    let target = button.accessibility_id().expect("render-backed identity");
    let clicks = Arc::new(Mutex::new(0_u32));
    let focuses = Arc::new(Mutex::new(0_u32));
    let (click_counter, focus_counter) = (Arc::clone(&clicks), Arc::clone(&focuses));
    button.config_mut().set_enabled(Some(false));
    button.config_mut().add_action(
        SemanticsAction::Click,
        Rc::new(move |_| *click_counter.lock() += 1),
    );
    button.config_mut().add_action(
        SemanticsAction::Focus,
        Rc::new(move |_| *focus_counter.lock() += 1),
    );
    let button = owner.insert(button);
    owner.add_child(root, button);
    owner.set_root(Some(root));
    owner.flush();

    assert_eq!(
        owner
            .resolve_action(SemanticsActionRequest::new(target, ActionCall::Click))
            .err(),
        Some(SemanticsActionError::Disabled { node_id: target })
    );
    owner
        .resolve_action(SemanticsActionRequest::new(target, ActionCall::Focus))
        .expect("a disabled node still takes focus")
        .invoke();
    assert_eq!((*clicks.lock(), *focuses.lock()), (0, 1));

    owner
        .get_mut(button)
        .expect("the button is live")
        .config_mut()
        .set_enabled(Some(true));
    owner.flush();
    owner
        .resolve_action(SemanticsActionRequest::new(target, ActionCall::Click))
        .expect("an enabled node is actionable")
        .invoke();
    assert_eq!(*clicks.lock(), 1);
}

/// A child the root lists but whose own parent link was never set is still
/// published by the full walk, so an action on it resolves.
#[test]
fn a_child_listed_without_a_parent_link_still_resolves() {
    use flui_semantics::{ActionCall, SemanticsAction, SemanticsActionRequest};

    let (mut owner, _) = recording_owner();
    let root = owner.insert(node(0, "root"));
    let mut button = node(1, "button");
    let target = button.accessibility_id().expect("render-backed identity");
    button
        .config_mut()
        .add_action(SemanticsAction::Click, Rc::new(|_| {}));
    let button = owner.insert(button);
    owner
        .get_mut(root)
        .expect("the root is live")
        .add_child(button);
    owner.set_root(Some(root));
    owner.flush();

    owner
        .resolve_action(SemanticsActionRequest::new(target, ActionCall::Click))
        .expect("a listed child is published and actionable")
        .invoke();
}
