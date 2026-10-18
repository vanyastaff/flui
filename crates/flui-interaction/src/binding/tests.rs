use crate::gesture::GestureContest;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::*;
use crate::events::{
    make_down_event, make_down_event_for_id, make_move_event, make_move_event_for_id, make_up_event,
};

#[derive(Debug, Default)]
struct CountingArenaMember {
    accepts: AtomicUsize,
    rejects: AtomicUsize,
}

impl crate::gesture::arena::GestureArenaMember for CountingArenaMember {
    fn accept_gesture(&self, _: GestureContest) {
        self.accepts.fetch_add(1, Ordering::Relaxed);
    }

    fn reject_gesture(&self, _: GestureContest) {
        self.rejects.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Default)]
struct PanickingAcceptArenaMember;

impl crate::gesture::arena::GestureArenaMember for PanickingAcceptArenaMember {
    fn accept_gesture(&self, _: GestureContest) {
        panic!("arena accept panic");
    }

    fn reject_gesture(&self, _: GestureContest) {}
}

fn set_resampling(binding: &GestureBinding, enabled: bool) {
    binding
        .set_resampling_enabled(enabled)
        .expect("tests configure resampling without active pointers");
}

// ========================================================================
// Resampler wiring tests
// ========================================================================

// Pointer-sequence routing matrix: route caching, pending moves, supersession,
/// resampling and cancel.
#[test]
fn pointer_sequence_matrix() {
    let cases: &[(&str, fn())] = &[
        (
            "contact_move_uses_down_route_and_fresh_mouse_tracking_route",
            contact_move_uses_down_route_and_fresh_mouse_tracking_route,
        ),
        (
            "terminal_dispatches_the_pending_move_before_up_on_the_detached_route",
            terminal_dispatches_the_pending_move_before_up_on_the_detached_route,
        ),
        (
            "superseding_down_abandons_old_arena_and_pending_move_before_hit_test",
            superseding_down_abandons_old_arena_and_pending_move_before_hit_test,
        ),
        (
            "resampling_never_crosses_a_reused_pointer_sequence",
            resampling_never_crosses_a_reused_pointer_sequence,
        ),
        (
            "cancel_active_pointers_delivers_cancel_and_demotes_following_moves_to_hover",
            cancel_active_pointers_delivers_cancel_and_demotes_following_moves_to_hover,
        ),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("matrix case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

fn contact_move_uses_down_route_and_fresh_mouse_tracking_route() {
    let device = crate::events::DeviceId::try_from(1_u64).expect("known mouse device");
    let from_device = |mut event: PointerEvent| {
        match &mut event {
            PointerEvent::Down(data) => data.pointer = data.pointer.with_device(device),
            PointerEvent::Move(data) => data.pointer = data.pointer.with_device(device),
            PointerEvent::Up(data) => data.pointer = data.pointer.with_device(device),
            _ => unreachable!("fixture contains only measured contact events"),
        }
        event
    };
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let gesture_moves = Rc::new(Cell::new(0));
    let mouse_enters = Rc::new(Cell::new(0));
    let mouse_hovers = Rc::new(Cell::new(0));
    let fresh_hit_tests = Rc::new(Cell::new(0));

    lane.enter(|| {
        let callback_gesture_moves = Rc::clone(&gesture_moves);
        let pointer_target = handle
            .register_pointer(move |dispatch| {
                let event = dispatch.local;
                if matches!(event, PointerEvent::Move(_)) {
                    callback_gesture_moves.set(callback_gesture_moves.get() + 1);
                }
            })
            .expect("register gesture target");
        let callback_mouse_enters = Rc::clone(&mouse_enters);
        let callback_mouse_hovers = Rc::clone(&mouse_hovers);
        let mouse_target = handle
            .register_mouse_region(MouseRegionCallbacks {
                on_enter: Some(Rc::new(move |_device, _position| {
                    callback_mouse_enters.set(callback_mouse_enters.get() + 1);
                })),
                on_hover: Some(Rc::new(move |_device, _position| {
                    callback_mouse_hovers.set(callback_mouse_hovers.get() + 1);
                })),
                on_exit: None,
            })
            .expect("register mouse target");

        let down_target_id = RenderId::new(1);
        let mut down_result = HitTestResult::new();
        down_result.add(HitTestEntry::new(down_target_id).pointer_target(pointer_target));
        let position = Offset::new(10.0, 10.0);
        binding.handle_pointer_event(
            &from_device(make_down_event(position, PointerKind::Mouse).expect("finite input")),
            |_| down_result,
        );

        let mouse_target_id = RenderId::new(2);
        let mut fresh_result = HitTestResult::new();
        fresh_result.add(
            HitTestEntry::new(mouse_target_id)
                .mouse_annotation(MouseTrackerAnnotation::new(mouse_target_id, mouse_target)),
        );
        let callback_fresh_hit_tests = Rc::clone(&fresh_hit_tests);
        binding.handle_pointer_event(
            &from_device(make_move_event(position, PointerKind::Mouse).expect("finite input")),
            move |_| {
                callback_fresh_hit_tests.set(callback_fresh_hit_tests.get() + 1);
                fresh_result
            },
        );

        assert_eq!(fresh_hit_tests.get(), 1);
        assert_eq!(mouse_enters.get(), 1);
        assert_eq!(
            mouse_hovers.get(),
            0,
            "a mouse drag updates enter/exit/cursor state but is not hover"
        );
        assert_eq!(gesture_moves.get(), 0);
        assert_eq!(binding.flush_pending_moves(), 1);
        assert_eq!(
            gesture_moves.get(),
            1,
            "contact delivery remains pinned to the target resolved at Down"
        );
        assert!(
            binding
                .mouse_tracker()
                .device_active_regions(device)
                .contains(&mouse_target_id)
        );

        binding.handle_pointer_event(
            &from_device(make_up_event(position, PointerKind::Mouse).expect("finite input")),
            |_| HitTestResult::new(),
        );
        handle
            .unregister_pointer(pointer_target)
            .expect("release pointer target");
        handle
            .unregister_mouse_region(mouse_target)
            .expect("release mouse target");
    });
}

fn terminal_dispatches_the_pending_move_before_up_on_the_detached_route() {
    use std::{cell::RefCell, rc::Rc};

    let binding = GestureBinding::new();
    let events = Rc::new(RefCell::new(Vec::new()));
    let routed_events = Rc::clone(&events);
    let handler: crate::pointer::PointerRouteHandler = Rc::new(move |event| {
        routed_events.borrow_mut().push(match event {
            PointerEvent::Down(_) => "down",
            PointerEvent::Move(_) => "move",
            PointerEvent::Up(_) => "up",
            _ => "other",
        });
    });
    binding.pointer_router().add_route(
        PointerId::new(core::num::NonZeroU64::MIN),
        Rc::clone(&handler),
    );

    let down = make_down_event(Offset::new(1.0, 2.0), PointerKind::Touch).expect("finite input");
    binding.handle_pointer_event(&down, |_| HitTestResult::new());
    let move_event =
        make_move_event(Offset::new(3.0, 4.0), PointerKind::Touch).expect("finite input");
    binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
    assert_eq!(events.borrow().as_slice(), ["down"]);

    let up = make_up_event(Offset::new(5.0, 6.0), PointerKind::Touch).expect("finite input");
    binding.handle_pointer_event(&up, |_| HitTestResult::new());

    assert_eq!(events.borrow().as_slice(), ["down", "move", "up"]);
    binding
        .pointer_router()
        .remove_route(PointerId::new(core::num::NonZeroU64::MIN), &handler);
}

fn superseding_down_abandons_old_arena_and_pending_move_before_hit_test() {
    use std::cell::Cell;

    let binding = GestureBinding::new();
    let old_member = Rc::new(CountingArenaMember::default());
    let down = make_down_event(Offset::new(1.0, 2.0), PointerKind::Touch).expect("finite input");
    binding.handle_pointer_event(&down, |_| {
        binding
            .arena()
            .add(PointerId::new(core::num::NonZeroU64::MIN), &old_member);
        HitTestResult::new()
    });
    let move_event =
        make_move_event(Offset::new(3.0, 4.0), PointerKind::Touch).expect("finite input");
    binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
    assert_eq!(binding.pending_move_count(), 1);

    let old_sequence_gone_before_hit_test = Cell::new(false);
    binding.handle_pointer_event(&down, |_| {
        old_sequence_gone_before_hit_test.set(
            old_member.rejects.load(Ordering::Relaxed) == 1
                && binding.pending_move_count() == 0
                && binding.active_resampler_count() == 0
                && !binding
                    .arena()
                    .has_active(PointerId::new(core::num::NonZeroU64::MIN)),
        );
        HitTestResult::new()
    });

    assert!(old_sequence_gone_before_hit_test.get());
    assert_eq!(old_member.accepts.load(Ordering::Relaxed), 0);
    assert_eq!(binding.pending_move_count(), 0);
}

// Reentrancy matrix: input arriving from inside callbacks and destructors.
#[test]
fn reentrancy_matrix() {
    let cases: &[(&str, fn())] = &[
        (
            "same_pointer_reentry_is_blocked_during_pending_move_and_terminal_callbacks",
            same_pointer_reentry_is_blocked_during_pending_move_and_terminal_callbacks,
        ),
        (
            "cancel_all_rejects_reentrant_input_from_route_destructors",
            cancel_all_rejects_reentrant_input_from_route_destructors,
        ),
        (
            "reentrant_target_unregister_defers_owner_drop_until_terminal_cleanup",
            reentrant_target_unregister_defers_owner_drop_until_terminal_cleanup,
        ),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("matrix case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

fn same_pointer_reentry_is_blocked_during_pending_move_and_terminal_callbacks() {
    use std::{cell::Cell, rc::Rc};

    let binding = Rc::new(GestureBinding::new());
    let reentrant_hit_tests = Rc::new(Cell::new(0));
    let detached_observations = Rc::new(Cell::new(0));
    let binding_from_route = Rc::clone(&binding);
    let reentrant_calls = Rc::clone(&reentrant_hit_tests);
    let observations = Rc::clone(&detached_observations);
    let handler: crate::pointer::PointerRouteHandler = Rc::new(move |event| {
        if matches!(event, PointerEvent::Move(_) | PointerEvent::Up(_)) {
            if binding_from_route.active_pointer_count() == 0
                && binding_from_route.pending_move_count() == 0
                && binding_from_route.active_resampler_count() == 0
                && !binding_from_route
                    .arena()
                    .has_active(PointerId::new(core::num::NonZeroU64::MIN))
            {
                observations.set(observations.get() + 1);
            }
            let down =
                make_down_event(Offset::new(9.0, 9.0), PointerKind::Touch).expect("finite input");
            let reentrant_calls = Rc::clone(&reentrant_calls);
            binding_from_route.handle_pointer_event(&down, move |_| {
                reentrant_calls.set(reentrant_calls.get() + 1);
                HitTestResult::new()
            });
        }
    });
    binding.pointer_router().add_route(
        PointerId::new(core::num::NonZeroU64::MIN),
        Rc::clone(&handler),
    );

    let down = make_down_event(Offset::new(1.0, 2.0), PointerKind::Touch).expect("finite input");
    binding.handle_pointer_event(&down, |_| HitTestResult::new());
    let move_event =
        make_move_event(Offset::new(3.0, 4.0), PointerKind::Touch).expect("finite input");
    binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
    let up = make_up_event(Offset::new(5.0, 6.0), PointerKind::Touch).expect("finite input");
    binding.handle_pointer_event(&up, |_| HitTestResult::new());

    assert_eq!(detached_observations.get(), 2);
    assert_eq!(reentrant_hit_tests.get(), 0);
    assert_eq!(binding.active_pointer_count(), 0);
    binding
        .pointer_router()
        .remove_route(PointerId::new(core::num::NonZeroU64::MIN), &handler);
}

fn resampling_never_crosses_a_reused_pointer_sequence() {
    use std::{cell::RefCell, rc::Rc};

    let binding = Rc::new(GestureBinding::new());
    set_resampling(&binding, true);
    binding.set_sampling_clock(SamplingClock::Fixed {
        period: Duration::from_millis(8),
    });

    let moves = Rc::new(RefCell::new(Vec::new()));
    let replaced_sequence = Rc::new(Cell::new(false));
    let callback_binding = Rc::clone(&binding);
    let callback_moves = Rc::clone(&moves);
    let callback_replaced_sequence = Rc::clone(&replaced_sequence);
    let handler: crate::pointer::PointerRouteHandler = Rc::new(move |event| {
        let PointerEvent::Move(pointer_move) = event else {
            return;
        };

        callback_moves
            .borrow_mut()
            .push(pointer_move.current().position.get().x);
        if callback_replaced_sequence.replace(true) {
            return;
        }

        let up = make_up_event(Offset::new(10.0, 10.0), PointerKind::Touch).expect("finite input");
        callback_binding.handle_pointer_event(&up, |_| HitTestResult::new());

        let down =
            make_down_event(Offset::new(90.0, 90.0), PointerKind::Touch).expect("finite input");
        callback_binding.handle_pointer_event(&down, |_| HitTestResult::new());
        let next_move =
            make_move_event(Offset::new(99.0, 99.0), PointerKind::Touch).expect("finite input");
        callback_binding.handle_pointer_event(&next_move, |_| HitTestResult::new());
    });
    binding.pointer_router().add_route(
        PointerId::new(core::num::NonZeroU64::MIN),
        Rc::clone(&handler),
    );

    let down = make_down_event(Offset::new(0.0, 0.0), PointerKind::Touch).expect("finite input");
    binding.handle_pointer_event(&down, |_| HitTestResult::new());
    for coordinate in [10.0, 20.0] {
        let pointer_move = make_move_event(Offset::new(coordinate, coordinate), PointerKind::Touch)
            .expect("finite input");
        binding.handle_pointer_event(&pointer_move, |_| HitTestResult::new());
    }

    assert_eq!(binding.flush_pending_moves(), 1);
    assert_eq!(
        moves.borrow().as_slice(),
        [10.0],
        "samples owned by the detached contact must not reach its replacement"
    );
    assert_eq!(
        binding.pending_move_count(),
        1,
        "a Move queued by the replacement contact belongs to the next frame"
    );

    binding
        .pointer_router()
        .remove_route(PointerId::new(core::num::NonZeroU64::MIN), &handler);
}

// ========================================================================
// Owner-routed route lifecycle (ADR-0027 Task 3)
// ========================================================================

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use crate::pointer::{
    HitTestEntry, InteractionLane, MouseRegionCallbacks, MouseTrackerAnnotation, PointerTarget,
    RenderId,
};

fn hit_result(target: PointerTarget) -> HitTestResult {
    let mut result = HitTestResult::new();
    result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
    result
}

/// Sets its cell when dropped, so a test can observe the moment the
/// owner-local handler (and its captures) is released.
struct SetOnDrop(Rc<Cell<bool>>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

struct PanicOnDrop;

impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        panic!("route cleanup panic");
    }
}

struct ReenterDownOnDrop {
    binding: Rc<GestureBinding>,
    pointer: PointerId,
}

impl Drop for ReenterDownOnDrop {
    fn drop(&mut self) {
        let down =
            make_down_event_for_id(self.pointer, Offset::new(12.0, 12.0), PointerKind::Touch)
                .expect("finite input");
        self.binding
            .handle_pointer_event(&down, |_| HitTestResult::new());
    }
}

/// Losing OS focus mid-contact must terminate the sequence like a real
/// platform Cancel: the route cached at Down observes the Cancel, the
/// arena rejects its members, and a following Move — the pointer may
/// keep moving while the window is defocused — is a hover, not a drag
/// update on the dead sequence's route.
fn cancel_active_pointers_delivers_cancel_and_demotes_following_moves_to_hover() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = Rc::new(GestureBinding::new());
    let log = Rc::new(RefCell::new(Vec::new()));
    let arena_member = Rc::new(CountingArenaMember::default());

    lane.enter(|| {
        let sink = Rc::clone(&log);
        let target = handle
            .register_pointer(move |dispatch| {
                let event = dispatch.local;
                sink.borrow_mut().push(match event {
                    PointerEvent::Down(_) => "down",
                    PointerEvent::Move(_) => "move",
                    PointerEvent::Cancel(_) => "cancel",
                    _ => "other",
                });
            })
            .expect("register");
        let result = hit_result(target);

        let down =
            make_down_event(Offset::new(5.0, 5.0), PointerKind::Touch).expect("finite input");
        let binding_for_down = Rc::clone(&binding);
        let member = arena_member.clone();
        binding.handle_pointer_event(&down, move |_| {
            binding_for_down
                .arena()
                .add(PointerId::new(core::num::NonZeroU64::MIN), &member);
            result
        });
        assert_eq!(&*log.borrow(), &["down"]);
        assert!(binding.has_hit_test(PointerId::new(core::num::NonZeroU64::MIN)));

        binding.cancel_active_pointers();

        assert_eq!(
            &*log.borrow(),
            &["down", "cancel"],
            "the cached route must observe a synthesized terminal Cancel"
        );
        assert_eq!(
            arena_member.rejects.load(Ordering::Relaxed),
            1,
            "the arena must reject its members, never silently keep them"
        );
        assert!(!binding.has_hit_test(PointerId::new(core::num::NonZeroU64::MIN)));
        assert!(binding.arena().is_empty());

        // The pointer keeps moving after defocus: with the sequence
        // gone this is a hover (fresh hit test, ephemeral dispatch) —
        // the dead sequence's route must not receive it as a drag.
        let mv = make_move_event(Offset::new(9.0, 9.0), PointerKind::Touch).expect("finite input");
        binding.handle_pointer_event(&mv, |_| HitTestResult::new());
        binding.flush_pending_moves();
        assert_eq!(
            &*log.borrow(),
            &["down", "cancel"],
            "a post-cancel Move must not be delivered on the cancelled route"
        );

        // Idempotent: nothing left to cancel.
        binding.cancel_active_pointers();
        assert_eq!(&*log.borrow(), &["down", "cancel"]);
    });
}

// Panic containment matrix: each row is one failure point (alone, or in
/// competition with a later one) and must leave the binding able to run the next
/// operation.
#[test]
fn panic_containment_matrix() {
    let cases: &[(&str, fn())] = &[
        (
            "arena_accept_panic_cleans_up_handle_pointer_event",
            arena_accept_panic_cleans_up_handle_pointer_event,
        ),
        (
            "per_target_panic_still_delivers_later_targets_and_cleans_up_the_sequence",
            per_target_panic_still_delivers_later_targets_and_cleans_up_the_sequence,
        ),
        (
            "target_panic_wins_over_a_later_route_cleanup_panic",
            target_panic_wins_over_a_later_route_cleanup_panic,
        ),
        (
            "cancel_all_pointer_sequences_finishes_after_the_first_cleanup_panic",
            cancel_all_pointer_sequences_finishes_after_the_first_cleanup_panic,
        ),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("matrix case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

fn per_target_panic_still_delivers_later_targets_and_cleans_up_the_sequence() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let later_deliveries = Rc::new(Cell::new(0));
    let handler_dropped = Rc::new(Cell::new(false));
    lane.enter(|| {
        let drop_probe = SetOnDrop(Rc::clone(&handler_dropped));
        let panicking = handle
            .register_pointer(move |dispatch| {
                let event = dispatch.local;
                let _keep_probe_alive = &drop_probe;
                if matches!(event, PointerEvent::Up(_)) {
                    panic!("target panic on Up");
                }
            })
            .expect("register panicking");
        let count = Rc::clone(&later_deliveries);
        let later = handle
            .register_pointer(move |_| count.set(count.get() + 1))
            .expect("register later");

        let mut result = HitTestResult::new();
        result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(panicking));
        result.add(HitTestEntry::new(RenderId::new(2)).pointer_target(later));

        let down =
            make_down_event(Offset::new(5.0, 5.0), PointerKind::Mouse).expect("finite input");
        binding.handle_pointer_event(&down, |_| result.clone());
        assert_eq!(later_deliveries.get(), 1);
        handle
            .unregister_pointer(panicking)
            .expect("route owns the panicking cell now");

        let up = make_up_event(Offset::new(5.0, 5.0), PointerKind::Mouse).expect("finite input");
        let unwind = catch_unwind(AssertUnwindSafe(|| {
            binding.handle_pointer_event(&up, |_| HitTestResult::new());
        }));
        assert!(unwind.is_err(), "the first target panic must propagate");

        // Later targets still received the Up, and the mandatory cleanup
        // (cache removal + route release) ran before the resumed unwind.
        assert_eq!(later_deliveries.get(), 2);
        assert!(!binding.has_hit_test(PointerId::new(core::num::NonZeroU64::MIN)));
        assert!(
            handler_dropped.get(),
            "Up must sweep and release the route before resuming the panic"
        );
    });
}

fn target_panic_wins_over_a_later_route_cleanup_panic() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();

    lane.enter(|| {
        let panic_on_cleanup = PanicOnDrop;
        let target = handle
            .register_pointer(move |dispatch| {
                let event = dispatch.local;
                let _keep_cleanup_panic_alive = &panic_on_cleanup;
                if matches!(event, PointerEvent::Up(_)) {
                    panic!("target first panic");
                }
            })
            .expect("register target");
        let result = hit_result(target);
        let down =
            make_down_event(Offset::new(5.0, 5.0), PointerKind::Mouse).expect("finite input");
        binding.handle_pointer_event(&down, |_| result);
        handle
            .unregister_pointer(target)
            .expect("cached route owns the target");

        let up = make_up_event(Offset::new(5.0, 5.0), PointerKind::Mouse).expect("finite input");
        let unwind = catch_unwind(AssertUnwindSafe(|| {
            binding.handle_pointer_event(&up, |_| HitTestResult::new());
        }));
        let payload = unwind.expect_err("the transaction must resume its first panic");

        assert_eq!(payload.downcast_ref::<&str>(), Some(&"target first panic"));
        assert_eq!(binding.active_pointer_count(), 0);
        assert_eq!(binding.active_resampler_count(), 0);
        assert!(binding.arena().is_empty());
    });
}

#[derive(Clone, Copy)]
enum BindingEntryPoint {
    HitTestClosure,
}

impl BindingEntryPoint {
    fn dispatch(self, binding: &GestureBinding, event: &PointerEvent, result: &HitTestResult) {
        match self {
            Self::HitTestClosure => {
                binding.handle_pointer_event(event, |_| result.clone());
            }
        }
    }
}

fn cancel_all_pointer_sequences_finishes_after_the_first_cleanup_panic() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let first_pointer = PointerId::try_from(21).expect("nonzero pointer id");
    let later_pointer = PointerId::try_from(22).expect("nonzero pointer id");
    let later_handler_dropped = Rc::new(Cell::new(false));

    lane.enter(|| {
        let first_owner = PanicOnDrop;
        let first_target = handle
            .register_pointer(move |_| {
                let _keep_owner_alive = &first_owner;
            })
            .expect("register first target");
        let later_owner = SetOnDrop(Rc::clone(&later_handler_dropped));
        let later_target = handle
            .register_pointer(move |_| {
                let _keep_owner_alive = &later_owner;
            })
            .expect("register later target");

        let mut members = Vec::new();
        for pointer in [first_pointer, later_pointer] {
            for _ in 0..2 {
                let member = Rc::new(CountingArenaMember::default());
                binding.arena().add(pointer, &member);
                members.push(member);
            }
        }

        let first_down =
            make_down_event_for_id(first_pointer, Offset::new(1.0, 1.0), PointerKind::Touch)
                .expect("finite input");
        binding.handle_pointer_event(&first_down, |_| hit_result(first_target));
        let later_down =
            make_down_event_for_id(later_pointer, Offset::new(2.0, 2.0), PointerKind::Touch)
                .expect("finite input");
        binding.handle_pointer_event(&later_down, |_| hit_result(later_target));

        let first_token = binding
            .contacts
            .token(first_pointer)
            .flatten()
            .expect("first cached route token");
        let later_token = binding
            .contacts
            .token(later_pointer)
            .flatten()
            .expect("later cached route token");

        handle
            .unregister_pointer(first_target)
            .expect("first route owns target");
        handle
            .unregister_pointer(later_target)
            .expect("later route owns target");
        for (pointer, coordinate) in [(first_pointer, 5.0), (later_pointer, 6.0)] {
            let move_event = make_move_event_for_id(
                pointer,
                Offset::new(coordinate, coordinate),
                PointerKind::Touch,
            )
            .expect("finite input");
            binding.handle_pointer_event(&move_event, |_| HitTestResult::new());
        }

        let unwind = catch_unwind(AssertUnwindSafe(|| {
            binding.cancel_all_pointer_sequences();
        }));

        // Keep the RED failure safe: the old implementation loses the
        // tokens without releasing their lane routes. Explicitly release
        // them before asserting so PanicOnDrop cannot double-panic during
        // test teardown.
        if unwind.is_ok() {
            let _ = catch_unwind(AssertUnwindSafe(|| handle.release_route(first_token)));
            let _ = handle.release_route(later_token);
        }

        let payload = unwind.expect_err("the first cleanup panic must propagate");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"route cleanup panic"));
        assert!(
            later_handler_dropped.get(),
            "cleanup after the first panic must still release later handler owners"
        );
        assert_eq!(binding.active_pointer_count(), 0);
        assert_eq!(binding.active_resampler_count(), 0);
        assert_eq!(binding.pending_move_count(), 0);
        assert!(binding.arena().is_empty());
        drop(members);
    });
}

fn cancel_all_rejects_reentrant_input_from_route_destructors() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = Rc::new(GestureBinding::new());
    let reentrant_pointer = PointerId::try_from(31).expect("nonzero pointer id");

    lane.enter(|| {
        let owner = ReenterDownOnDrop {
            binding: Rc::clone(&binding),
            pointer: reentrant_pointer,
        };
        let target = handle
            .register_pointer(move |_| {
                let _keep_owner_alive = &owner;
            })
            .expect("register target");
        let down =
            make_down_event(Offset::new(2.0, 2.0), PointerKind::Touch).expect("finite input");
        binding.handle_pointer_event(&down, |_| hit_result(target));
        handle
            .unregister_pointer(target)
            .expect("cached route owns target");

        binding.cancel_all_pointer_sequences();

        assert_eq!(binding.active_pointer_count(), 0);
        assert_eq!(binding.active_resampler_count(), 0);
        assert_eq!(binding.pending_move_count(), 0);
        assert!(binding.arena().is_empty());
        assert!(!binding.has_hit_test(reentrant_pointer));
    });
}

