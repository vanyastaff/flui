//! Public-contract tests for the inert ADR-0027 owner-local interaction lane.

use flui_interaction::routing::MouseRegionTarget;
use flui_interaction::{
    HitTestEntry, HitTestProbe, HitTestResult, InteractionDispatchError, InteractionDispatchHandle,
    InteractionLane, PointerTarget, RenderId, ResolvedRouteToken, RouteResolutionMiss,
};
use flui_types::{Offset, Pixels};
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
fn public_error_surface_is_typed_and_identifier_free() {
    let variants = [
        InteractionDispatchError::IdentifierExhausted,
        InteractionDispatchError::WrongThread,
        InteractionDispatchError::InactiveRealm,
        InteractionDispatchError::WrongRealm,
        InteractionDispatchError::OwnerGone,
        InteractionDispatchError::TargetGone,
        InteractionDispatchError::StaleRoute,
        InteractionDispatchError::NoHitTestProbe,
        InteractionDispatchError::TreeBusy,
    ];
    assert_eq!(variants.len(), 9);

    let miss = RouteResolutionMiss::TargetGone { path_index: 3 };
    assert_eq!(miss.path_index(), 3);
}

#[test]
fn lane_mints_a_send_safe_least_privilege_handle() {
    let lane = InteractionLane::try_new().expect("lane identity should be available");
    let handle = lane.dispatch_handle();
    assert_eq!(format!("{handle:?}"), "InteractionDispatchHandle { .. }");
}

#[test]
fn activation_errors_have_stable_precedence() {
    let lane_a = InteractionLane::try_new().expect("lane A");
    let lane_b = InteractionLane::try_new().expect("lane B");
    let handle_a = lane_a.dispatch_handle();
    let handle_b = lane_b.dispatch_handle();

    let foreign_target = lane_b.enter(|| handle_b.register_pointer(|_| {}).expect("B target"));
    let stale_route = lane_a.enter(|| {
        let route = handle_a
            .resolve_pointer_route(&[])
            .expect("A route")
            .token();
        handle_a.release_route(route).expect("make A route stale");
        route
    });

    assert_eq!(
        handle_a.unregister_pointer(foreign_target),
        Err(InteractionDispatchError::InactiveRealm)
    );
    assert_eq!(
        handle_a.release_route(stale_route),
        Err(InteractionDispatchError::InactiveRealm)
    );

    lane_b.enter(|| {
        assert_eq!(
            handle_a.release_route(stale_route),
            Err(InteractionDispatchError::WrongRealm)
        );
    });
    lane_a.enter(|| assert!(handle_a.register_pointer(|_| {}).is_ok()));

    let dead_lane = InteractionLane::try_new().expect("dead lane");
    let dead_handle = dead_lane.dispatch_handle();
    drop(dead_lane);
    assert_eq!(
        dead_handle.register_pointer(|_| {}),
        Err(InteractionDispatchError::OwnerGone)
    );
    lane_b.enter(|| {
        assert_eq!(
            dead_handle.register_pointer(|_| {}),
            Err(InteractionDispatchError::OwnerGone)
        );
    });

    let threaded_dead = dead_handle.clone();
    let wrong_thread = std::thread::spawn(move || threaded_dead.register_pointer(|_| {}))
        .join()
        .expect("worker must not panic");
    assert_eq!(wrong_thread, Err(InteractionDispatchError::WrongThread));
}

