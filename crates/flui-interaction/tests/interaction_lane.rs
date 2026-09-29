//! Public-contract tests for the inert ADR-0027 owner-local interaction lane.

use flui_foundation::geometry::Offset;
use flui_interaction::routing::MouseRegionTarget;
use flui_interaction::{
    HitTestEntry, HitTestHandle, HitTestProbe, HitTestResult, InteractionDispatchError,
    InteractionDispatchHandle, InteractionLane, PointerTarget, RenderId, ResolvedRouteToken,
};
use static_assertions::{assert_impl_all, assert_not_impl_any};
use std::cell::RefCell;
use std::rc::Rc;

/// A transform-less hit entry addressing `target`, for resolver tests.
fn hit_entry(target: PointerTarget) -> HitTestEntry {
    HitTestEntry::new(RenderId::new(1)).pointer_target(target)
}

assert_not_impl_any!(InteractionLane: Send, Sync);
assert_impl_all!(InteractionDispatchHandle: Clone, Send, Sync);
assert_impl_all!(PointerTarget: Copy, Send, Sync);
assert_impl_all!(MouseRegionTarget: Copy, Send, Sync);
assert_impl_all!(ResolvedRouteToken: Copy, Send, Sync);

#[test]
fn lane_mints_a_send_safe_least_privilege_handle() {
    let lane = InteractionLane::try_new().expect("lane identity should be available");
    let handle = lane.dispatch_handle();
    assert_eq!(format!("{handle:?}"), "InteractionDispatchHandle { .. }");
}

#[test]
fn realm_recreation_rejects_every_old_capability() {
    let old_lane = InteractionLane::try_new().expect("old lane");
    let old_handle = old_lane.dispatch_handle();
    let (old_target, old_route) = old_lane.enter(|| {
        let target = old_handle.register_pointer(|_| {}).expect("old target");
        let route = old_handle
            .resolve_pointer_route(&[hit_entry(target)])
            .expect("old route")
            .token();
        (target, route)
    });
    drop(old_lane);

    assert_eq!(
        old_handle.register_pointer(|_| {}),
        Err(InteractionDispatchError::OwnerGone)
    );

    let replacement_lane = InteractionLane::try_new().expect("replacement lane");
    let replacement_handle = replacement_lane.dispatch_handle();
    replacement_lane.enter(|| {
        assert_eq!(
            replacement_handle.unregister_pointer(old_target),
            Err(InteractionDispatchError::WrongRealm)
        );
        assert_eq!(
            replacement_handle.release_route(old_route),
            Err(InteractionDispatchError::WrongRealm)
        );
    });
}

#[test]
fn concurrently_live_lanes_keep_targets_and_routes_isolated() {
    let lane_a = InteractionLane::try_new().expect("lane A");
    let lane_b = InteractionLane::try_new().expect("lane B");
    let handle_a = lane_a.dispatch_handle();
    let handle_b = lane_b.dispatch_handle();
    let (target_a, route_a) = lane_a.enter(|| {
        let target = handle_a.register_pointer(|_| {}).expect("A target");
        let route = handle_a
            .resolve_pointer_route(&[hit_entry(target)])
            .expect("A route")
            .token();
        (target, route)
    });

    lane_b.enter(|| {
        assert_eq!(
            handle_b.unregister_pointer(target_a),
            Err(InteractionDispatchError::WrongRealm)
        );
        assert_eq!(
            handle_b.release_route(route_a),
            Err(InteractionDispatchError::WrongRealm)
        );
    });

    lane_a.enter(|| {
        handle_a
            .release_route(route_a)
            .expect("A route remains addressable from A");
        handle_a
            .unregister_pointer(target_a)
            .expect("A target remains addressable from A");
    });
}

// ---------------------------------------------------------------------------
// Fresh hit test — the capability a drag needs to discover targets it has moved
// over, which a replayed pointer-down route can never see.
//
// The handle pairs a realm ticket with ONE presentation's probe: identity is
// realm-wide, the tree is not, and a realm may host several presentations each
// with its own render tree.
// ---------------------------------------------------------------------------