fn reentrant_target_unregister_defers_owner_drop_until_terminal_cleanup() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let target_slot = Rc::new(Cell::new(None));
    let did_unregister = Rc::new(Cell::new(false));
    let later_deliveries = Rc::new(Cell::new(0));

    lane.enter(|| {
        let handle_for_target = handle.clone();
        let slot_for_target = Rc::clone(&target_slot);
        let did_unregister_in_target = Rc::clone(&did_unregister);
        let panic_on_terminal_drop = PanicOnDrop;
        let unregistering_target = handle
            .register_pointer(move |_| {
                let _keep_owner_alive = &panic_on_terminal_drop;
                if !did_unregister_in_target.replace(true) {
                    handle_for_target
                        .unregister_pointer(
                            slot_for_target
                                .get()
                                .expect("target installed before dispatch"),
                        )
                        .expect("unregister target reentrantly");
                }
            })
            .expect("register unregistering target");
        target_slot.set(Some(unregistering_target));

        let later_count = Rc::clone(&later_deliveries);
        let later_target = handle
            .register_pointer(move |_| later_count.set(later_count.get() + 1))
            .expect("register later target");
        let mut result = HitTestResult::new();
        result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(unregistering_target));
        result.add(HitTestEntry::new(RenderId::new(2)).pointer_target(later_target));
        let members = [
            Rc::new(CountingArenaMember::default()),
            Rc::new(CountingArenaMember::default()),
        ];
        for member in &members {
            binding
                .arena()
                .add(PointerId::new(core::num::NonZeroU64::MIN), member);
        }

        let down =
            make_down_event(Offset::new(6.0, 6.0), PointerKind::Touch).expect("finite input");
        binding.handle_pointer_event(&down, |_| result);
        assert!(did_unregister.get());
        assert_eq!(later_deliveries.get(), 1);
        assert!(
            !binding
                .arena()
                .is_open(PointerId::new(core::num::NonZeroU64::MIN))
        );

        handle
            .unregister_pointer(later_target)
            .expect("unregister later target");
        let up = make_up_event(Offset::new(6.0, 6.0), PointerKind::Touch).expect("finite input");
        let unwind = catch_unwind(AssertUnwindSafe(|| {
            binding.handle_pointer_event(&up, |_| HitTestResult::new());
        }));
        let payload = unwind.expect_err("terminal route cleanup must propagate Drop panic");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"route cleanup panic"));
        assert_eq!(later_deliveries.get(), 2);
        assert_eq!(binding.active_pointer_count(), 0);
        assert_eq!(binding.active_resampler_count(), 0);
        assert!(binding.arena().is_empty());
    });
}