#[test]
fn partial_resolution_preserves_live_order_and_reports_ordered_misses() {
    use flui_interaction::Offset;
    use flui_interaction::events::{PointerType, make_down_event};

    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let calls = Rc::new(RefCell::new(Vec::new()));

    lane.enter(|| {
        let first_calls = Rc::clone(&calls);
        let first = handle
            .register_pointer(move |_| first_calls.borrow_mut().push(1))
            .expect("first target");
        let gone = handle
            .register_pointer(|_| {})
            .expect("eventually-gone target");
        let last_calls = Rc::clone(&calls);
        let last = handle
            .register_pointer(move |_| last_calls.borrow_mut().push(3))
            .expect("last target");
        handle
            .unregister_pointer(gone)
            .expect("remove middle target");

        let resolution = handle
            .resolve_pointer_route(&[hit_entry(first), hit_entry(gone), hit_entry(last)])
            .expect("partial resolution succeeds");
        assert_eq!(
            resolution.misses(),
            &[RouteResolutionMiss::TargetGone { path_index: 1 }]
        );

        handle
            .unregister_pointer(first)
            .expect("cached route retains first cell");
        handle
            .unregister_pointer(last)
            .expect("cached route retains last cell");

        let event = make_down_event(Offset::ZERO, PointerType::Touch);
        handle
            .invoke_pointer_route(resolution.token(), &event)
            .expect("resolved route remains live");
        assert_eq!(&*calls.borrow(), &[1, 3]);

        let all_missing = handle
            .resolve_pointer_route(&[hit_entry(first), hit_entry(gone), hit_entry(last)])
            .expect("an all-missing path still resolves to an empty route");
        assert_eq!(
            all_missing.misses(),
            &[
                RouteResolutionMiss::TargetGone { path_index: 0 },
                RouteResolutionMiss::TargetGone { path_index: 1 },
                RouteResolutionMiss::TargetGone { path_index: 2 },
            ]
        );
        handle
            .invoke_pointer_route(all_missing.token(), &event)
            .expect("empty resolved route is a no-op");
        assert_eq!(&*calls.borrow(), &[1, 3]);
    });
}

#[test]
fn target_and_route_identity_never_alias_within_one_lane() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    lane.enter(|| {
        let retired_target = handle.register_pointer(|_| {}).expect("retired target");
        handle
            .unregister_pointer(retired_target)
            .expect("retire target");
        let replacement_target = handle.register_pointer(|_| {}).expect("replacement target");
        assert_ne!(retired_target, replacement_target);
        assert_eq!(
            handle.replace_pointer(retired_target, |_| {}),
            Err(InteractionDispatchError::TargetGone)
        );

        let retired_route = handle.resolve_pointer_route(&[]).expect("route").token();
        handle.release_route(retired_route).expect("retire route");
        let replacement_route = handle
            .resolve_pointer_route(&[])
            .expect("new route has a distinct identity")
            .token();
        assert_ne!(retired_route, replacement_route);
        assert_eq!(
            handle.release_route(retired_route),
            Err(InteractionDispatchError::StaleRoute)
        );
    });
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

#[test]
fn cached_route_reports_owner_gone_after_lane_drop_not_stale_route() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let route = lane.enter(|| {
        handle
            .resolve_pointer_route(&[])
            .expect("empty route is valid")
            .token()
    });

    drop(lane);

    assert_eq!(
        handle.release_route(route),
        Err(InteractionDispatchError::OwnerGone)
    );
    assert_ne!(
        handle.release_route(route),
        Err(InteractionDispatchError::StaleRoute)
    );
}

// ---------------------------------------------------------------------------
// Fresh hit test — the capability a drag needs to discover targets it has
// moved over, which a replayed pointer-down route can never see.
// ---------------------------------------------------------------------------

/// Records the positions it was asked about and answers with a fixed path.
struct RecordingProbe {
    asked: RefCell<Vec<Offset<Pixels>>>,
    answer: Vec<RenderId>,
}

impl HitTestProbe for RecordingProbe {
    fn probe(
        &self,
        position: Offset<Pixels>,
        result: &mut HitTestResult,
    ) -> Result<(), InteractionDispatchError> {
        self.asked.borrow_mut().push(position);
        for id in &self.answer {
            result.add(HitTestEntry::new(*id));
        }
        Ok(())
    }
}

fn at(x: f32, y: f32) -> Offset<Pixels> {
    Offset::new(Pixels(x), Pixels(y))
}