/// Records the positions it was asked about and answers with a fixed path.
struct RecordingProbe {
    asked: RefCell<Vec<Offset<f64>>>,
    answer: Vec<RenderId>,
}

impl HitTestProbe for RecordingProbe {
    fn probe(
        &self,
        position: Offset<f64>,
        result: &mut HitTestResult,
    ) -> Result<(), InteractionDispatchError> {
        self.asked.borrow_mut().push(position);
        for id in &self.answer {
            result.add(HitTestEntry::new(*id));
        }
        Ok(())
    }
}

fn at(x: f64, y: f64) -> Offset<f64> {
    Offset::new(x, y)
}

fn recording(answer: Vec<RenderId>) -> Rc<RecordingProbe> {
    Rc::new(RecordingProbe {
        asked: RefCell::new(Vec::new()),
        answer,
    })
}

#[test]
fn a_fresh_hit_test_asks_the_probe_at_the_position_given() {
    let lane = InteractionLane::try_new().expect("lane");
    let probe = recording(vec![RenderId::new(7), RenderId::new(9)]);
    let handle = HitTestHandle::new(lane.dispatch_handle(), probe.clone());

    let snapshot = lane
        .enter(|| handle.hit_test_at(at(120.0, 40.0)))
        .expect("realm active");

    assert_eq!(
        probe.asked.borrow().as_slice(),
        &[at(120.0, 40.0)],
        "the probe must be asked about the position passed in, not a cached one"
    );
    assert_eq!(snapshot.position(), at(120.0, 40.0));
    assert_eq!(
        snapshot
            .path()
            .iter()
            .map(|entry| entry.target)
            .collect::<Vec<_>>(),
        vec![RenderId::new(7), RenderId::new(9)],
        "the snapshot carries the probe's whole path, leaf-first"
    );
}

/// Two handles on ONE realm read two different trees.
///
/// A realm may host several presentations, each with its own `PipelineOwner`.
/// Holding the probe on the realm's lane instead — one per realm — would answer
/// every presentation with whichever tree was installed first, and would report
/// the tree gone once *that* presentation closed while others were still live.
#[test]
fn two_presentations_on_one_realm_read_their_own_trees() {
    let lane = InteractionLane::try_new().expect("lane");
    let (first, second) = (
        recording(vec![RenderId::new(11)]),
        recording(vec![RenderId::new(22)]),
    );
    let a = HitTestHandle::new(lane.dispatch_handle(), first.clone());
    let b = HitTestHandle::new(lane.dispatch_handle(), second.clone());

    let (from_a, from_b) =
        lane.enter(|| (a.hit_test_at(at(1.0, 1.0)), b.hit_test_at(at(2.0, 2.0))));

    assert_eq!(
        from_a.expect("a answers").path()[0].target,
        RenderId::new(11)
    );
    assert_eq!(
        from_b.expect("b answers").path()[0].target,
        RenderId::new(22),
        "the second presentation must read ITS tree, not the first's"
    );
    assert_eq!(first.asked.borrow().as_slice(), &[at(1.0, 1.0)]);
    assert_eq!(second.asked.borrow().as_slice(), &[at(2.0, 2.0)]);
}

/// A probe that cannot read the tree right now.
struct BusyProbe;

impl HitTestProbe for BusyProbe {
    fn probe(
        &self,
        _position: Offset<f64>,
        _result: &mut HitTestResult,
    ) -> Result<(), InteractionDispatchError> {
        Err(InteractionDispatchError::TreeBusy)
    }
}

#[test]
fn a_busy_tree_is_reported_rather_than_answered_as_empty() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = HitTestHandle::new(lane.dispatch_handle(), Rc::new(BusyProbe));

    // "Nothing is there" and "I could not look" must not collapse into the same
    // answer: a drag over a live target during a frame would otherwise read as a
    // drag over blank space, and leave every target it was over.
    assert_eq!(
        lane.enter(|| handle.hit_test_at(at(1.0, 1.0))).unwrap_err(),
        InteractionDispatchError::TreeBusy
    );
}