fn assert_arena_accept_panic_cleanup(entry_point: BindingEntryPoint) {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let handler_dropped = Rc::new(Cell::new(false));

    lane.enter(|| {
        let probe = SetOnDrop(Rc::clone(&handler_dropped));
        let arena = binding.arena().clone();
        let accepting_member = Rc::new(PanickingAcceptArenaMember);
        let competing_member = Rc::new(CountingArenaMember::default());
        let target = handle
            .register_pointer(move |dispatch| {
                let event = dispatch.local;
                let _keep_probe_alive = &probe;
                if matches!(event, PointerEvent::Down(_)) {
                    arena.add(
                        PointerId::new(core::num::NonZeroU64::MIN),
                        &accepting_member,
                    );
                    arena.add(
                        PointerId::new(core::num::NonZeroU64::MIN),
                        &competing_member,
                    );
                }
            })
            .expect("register hit target");
        let result = hit_result(target);

        let down =
            make_down_event(Offset::new(5.0, 5.0), PointerKind::Touch).expect("finite input");
        entry_point.dispatch(&binding, &down, &result);
        handle
            .unregister_pointer(target)
            .expect("cached route owns the target");
        assert!(!handler_dropped.get());

        let up = make_up_event(Offset::new(5.0, 5.0), PointerKind::Touch).expect("finite input");
        let unwind = catch_unwind(AssertUnwindSafe(|| {
            entry_point.dispatch(&binding, &up, &HitTestResult::new());
        }));
        let payload = unwind.expect_err("arena accept panic must propagate");

        assert_eq!(payload.downcast_ref::<&str>(), Some(&"arena accept panic"));
        assert_eq!(binding.active_pointer_count(), 0);
        assert_eq!(binding.active_resampler_count(), 0);
        assert!(
            binding.arena().is_empty(),
            "the arena slot must be removed before invoking its members"
        );
        assert!(
            handler_dropped.get(),
            "the cached route must be released before unwind resumes"
        );
    });
}

fn arena_accept_panic_cleans_up_handle_pointer_event() {
    assert_arena_accept_panic_cleanup(BindingEntryPoint::HitTestClosure);
}

/// The mint is private: only a seam can start it at its terminal value.
#[test]
fn an_exhausted_surface_mint_refuses_permanently() {
    let binding = GestureBinding::new();
    binding.surfaces.reserve_through(u64::MAX - 1);
    let last = binding
        .embed_surface()
        .expect("the last identity is minted");
    assert_eq!(
        last,
        InputSurface::Embedded(EmbeddedSurfaceId::new(
            NonZeroU64::new(u64::MAX).expect("nonzero")
        ))
    );
    assert_eq!(binding.embed_surface(), Err(SurfaceIdsExhausted));
    assert_eq!(
        binding.embed_surface(),
        Err(SurfaceIdsExhausted),
        "refusal is permanent, never a wrapped reissue"
    );
}