#[test]
fn a_fresh_hit_test_asks_the_probe_at_the_position_given() {
    let lane = InteractionLane::try_new().expect("lane");
    let probe = Rc::new(RecordingProbe {
        asked: RefCell::new(Vec::new()),
        answer: vec![RenderId::new(7), RenderId::new(9)],
    });
    lane.set_hit_test_probe(probe.clone());
    let handle = lane.dispatch_handle();

    let snapshot = lane
        .enter(|| handle.hit_test_at(at(120.0, 40.0)))
        .expect("probe installed and realm active");

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

#[test]
fn each_call_re_probes_rather_than_replaying_the_first_answer() {
    let lane = InteractionLane::try_new().expect("lane");
    let probe = Rc::new(RecordingProbe {
        asked: RefCell::new(Vec::new()),
        answer: vec![RenderId::new(1)],
    });
    lane.set_hit_test_probe(probe.clone());
    let handle = lane.dispatch_handle();

    // The whole point of the capability: a drag moving across three positions
    // must produce three questions. Caching the first answer is exactly the
    // pointer-down-route behaviour this exists to escape.
    lane.enter(|| {
        for x in [0.0, 10.0, 20.0] {
            handle.hit_test_at(at(x, 0.0)).expect("probe installed");
        }
    });

    assert_eq!(
        probe.asked.borrow().as_slice(),
        &[at(0.0, 0.0), at(10.0, 0.0), at(20.0, 0.0)]
    );
}

#[test]
fn a_lane_with_no_probe_says_so_instead_of_answering_emptily() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();

    // An empty snapshot and "nobody can answer" are different facts, and a
    // caller that cannot tell them apart silently treats an unwired realm as a
    // drag over blank space.
    assert_eq!(
        lane.enter(|| handle.hit_test_at(at(0.0, 0.0))).unwrap_err(),
        InteractionDispatchError::NoHitTestProbe
    );
}

#[test]
fn a_fresh_hit_test_is_refused_outside_its_own_realm() {
    let lane = InteractionLane::try_new().expect("lane");
    lane.set_hit_test_probe(Rc::new(RecordingProbe {
        asked: RefCell::new(Vec::new()),
        answer: vec![RenderId::new(1)],
    }));
    let handle = lane.dispatch_handle();

    assert_eq!(
        handle.hit_test_at(at(0.0, 0.0)).unwrap_err(),
        InteractionDispatchError::InactiveRealm,
        "no realm entered"
    );

    let other = InteractionLane::try_new().expect("second lane");
    assert_eq!(
        other
            .enter(|| handle.hit_test_at(at(0.0, 0.0)))
            .unwrap_err(),
        InteractionDispatchError::WrongRealm,
        "a handle minted by one realm must not read another's tree"
    );
}

#[test]
fn a_fresh_hit_test_is_refused_from_another_thread() {
    let lane = InteractionLane::try_new().expect("lane");
    lane.set_hit_test_probe(Rc::new(RecordingProbe {
        asked: RefCell::new(Vec::new()),
        answer: vec![RenderId::new(1)],
    }));
    let handle = lane.dispatch_handle();

    let elsewhere = std::thread::spawn(move || handle.hit_test_at(at(0.0, 0.0)).unwrap_err())
        .join()
        .expect("thread");

    assert_eq!(elsewhere, InteractionDispatchError::WrongThread);
}

#[test]
fn a_snapshot_outlives_the_lane_that_made_it_without_addressing_it() {
    let snapshot = {
        let lane = InteractionLane::try_new().expect("lane");
        lane.set_hit_test_probe(Rc::new(RecordingProbe {
            asked: RefCell::new(Vec::new()),
            answer: vec![RenderId::new(3)],
        }));
        let handle = lane.dispatch_handle();
        lane.enter(|| handle.hit_test_at(at(5.0, 5.0))).expect("ok")
    };

    // Owned, so this compiles and reads fine after the realm is gone. That is
    // the documented contract: a stale snapshot is useless, not unsound — it
    // names render objects that may have moved or been dropped.
    assert_eq!(snapshot.path().len(), 1);
    assert_eq!(snapshot.position(), at(5.0, 5.0));
}

/// A probe that cannot read the tree right now.
struct BusyProbe;

impl HitTestProbe for BusyProbe {
    fn probe(
        &self,
        _position: Offset<Pixels>,
        _result: &mut HitTestResult,
    ) -> Result<(), InteractionDispatchError> {
        Err(InteractionDispatchError::TreeBusy)
    }
}

#[test]
fn a_busy_tree_is_reported_rather_than_answered_as_empty() {
    let lane = InteractionLane::try_new().expect("lane");
    lane.set_hit_test_probe(Rc::new(BusyProbe));
    let handle = lane.dispatch_handle();

    // "Nothing is there" and "I could not look" must not collapse into the
    // same answer: a drag over a live target during a frame would otherwise
    // read as a drag over blank space, and leave every target it was over.
    assert_eq!(
        lane.enter(|| handle.hit_test_at(at(1.0, 1.0))).unwrap_err(),
        InteractionDispatchError::TreeBusy
    );
}
