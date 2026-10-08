//! Public-contract tests for the inert ADR-0027 owner-local interaction lane.

use flui_interaction::routing::MouseRegionTarget;
use flui_interaction::{
    HitTestEntry, InteractionDispatchError, InteractionDispatchHandle, InteractionLane,
    PointerTarget, RenderId, ResolvedRouteToken,
};
use static_assertions::{assert_impl_all, assert_not_impl_any};

/// A transform-less hit entry addressing `target`, for resolver tests.
fn hit_entry(target: PointerTarget) -> HitTestEntry {
    HitTestEntry::new(RenderId::new(1)).pointer_target(target)
}

#[test]
fn explicit_pointer_capture_contract() {
    let rows: &[(&str, fn())] = &[
        ("implicit_route", capture_keeps_the_full_implicit_down_route),
        (
            "exclusive_route",
            capture_selects_one_target_after_the_down_round,
        ),
        (
            "first_claim",
            capture_cannot_be_stolen_by_a_later_down_target,
        ),
        (
            "drop_loss",
            capture_drop_defers_exactly_one_loss_to_owner_entry,
        ),
        (
            "release_loss",
            capture_release_defers_exactly_one_loss_to_owner_entry,
        ),
        (
            "native_terminal",
            capture_native_terminal_invalidates_the_token,
        ),
        (
            "reused_identity",
            capture_old_token_cannot_cancel_a_replacement_down,
        ),
        (
            "callback_release",
            capture_release_inside_motion_defers_loss_until_next_entry,
        ),
        (
            "loss_failure",
            capture_loss_callback_failure_still_retires_contact,
        ),
        (
            "competing_loss_failure",
            capture_loss_preserves_first_failure_and_next_contact,
        ),
        (
            "owner_close",
            capture_owner_close_invalidates_retained_token,
        ),
        (
            "device_removal",
            capture_device_removal_invalidates_retained_token,
        ),
        (
            "accepted_motion",
            capture_release_delivers_accepted_motion_before_loss,
        ),
        (
            "released_tail",
            capture_released_tail_waits_for_a_fresh_down,
        ),
        ("wake_failure", capture_wake_failure_preserves_release_debt),
        (
            "wake_during_unwind",
            capture_wake_during_unwind_preserves_the_earlier_failure,
        ),
        (
            "committed_frame_release",
            capture_release_keeps_another_pointers_committed_frame_motion,
        ),
        (
            "released_device_identity",
            capture_release_refuses_only_its_own_device_tail,
        ),
    ];
    for &(name, row) in rows {
        if let Err(payload) = std::panic::catch_unwind(row) {
            eprintln!("capture contract row `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CaptureCase {
    Implicit,
    Exclusive,
    FirstClaim,
    Drop,
    Release,
    NativeLoss,
    ReusedId,
    CallbackRelease,
    LossFailure,
    CompetingLossFailure,
    OwnerClose,
    DeviceRemoval,
    QueuedRelease,
    ReleasedTail,
}

fn capture_keeps_the_full_implicit_down_route() {
    assert_capture_route(CaptureCase::Implicit);
}
fn capture_selects_one_target_after_the_down_round() {
    assert_capture_route(CaptureCase::Exclusive);
}
fn capture_cannot_be_stolen_by_a_later_down_target() {
    assert_capture_route(CaptureCase::FirstClaim);
}
fn capture_drop_defers_exactly_one_loss_to_owner_entry() {
    assert_capture_route(CaptureCase::Drop);
}
fn capture_release_defers_exactly_one_loss_to_owner_entry() {
    assert_capture_route(CaptureCase::Release);
}
fn capture_native_terminal_invalidates_the_token() {
    assert_capture_route(CaptureCase::NativeLoss);
}
fn capture_old_token_cannot_cancel_a_replacement_down() {
    assert_capture_route(CaptureCase::ReusedId);
}
fn capture_release_inside_motion_defers_loss_until_next_entry() {
    assert_capture_route(CaptureCase::CallbackRelease);
}
fn capture_loss_callback_failure_still_retires_contact() {
    assert_capture_route(CaptureCase::LossFailure);
}
fn capture_loss_preserves_first_failure_and_next_contact() {
    assert_capture_route(CaptureCase::CompetingLossFailure);
}
fn capture_owner_close_invalidates_retained_token() {
    assert_capture_route(CaptureCase::OwnerClose);
}
fn capture_device_removal_invalidates_retained_token() {
    assert_capture_route(CaptureCase::DeviceRemoval);
}
fn capture_release_delivers_accepted_motion_before_loss() {
    assert_capture_route(CaptureCase::QueuedRelease);
}
fn capture_released_tail_waits_for_a_fresh_down() {
    assert_capture_route(CaptureCase::ReleasedTail);
}

fn capture_release_keeps_another_pointers_committed_frame_motion() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerEvent, PointerKind, make_down_event_for_id, make_move_event_for_id,
        make_up_event_for_id,
    };
    use flui_interaction::{GestureBinding, HitTestResult, PointerCapture, PointerId};
    use std::{cell::RefCell, rc::Rc};

    let lane = InteractionLane::try_new().expect("lane");
    let binding = Rc::new(GestureBinding::new());
    let first = PointerId::try_from(11_u64).expect("first");
    let second = PointerId::try_from(12_u64).expect("second");
    let held = Rc::new(RefCell::new(std::collections::HashMap::<
        PointerId,
        PointerCapture,
    >::new()));
    let tokens = held.clone();
    let owner = Rc::downgrade(&binding);
    let log = Rc::new(RefCell::new(Vec::new()));
    let observed = log.clone();
    lane.enter(|| {
        let target = lane.dispatch_handle().register_pointer(move |dispatch| match dispatch.global {
            PointerEvent::Down(press) => {
                let capture = dispatch.capture().expect("real Down");
                tokens.borrow_mut().insert(press.pointer.id, capture);
            }
            PointerEvent::Move(motion) => {
                observed.borrow_mut().push((motion.pointer.id, "move", motion.current().position.get().x));
                if motion.pointer.id == first {
                    let token = tokens.borrow_mut().remove(&second).expect("other pointer capture");
                    drop(token);
                    let newer = make_move_event_for_id(second, Offset::new(300.0, 0.0), PointerKind::Touch).expect("post-release motion");
                    owner.upgrade().expect("live owner").handle_pointer_event(&newer, |_| panic!("released contact cannot become hover"));
                }
            }
            PointerEvent::Cancel(cancel) => {
                assert_eq!(cancel.reason, flui_platform_api::pointer::CancelReason::CaptureLost);
                observed.borrow_mut().push((cancel.pointer.id, "lost", 0.0));
            }
            _ => {}
        }).expect("target");
        for pointer in [first, second] {
            let down = make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Touch).expect("down");
            binding.handle_pointer_event(&down, |_| {
                let mut path = HitTestResult::new();
                path.add(hit_entry(target));
                path
            });
        }
        for (pointer, x) in [(first, 10.0), (second, 20.0)] {
            let movement = make_move_event_for_id(pointer, Offset::new(x, 0.0), PointerKind::Touch).expect("accepted motion");
            binding.handle_pointer_event(&movement, |_| panic!("contact retains route"));
        }
        binding.flush_pending_moves();
        assert_eq!(&*log.borrow(), &[(first, "move", 10.0), (second, "move", 20.0), (second, "lost", 0.0)], "committed frame motion survives release from an earlier callback; new motion is refused");
        binding.flush_pending_moves();
        assert_eq!(log.borrow().len(), 3, "no duplicate loss or post-release frame tail");
        let up = make_up_event_for_id(first, Offset::ZERO, PointerKind::Touch).expect("up");
        binding.handle_pointer_event(&up, |_| panic!("captured terminal"));
        let token = held.borrow_mut().remove(&first).expect("remaining token");
        drop(token);
        assert_eq!(binding.active_pointer_count(), 0);
    });
}

fn capture_release_refuses_only_its_own_device_tail() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        DeviceId, PointerEvent, PointerKind, make_down_event_for_id, make_move_event_for_id,
    };
    use flui_interaction::{GestureBinding, HitTestResult, PointerCapture, PointerId};
    use std::{cell::RefCell, rc::Rc};
    let lane = InteractionLane::try_new().expect("lane");
    let binding = GestureBinding::new();
    let id = PointerId::try_from(1_u64).expect("primary contact");
    let first = DeviceId::try_from(17_u64).expect("first device");
    let other = DeviceId::try_from(18_u64).expect("other device");
    let held = Rc::new(RefCell::new(None::<PointerCapture>));
    let store = held.clone();
    let devices = Rc::new(RefCell::new(Vec::new()));
    let observed = devices.clone();
    lane.enter(|| {
        let target = lane
            .dispatch_handle()
            .register_pointer(move |dispatch| match dispatch.global {
                PointerEvent::Down(_) => {
                    *store.borrow_mut() = Some(dispatch.capture().expect("real Down"))
                }
                PointerEvent::Move(motion) => observed.borrow_mut().push(motion.pointer.device),
                _ => {}
            })
            .expect("target");
        let path = || {
            let mut result = HitTestResult::new();
            result.add(hit_entry(target));
            result
        };
        let mut down = make_down_event_for_id(id, Offset::ZERO, PointerKind::Mouse).expect("down");
        let PointerEvent::Down(press) = &mut down else {
            unreachable!()
        };
        press.pointer = press.pointer.with_device(first);
        binding.handle_pointer_event(&down, |_| path());
        let token = held.borrow_mut().take().expect("capture");
        drop(token);
        binding.flush_pending_moves();
        for device in [first, other] {
            let mut movement =
                make_move_event_for_id(id, Offset::new(20.0, 20.0), PointerKind::Mouse)
                    .expect("motion");
            let PointerEvent::Move(motion) = &mut movement else {
                unreachable!()
            };
            motion.pointer = motion.pointer.with_device(device);
            if device == other {
                *motion = flui_platform_api::pointer::PointerMove::new(
                    motion.pointer,
                    flui_platform_api::pointer::PointerButtons::NONE,
                    *motion.current(),
                );
            }
            binding.handle_pointer_event(&movement, |_| {
                assert_eq!(device, other, "released device tail cannot hit-test");
                path()
            });
            binding.flush_pending_moves();
        }
        assert_eq!(
            &*devices.borrow(),
            &[Some(other)],
            "released contact identity cannot suppress another device's hover"
        );
    });
}

struct CaptureWakeWindow(std::sync::atomic::AtomicUsize);

impl flui_platform_api::PlatformWindow for CaptureWakeWindow {
    fn id(&self) -> flui_platform_api::WindowId {
        flui_platform_api::WindowId::new(21)
    }
    fn physical_size(&self) -> flui_foundation::geometry::Size<i32> {
        flui_foundation::geometry::Size::new(80, 80)
    }
    fn logical_size(&self) -> flui_foundation::geometry::Size<f64> {
        flui_foundation::geometry::Size::new(80.0, 80.0)
    }
    fn scale_factor(&self) -> f64 {
        1.0
    }
    fn request_redraw(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        panic!("capture wake failure");
    }
    fn is_focused(&self) -> bool {
        true
    }
    fn is_visible(&self) -> bool {
        true
    }
    fn set_cursor(
        &self,
        _: flui_platform_api::CursorIcon,
    ) -> Result<(), flui_platform_api::CursorError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn capture_wake_failure_preserves_release_debt() {
    assert_capture_wake_failure(false);
}
fn capture_wake_during_unwind_preserves_the_earlier_failure() {
    assert_capture_wake_failure(true);
}

fn assert_capture_wake_failure(active_unwind: bool) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{PointerEvent, PointerKind, make_down_event};
    use flui_interaction::{GestureBinding, HitTestResult, PointerCapture};
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    let lane = InteractionLane::try_new().expect("lane");
    let binding = GestureBinding::new();
    let window = Arc::new(CaptureWakeWindow(AtomicUsize::new(0)));
    let capability: Arc<dyn flui_platform_api::PlatformWindow> = window.clone();
    flui_interaction::__runtime::set_pointer_capture_wake(&binding, Arc::downgrade(&capability));
    let held = Rc::new(RefCell::new(None::<PointerCapture>));
    let store = held.clone();
    let lost = Rc::new(Cell::new(0));
    let callbacks = lost.clone();
    lane.enter(|| {
        let target = lane
            .dispatch_handle()
            .register_pointer(move |dispatch| match dispatch.global {
                PointerEvent::Down(_) => {
                    *store.borrow_mut() = Some(dispatch.capture().expect("real Down"))
                }
                PointerEvent::Cancel(cancel) => {
                    assert_eq!(
                        cancel.reason,
                        flui_platform_api::pointer::CancelReason::CaptureLost
                    );
                    callbacks.set(callbacks.get() + 1);
                }
                _ => {}
            })
            .expect("target");
        let down = make_down_event(Offset::new(5.0, 5.0), PointerKind::Touch).expect("down");
        binding.handle_pointer_event(&down, |_| {
            let mut path = HitTestResult::new();
            path.add(hit_entry(target));
            path
        });
        let token = held.borrow_mut().take().expect("retained token");
        let failed = catch_unwind(AssertUnwindSafe(|| {
            if active_unwind {
                let _capture = token;
                panic!("earlier capture failure");
            }
            drop(token);
        }))
        .expect_err("wake failure or earlier unwind propagates");
        assert_eq!(
            flui_foundation::panic::payload_text(&*failed),
            Some(if active_unwind {
                "earlier capture failure"
            } else {
                "capture wake failure"
            })
        );
        assert_eq!(
            window.0.load(Ordering::Relaxed),
            1,
            "release attempts the owning presentation wake once"
        );
        assert_eq!(
            lost.get(),
            0,
            "wake failure does not invoke event callbacks from token Drop"
        );
        binding.flush_pending_moves();
        binding.flush_pending_moves();
        assert_eq!(
            lost.get(),
            1,
            "committed loss remains deliverable after failed wake"
        );
        assert_eq!(binding.active_pointer_count(), 0);
        assert!(binding.arena().is_empty());
    });
}

fn assert_capture_route(case: CaptureCase) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerEvent, PointerKind, make_down_event, make_move_event, make_up_event,
    };
    use flui_interaction::{
        GestureBinding, HitTestResult, PointerCapture, PointerCaptureError, PointerDispatch,
    };
    use flui_platform_api::pointer::CancelReason;
    use std::{cell::RefCell, rc::Rc};

    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let tokens = Rc::new(RefCell::new(Vec::<PointerCapture>::new()));
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut down = make_down_event(Offset::new(5.0, 5.0), PointerKind::Touch).expect("down");
    let device = flui_platform_api::pointer::DeviceId::try_from(17_u64).expect("nonzero device");
    if case == CaptureCase::DeviceRemoval {
        let PointerEvent::Down(press) = &mut down else {
            unreachable!()
        };
        press.pointer = press.pointer.with_device(device);
    }
    assert!(
        matches!(
            PointerDispatch::at_root(&down).capture(),
            Err(PointerCaptureError::Unavailable)
        ),
        "synthetic dispatch has no capture authority"
    );
    let fails = matches!(
        case,
        CaptureCase::LossFailure | CaptureCase::CompetingLossFailure
    );
    lane.enter(|| {
        if case == CaptureCase::CompetingLossFailure {
            binding
                .pointer_router()
                .add_global_handler(Rc::new(|event| {
                    assert!(
                        !matches!(event, PointerEvent::Cancel(_)),
                        "second capture loss failure"
                    );
                }));
        }
        let held = tokens.clone();
        let first_log = log.clone();
        let first = handle
            .register_pointer(move |dispatch| {
                first_log.borrow_mut().push((
                    1,
                    match dispatch.global {
                        PointerEvent::Down(_) => "down",
                        PointerEvent::Move(_) => "move",
                        PointerEvent::Up(_) => "up",
                        PointerEvent::Cancel(cancel) => {
                            if case == CaptureCase::DeviceRemoval {
                                assert_eq!(cancel.reason, CancelReason::DeviceRemoved);
                                "removed"
                            } else {
                                assert_eq!(cancel.reason, CancelReason::CaptureLost);
                                "lost"
                            }
                        }
                        _ => "other",
                    },
                ));
                if fails && matches!(dispatch.global, PointerEvent::Cancel(_)) {
                    panic!("first capture loss failure");
                }
                if case != CaptureCase::Implicit && matches!(dispatch.global, PointerEvent::Down(_))
                {
                    let token = dispatch.capture().expect("real Down target can capture");
                    held.borrow_mut().push(token);
                } else if matches!(dispatch.global, PointerEvent::Move(_)) {
                    assert!(
                        matches!(dispatch.capture(), Err(PointerCaptureError::NotDown)),
                        "Move cannot mint capture authority"
                    );
                    if case == CaptureCase::CallbackRelease {
                        let token = held.borrow_mut().pop().expect("callback owns capture");
                        drop(token);
                        assert_eq!(
                            first_log.borrow().last(),
                            Some(&(1, "move")),
                            "release within delivery invokes no nested cancellation"
                        );
                    }
                }
            })
            .expect("first target");
        let later_log = log.clone();
        let second = handle
            .register_pointer(move |dispatch| {
                later_log.borrow_mut().push((
                    2,
                    match dispatch.global {
                        PointerEvent::Down(_) => "down",
                        PointerEvent::Move(_) => "move",
                        PointerEvent::Up(_) => "up",
                        PointerEvent::Cancel(_) => "lost",
                        _ => "other",
                    },
                ));
                if case == CaptureCase::FirstClaim
                    && matches!(dispatch.global, PointerEvent::Down(_))
                {
                    assert!(
                        matches!(dispatch.capture(), Err(PointerCaptureError::AlreadyClaimed)),
                        "first claimant keeps exclusive authority"
                    );
                }
            })
            .expect("second target");
        let path = || {
            let mut result = HitTestResult::new();
            result.add(hit_entry(first));
            result.add(hit_entry(second));
            result
        };
        binding.handle_pointer_event(&down, |_| path());
        assert_eq!(
            &*log.borrow(),
            &[(1, "down"), (2, "down")],
            "capture does not truncate the committed Down round"
        );
        if matches!(case, CaptureCase::OwnerClose | CaptureCase::DeviceRemoval) {
            if case == CaptureCase::OwnerClose {
                flui_interaction::__runtime::close_gestures(
                    &binding,
                    flui_interaction::__runtime::CloseMode::Ordinary,
                );
            } else {
                let removed = PointerEvent::DeviceRemoved(
                    flui_platform_api::pointer::PointerDeviceChange::new(
                        device,
                        PointerKind::Touch,
                        flui_platform_api::EventTime::from_nanos(1),
                    ),
                );
                binding
                    .handle_pointer_event(&removed, |_| panic!("device removal does not hit-test"));
            }
            let before_drop = log.borrow().clone();
            let token = tokens
                .borrow_mut()
                .pop()
                .expect("retained closed-owner token");
            drop(token);
            binding.flush_pending_moves();
            assert_eq!(
                &*log.borrow(),
                &before_drop,
                "closed owner/device invalidates token before callbacks"
            );
            assert_eq!(binding.active_pointer_count(), 0);
            assert!(binding.arena().is_empty());
            return;
        }
        if case == CaptureCase::ReusedId {
            binding.handle_pointer_event(&down, |_| path());
            let stale = tokens.borrow_mut().remove(0);
            drop(stale);
        }
        if matches!(
            case,
            CaptureCase::Drop
                | CaptureCase::Release
                | CaptureCase::QueuedRelease
                | CaptureCase::ReleasedTail
        ) || fails
        {
            if case == CaptureCase::QueuedRelease {
                let accepted = make_move_event(Offset::new(20.0, 20.0), PointerKind::Touch)
                    .expect("accepted move");
                binding
                    .handle_pointer_event(&accepted, |_| panic!("accepted contact retains route"));
            }
            let token = tokens.borrow_mut().pop().expect("retained token");
            if case == CaptureCase::Release {
                token.release();
            } else {
                drop(token);
            }
            assert_eq!(
                log.borrow().last(),
                Some(&(2, "down")),
                "release runs no event callback inline"
            );
            let drained = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                binding.flush_pending_moves()
            }));
            if fails {
                let payload = drained.expect_err("loss failure propagates after mandatory cleanup");
                assert_eq!(
                    flui_foundation::panic::payload_text(&*payload),
                    Some("first capture loss failure")
                );
            } else {
                drained.expect("healthy release");
            }
            assert_eq!(log.borrow().last(), Some(&(1, "lost")));
            if case == CaptureCase::QueuedRelease {
                assert!(
                    log.borrow().ends_with(&[(1, "move"), (1, "lost")]),
                    "accepted motion precedes loss on its frozen capture route"
                );
            }
            assert_eq!(binding.active_pointer_count(), 0);
            binding.flush_pending_moves();
            assert_eq!(
                log.borrow()
                    .iter()
                    .filter(|(_, event)| *event == "lost")
                    .count(),
                1
            );
            assert!(binding.arena().is_empty());
            if case == CaptureCase::ReleasedTail {
                let tail = make_move_event(Offset::new(30.0, 30.0), PointerKind::Touch)
                    .expect("released tail");
                binding.handle_pointer_event(&tail, |_| {
                    panic!("released contact tail cannot become hover")
                });
                binding.flush_pending_moves();
                assert_eq!(
                    log.borrow().last(),
                    Some(&(1, "lost")),
                    "unadmitted post-release motion is refused"
                );
            }
            if fails || case == CaptureCase::ReleasedTail {
                binding.handle_pointer_event(&down, |_| path());
                let up =
                    make_up_event(Offset::new(5.0, 5.0), PointerKind::Touch).expect("healthy up");
                binding.handle_pointer_event(&up, |_| panic!("healthy terminal retains route"));
                let token = tokens
                    .borrow_mut()
                    .pop()
                    .expect("replacement contact capture");
                drop(token);
                binding.flush_pending_moves();
                assert_eq!(
                    log.borrow().last(),
                    Some(&(1, "up")),
                    "next contact completes after containment"
                );
            }
            return;
        }
        let movement =
            make_move_event(Offset::new(200.0, 200.0), PointerKind::Touch).expect("outside move");
        binding.handle_pointer_event(&movement, |_| {
            panic!("touch contact retains its Down route")
        });
        binding.flush_pending_moves();
        let terminal = if case == CaptureCase::NativeLoss {
            let PointerEvent::Down(press) = &down else {
                unreachable!()
            };
            PointerEvent::Cancel(flui_platform_api::pointer::PointerCancel::new(
                press.pointer,
                flui_platform_api::EventTime::from_nanos(1),
                CancelReason::CaptureLost,
            ))
        } else {
            make_up_event(Offset::new(200.0, 200.0), PointerKind::Touch).expect("up")
        };
        binding.handle_pointer_event(&terminal, |_| panic!("terminal retains its Down route"));
        let loses = matches!(case, CaptureCase::NativeLoss | CaptureCase::CallbackRelease);
        let tail = if case == CaptureCase::Implicit {
            vec![(1, "move"), (2, "move"), (1, "up"), (2, "up")]
        } else if loses {
            vec![(1, "move"), (1, "lost")]
        } else {
            vec![(1, "move"), (1, "up")]
        };
        assert!(
            log.borrow().ends_with(&tail),
            "later delivery uses the claimed target, or the entire implicit route"
        );
        tokens.borrow_mut().clear();
        binding.flush_pending_moves();
        assert_eq!(
            log.borrow()
                .iter()
                .filter(|(_, event)| *event == "lost")
                .count(),
            usize::from(loses),
            "native terminal and stale tokens cannot add cancellation"
        );
        assert_eq!(binding.active_pointer_count(), 0);
    });
}

#[test]
fn binding_input_contract_matrix() {
    if let Ok(mode) = std::env::var("FLUI_HOVER_METADATA_WORKER") {
        assert_hover_path_retirement(mode != "metadata-only", 2);
        return;
    }
    let cases: &[(&str, fn())] = &[
        (
            "hover_healthy_metadata_retirement",
            hover_healthy_metadata_retirement,
        ),
        (
            "hover_callback_retains_metadata",
            hover_callback_failure_retains_metadata,
        ),
        (
            "hover_callback_first_before_metadata_drop",
            hover_callback_failure_stays_first_before_metadata_drop,
        ),
        (
            "hover_competing_metadata_drop",
            hover_competing_metadata_retirement_is_contained,
        ),
        (
            "hover_metadata_failure_alone",
            hover_metadata_retirement_failure_alone,
        ),
        (
            "hover_metadata_failures_compete_without_body_failure",
            hover_metadata_retirement_failures_compete_without_body_failure,
        ),
        (
            "non_finite_hover_move_is_refused",
            non_finite_hover_move_is_refused,
        ),
        (
            "non_finite_contact_move_is_refused",
            non_finite_contact_move_is_refused,
        ),
        (
            "non_finite_wheel_position_is_refused",
            non_finite_wheel_position_is_refused,
        ),
        (
            "non_finite_pinch_position_is_refused",
            non_finite_pinch_position_is_refused,
        ),
        (
            "non_finite_pinch_during_contact_is_refused",
            non_finite_pinch_during_contact_is_refused,
        ),
        (
            "frame_coalescing_preserves_hardware_history",
            frame_coalescing_preserves_hardware_history,
        ),
        (
            "frame_coalesced_history_reaches_drag_velocity",
            frame_coalesced_history_reaches_drag_velocity,
        ),
        (
            "frame_coalesced_history_reaches_multi_drag_velocity",
            frame_coalesced_history_reaches_multi_drag_velocity,
        ),
        (
            "frame_coalesced_history_reaches_scale_velocity",
            frame_coalesced_history_reaches_scale_velocity,
        ),
        (
            "frame_coalesced_history_reaches_tap_drag_velocity",
            frame_coalesced_history_reaches_tap_drag_velocity,
        ),
        (
            "device_removal_preserves_a_reentrant_replacement",
            device_removal_preserves_a_reentrant_replacement,
        ),
        (
            "device_removal_preserves_replacement_after_competing_failures",
            device_removal_preserves_replacement_after_competing_failures,
        ),
        (
            "focus_loss_preserves_a_reentrant_replacement",
            focus_loss_preserves_a_reentrant_replacement,
        ),
        (
            "focus_loss_preserves_replacement_after_competing_failures",
            focus_loss_preserves_replacement_after_competing_failures,
        ),
        (
            "checked_down_refuses_non_finite_position",
            checked_down_refuses_non_finite_position,
        ),
        (
            "capped_contact_never_becomes_hover",
            capped_contact_never_becomes_hover,
        ),
        (
            "refusal_saturation_preserves_admitted_contacts_and_resets_with_lifecycle",
            refusal_saturation_preserves_admitted_contacts_and_resets_with_lifecycle,
        ),
        (
            "wheel_listener_failure_keeps_claim_delivery",
            wheel_listener_failure_keeps_claim_delivery,
        ),
        (
            "scroll_selected_failure",
            scroll_selected_failure_keeps_lease,
        ),
        (
            "scroll_competing_failure",
            scroll_observer_failure_stays_first,
        ),
        (
            "scroll_retired_target",
            scroll_retired_target_does_not_chain,
        ),
        (
            "scroll_terminal_reentry",
            scroll_terminal_reentry_keeps_new_lease,
        ),
        ("scroll_claim_reentry", scroll_claim_reentry_keeps_new_lease),
        ("scroll_focus_loss", scroll_focus_loss_releases_lease),
        ("scroll_kind_metadata", scroll_kind_metadata_keeps_lease),
        ("scroll_role_metadata", scroll_role_metadata_keeps_lease),
        (
            "native_selected_failure",
            native_selected_failure_keeps_lease,
        ),
        (
            "native_competing_failure",
            native_observer_failure_stays_first,
        ),
        (
            "native_retired_target",
            native_retired_target_does_not_chain,
        ),
        (
            "native_terminal_reentry",
            native_terminal_reentry_keeps_new_lease,
        ),
        ("native_claim_reentry", native_claim_reentry_keeps_new_lease),
        ("native_focus_loss", native_focus_loss_releases_lease),
        (
            "native_claim_retirement",
            native_claim_survives_capture_retirement,
        ),
        (
            "native_competing_retirement",
            native_first_failure_survives_capture_retirement,
        ),
        (
            "native_repeated_start_owner",
            native_repeated_start_keeps_selected_owner,
        ),
        (
            "scroll_batch_reentry",
            scroll_owner_batch_preserves_reentrant_admission,
        ),
        (
            "scroll_device_batch_reentry",
            scroll_device_batch_preserves_reentrant_admission,
        ),
        (
            "native_focus_batch_reentry",
            native_focus_batch_preserves_reentrant_admission,
        ),
        (
            "native_device_batch_reentry",
            native_device_batch_preserves_reentrant_admission,
        ),
        (
            "scroll_first_terminal_delta",
            scroll_first_terminal_delta_remains_deliverable,
        ),
        (
            "scroll_claim_retirement",
            scroll_claim_survives_capture_retirement,
        ),
        (
            "scroll_competing_retirement",
            scroll_first_failure_survives_capture_retirement,
        ),
        (
            "wheel_first_failure_survives_claim_failure",
            wheel_first_failure_survives_claim_failure,
        ),
        (
            "pinch_listener_failure_keeps_claim_delivery",
            pinch_listener_failure_keeps_claim_delivery,
        ),
        (
            "pinch_first_failure_survives_claim_failure",
            pinch_first_failure_survives_claim_failure,
        ),
        (
            "captured_pinch_listener_failure_keeps_claim_delivery",
            captured_pinch_listener_failure_keeps_claim_delivery,
        ),
        (
            "captured_pinch_first_failure_survives_claim_failure",
            captured_pinch_first_failure_survives_claim_failure,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            failures.push(format!(
                "{name}: {}",
                flui_foundation::panic::payload_text(&*payload).unwrap_or("unknown failure")
            ));
        }
    }
    assert!(failures.is_empty(), "failed rows:\n{}", failures.join("\n"));
}

fn assert_hover_path_retirement(callback_fails: bool, failing_metadata: usize) {
    use flui_interaction::events::{
        PointerEvent, PointerKind, make_move_event, make_move_event_for_id,
    };
    use flui_interaction::{GestureBinding, HitTestResult, Offset, PointerId};
    use std::{
        any::Any,
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    struct MetadataCapture {
        drops: Arc<AtomicUsize>,
        panic_on_drop: bool,
    }
    impl Drop for MetadataCapture {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
            if self.panic_on_drop {
                panic!("later hover metadata retirement failure");
            }
        }
    }
    let lane = InteractionLane::try_new().expect("owner lane");
    let binding = GestureBinding::new();
    let drops = Arc::new(AtomicUsize::new(0));
    let entries = failing_metadata.max(1);
    let external: Rc<RefCell<Vec<Arc<dyn Any + Send + Sync>>>> = Rc::new(RefCell::new(
        (0..entries)
            .map(|index| {
                Arc::new(MetadataCapture {
                    drops: Arc::clone(&drops),
                    panic_on_drop: index < failing_metadata,
                }) as Arc<dyn Any + Send + Sync>
            })
            .collect(),
    ));
    let owner = Rc::clone(&external);
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let peer_calls = Rc::new(Cell::new(0));
    let peer = Rc::clone(&peer_calls);
    binding
        .pointer_router()
        .add_global_handler(Rc::new(move |event| {
            if let PointerEvent::Move(motion) = event {
                if motion.pointer.id == PointerId::try_from(2_u64).expect("peer identity") {
                    peer.set(peer.get() + 1);
                    return;
                }
                observed.set(observed.get() + 1);
                // The accepted hover path is now each payload's last owner. This
                // retirement is safe before failure because that path still owns it.
                let released = owner.borrow_mut().drain(..).collect::<Vec<_>>();
                drop(released);
                if callback_fails {
                    panic!("first hover callback failure");
                }
            }
        }));
    let movement =
        make_move_event(Offset::new(20.0, 20.0), PointerKind::Mouse).expect("finite hover");
    let PointerEvent::Move(motion) = movement else {
        unreachable!()
    };
    let movement = PointerEvent::Move(flui_platform_api::pointer::PointerMove::new(
        motion.pointer,
        flui_platform_api::pointer::PointerButtons::NONE,
        *motion.current(),
    ));
    lane.enter(|| {
        binding.handle_pointer_event(&movement, |_| {
            let mut path = HitTestResult::new();
            for (index, metadata) in external.borrow().iter().enumerate() {
                path.add(
                    HitTestEntry::new(RenderId::new(index + 1)).metadata(Arc::clone(metadata)),
                );
            }
            path
        });
        let peer_movement = make_move_event_for_id(
            PointerId::try_from(2_u64).expect("peer identity"),
            Offset::new(30.0, 30.0),
            PointerKind::Mouse,
        )
        .expect("finite peer hover");
        let PointerEvent::Move(peer_motion) = peer_movement else {
            unreachable!()
        };
        let peer_movement = PointerEvent::Move(flui_platform_api::pointer::PointerMove::new(
            peer_motion.pointer,
            flui_platform_api::pointer::PointerButtons::NONE,
            *peer_motion.current(),
        ));
        binding.handle_pointer_event(&peer_movement, |_| HitTestResult::new());
        assert_eq!(
            calls.get(),
            0,
            "hover delivery is queued until the owner frame"
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        let outcome = catch_unwind(AssertUnwindSafe(|| binding.flush_pending_moves()));
        if callback_fails {
            let first = outcome.expect_err("the public callback failed");
            assert_eq!(
                first.downcast_ref::<&str>().copied(),
                Some("first hover callback failure")
            );
            assert_eq!(
                drops.load(Ordering::SeqCst),
                0,
                "failed delivery retains last-owner opaque metadata before any destructor runs"
            );
        } else if failing_metadata != 0 {
            let first = outcome.expect_err("metadata retirement failed after healthy delivery");
            assert_eq!(
                first.downcast_ref::<&str>().copied(),
                Some("later hover metadata retirement failure")
            );
            assert_eq!(
                drops.load(Ordering::SeqCst),
                1,
                "the first failed metadata retirement retains later entries"
            );
        } else {
            assert_eq!(outcome.expect("healthy hover delivery"), 2);
            assert_eq!(
                drops.load(Ordering::SeqCst),
                1,
                "healthy last-owner metadata is destroyed"
            );
        }
        assert!(
            external.borrow().is_empty(),
            "callback released its independent metadata owners"
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(
            peer_calls.get(),
            1,
            "metadata retirement cannot discard the remaining accepted frame peer"
        );
        binding.pointer_router().clear();
        let healthy_calls = Rc::new(Cell::new(0));
        let healthy = Rc::clone(&healthy_calls);
        binding
            .pointer_router()
            .add_global_handler(Rc::new(move |_| healthy.set(healthy.get() + 1)));
        binding.handle_pointer_event(&movement, |_| HitTestResult::new());
        assert_eq!(
            binding.flush_pending_moves(),
            1,
            "next accepted hover remains deliverable"
        );
        assert_eq!(healthy_calls.get(), 1);
        assert_eq!(binding.flush_pending_moves(), 0);
        binding.pointer_router().clear();
    });
}

fn hover_healthy_metadata_retirement() {
    assert_hover_path_retirement(false, 0);
}
fn hover_callback_failure_retains_metadata() {
    assert_hover_path_retirement(true, 0);
}
fn hover_callback_failure_stays_first_before_metadata_drop() {
    assert_hover_path_retirement(true, 1);
}
fn hover_competing_metadata_retirement_is_contained() {
    assert_isolated_hover_metadata_retirement("competing");
}

fn hover_metadata_retirement_failure_alone() {
    assert_hover_path_retirement(false, 1);
}

fn hover_metadata_retirement_failures_compete_without_body_failure() {
    assert_isolated_hover_metadata_retirement("metadata-only");
}

fn assert_isolated_hover_metadata_retirement(mode: &str) {
    // Two ordinary metadata destructors can abort a default path Vec Drop.
    // Isolate that baseline failure; no panic payload has a hostile destructor.
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "interaction_lane::binding_input_contract_matrix",
            "--nocapture",
        ])
        .env("FLUI_HOVER_METADATA_WORKER", mode)
        .env("RUST_BACKTRACE", "0")
        .output()
        .expect("isolated hover metadata worker");
    assert!(
        output.status.success(),
        "competing hover metadata worker failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn device_removal_preserves_a_reentrant_replacement() {
    assert_lifecycle_snapshot_preserves_replacement(true, false);
}

fn device_removal_preserves_replacement_after_competing_failures() {
    assert_lifecycle_snapshot_preserves_replacement(true, true);
}

fn focus_loss_preserves_a_reentrant_replacement() {
    assert_lifecycle_snapshot_preserves_replacement(false, false);
}

fn focus_loss_preserves_replacement_after_competing_failures() {
    assert_lifecycle_snapshot_preserves_replacement(false, true);
}

fn assert_lifecycle_snapshot_preserves_replacement(device_removed: bool, competing: bool) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        DeviceId, PointerEvent, PointerKind, make_down_event_for_id, make_up_event_for_id,
    };
    use flui_interaction::{GestureBinding, HitTestResult, PointerId};
    use flui_platform_api::{EventTime, pointer::PointerDeviceChange};
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    fn contact_event(pointer: PointerId, device: DeviceId, up: bool, time: u64) -> PointerEvent {
        let mut event = if up {
            make_up_event_for_id(pointer, Offset::ZERO, PointerKind::Mouse)
        } else {
            make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Mouse)
        }
        .expect("finite measured contact");
        match &mut event {
            PointerEvent::Down(data) => {
                data.pointer = data.pointer.with_device(device);
                data.sample.time = EventTime::from_nanos(time);
            }
            PointerEvent::Up(data) => {
                data.pointer = data.pointer.with_device(device);
                data.sample.time = EventTime::from_nanos(time);
            }
            _ => unreachable!(),
        }
        event
    }

    let binding = Rc::new(GestureBinding::new());
    let first = PointerId::try_from(1_u64).expect("contact");
    let second = PointerId::try_from(2_u64).expect("contact");
    let third = PointerId::try_from(3_u64).expect("contact");
    let old_device = DeviceId::try_from(1_u64).expect("device");
    let new_device = DeviceId::try_from(2_u64).expect("device");
    let replaced = Rc::new(Cell::new(false));
    let replacement_cancels = Rc::new(Cell::new(0));
    let replacement_ups = Rc::new(Cell::new(0));
    let later_cancels = Rc::new(Cell::new(0));
    let owner = Rc::downgrade(&binding);
    let did_replace = Rc::clone(&replaced);
    let cancels = Rc::clone(&replacement_cancels);
    let ups = Rc::clone(&replacement_ups);
    let later = Rc::clone(&later_cancels);
    binding
        .pointer_router()
        .add_global_handler(Rc::new(move |event| match event {
            PointerEvent::Cancel(data)
                if data.pointer.id == first && !did_replace.replace(true) =>
            {
                let binding = owner.upgrade().expect("live binding");
                binding.handle_pointer_event(
                    &contact_event(second, new_device, false, 20_000_000),
                    |_| HitTestResult::new(),
                );
                if competing {
                    panic!("first lifecycle cancellation failure");
                }
            }
            PointerEvent::Cancel(data) if data.pointer.id == second && did_replace.get() => {
                cancels.set(cancels.get() + 1);
            }
            PointerEvent::Cancel(data) if data.pointer.id == third => {
                later.set(later.get() + 1);
                if competing {
                    panic!("later lifecycle cancellation second failure");
                }
            }
            PointerEvent::Up(data) if data.pointer.id == second && did_replace.get() => {
                ups.set(ups.get() + 1);
            }
            _ => {}
        }));
    binding.handle_pointer_event(&contact_event(first, old_device, false, 0), |_| {
        HitTestResult::new()
    });
    binding.handle_pointer_event(
        &contact_event(second, old_device, false, 10_000_000),
        |_| HitTestResult::new(),
    );
    binding.handle_pointer_event(&contact_event(third, old_device, false, 15_000_000), |_| {
        HitTestResult::new()
    });
    let result = catch_unwind(AssertUnwindSafe(|| {
        if device_removed {
            let event = PointerEvent::DeviceRemoved(PointerDeviceChange::new(
                old_device,
                PointerKind::Mouse,
                EventTime::from_nanos(30_000_000),
            ));
            binding.handle_pointer_event(&event, |_| panic!("device lifecycle must not hit-test"));
        } else {
            binding.cancel_active_pointers();
        }
    }));
    if competing {
        let payload = result.expect_err("first cancellation failure propagates after cleanup");
        assert_eq!(
            flui_foundation::panic::payload_text(&*payload),
            Some("first lifecycle cancellation failure")
        );
    } else {
        result.expect("healthy lifecycle cleanup");
    }
    assert!(
        replaced.get(),
        "first contact cancellation re-admitted the competing identity"
    );
    assert!(
        !binding.has_hit_test(first),
        "original first contact retired"
    );
    assert_eq!(
        later_cancels.get(),
        1,
        "later accepted cancellation remains deliverable after first failure"
    );
    assert!(
        !binding.has_hit_test(third),
        "later original contact retired"
    );
    assert_eq!(
        replacement_cancels.get(),
        0,
        "snapshot cancellation must not reach a newer admitted sequence"
    );
    assert!(
        binding.has_hit_test(second),
        "replacement retains its terminal delivery obligation"
    );
    binding.handle_pointer_event(&contact_event(second, new_device, true, 40_000_000), |_| {
        panic!("captured Up must not hit-test")
    });
    assert_eq!(
        replacement_ups.get(),
        1,
        "replacement delivers its own healthy Up after containment"
    );
    assert_eq!(binding.active_pointer_count(), 0);
    assert!(binding.arena().is_empty());
    binding.handle_pointer_event(&contact_event(first, new_device, false, 50_000_000), |_| {
        HitTestResult::new()
    });
    binding.handle_pointer_event(&contact_event(first, new_device, true, 60_000_000), |_| {
        HitTestResult::new()
    });
    assert_eq!(
        binding.active_pointer_count(),
        0,
        "later independent contact remains healthy"
    );
}

fn checked_down_refuses_non_finite_position() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{PointerKind, make_down_event_for_id, make_up_event_for_id};
    use flui_interaction::{GestureBinding, HitTestResult, PointerId};
    use std::{cell::Cell, rc::Rc};
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for position in [Offset::new(bad, 0.0), Offset::new(0.0, bad)] {
            let binding = GestureBinding::new();
            let hits = Cell::new(0);
            let deliveries = Rc::new(Cell::new(0));
            let log = Rc::clone(&deliveries);
            binding
                .pointer_router()
                .add_global_handler(Rc::new(move |_| log.set(log.get() + 1)));
            let pointer = PointerId::try_from(2).expect("nonzero pointer");
            assert!(
                make_down_event_for_id(pointer, position, PointerKind::Touch).is_err(),
                "checked vocabulary refuses invalid Down before binding admission"
            );
            assert_eq!(hits.get(), 0);
            assert_eq!(deliveries.get(), 0);
            let route = |_| {
                hits.set(hits.get() + 1);
                HitTestResult::new()
            };
            binding.handle_pointer_event(
                &make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
                    .expect("finite input"),
                route,
            );
            binding.handle_pointer_event(
                &make_up_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
                    .expect("finite input"),
                route,
            );
            assert_eq!(
                hits.get(),
                1,
                "healthy contact can be admitted after constructor refusal"
            );
            assert_eq!(
                deliveries.get(),
                2,
                "healthy contact and terminal event are delivered"
            );
        }
    }
}

#[derive(Clone, Copy)]
enum NonFiniteInput {
    HoverMove,
    ContactMove,
    Wheel,
    Pinch,
    PinchDuringContact,
}

fn assert_non_finite_motion_or_signal_is_refused(input: NonFiniteInput) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerKind, make_down_event_for_id, make_move_event_for_id as checked_move,
        make_pinch_gesture_event as checked_pinch, make_scroll_event as checked_scroll,
        make_up_event_for_id,
    };
    use flui_interaction::{GestureBinding, HitTestResult, PointerId};
    use std::{cell::Cell, rc::Rc};

    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for position in [Offset::new(bad, 0.0), Offset::new(0.0, bad)] {
            let binding = GestureBinding::new();
            let hits = Cell::new(0);
            let deliveries = Rc::new(Cell::new(0));
            let log = Rc::clone(&deliveries);
            binding
                .pointer_router()
                .add_global_handler(Rc::new(move |_| log.set(log.get() + 1)));
            // Native PanZoom uses its own virtual contact identity; it cannot
            // reuse the live touch contact that is kept captured here.
            let pointer = PointerId::new(core::num::NonZeroU64::MIN);
            let route = |_| {
                hits.set(hits.get() + 1);
                HitTestResult::new()
            };
            let captured = matches!(
                input,
                NonFiniteInput::ContactMove | NonFiniteInput::PinchDuringContact
            );
            if captured {
                binding.handle_pointer_event(
                    &make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
                        .expect("finite input"),
                    route,
                );
                assert_eq!(deliveries.get(), 1, "healthy contact was admitted");
            }
            hits.set(0);
            deliveries.set(0);
            let event = |position| match input {
                NonFiniteInput::HoverMove | NonFiniteInput::ContactMove => {
                    checked_move(pointer, position, PointerKind::Touch)
                }
                NonFiniteInput::Wheel => checked_scroll(position, Offset::new(0.0, 10.0)),
                NonFiniteInput::Pinch | NonFiniteInput::PinchDuringContact => {
                    checked_pinch(position, 0.1)
                }
            };
            assert!(
                event(position).is_err(),
                "checked position refuses invalid input before delivery"
            );
            binding.flush_pending_moves();
            assert_eq!(hits.get(), 0, "invalid position must not reach hit testing");
            assert_eq!(
                deliveries.get(),
                0,
                "invalid position must not reach pointer consumers"
            );

            binding
                .handle_pointer_event(&event(Offset::new(2.0, 0.0)).expect("finite input"), route);
            binding.flush_pending_moves();
            assert_eq!(deliveries.get(), 1, "healthy input follows refused input");
            if captured {
                let expected_hits =
                    usize::from(matches!(input, NonFiniteInput::PinchDuringContact));
                assert_eq!(
                    hits.get(),
                    expected_hits,
                    "native gestures hit-test independently while contact motion retains its route"
                );
                binding.handle_pointer_event(
                    &make_up_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
                        .expect("finite input"),
                    route,
                );
                assert_eq!(
                    deliveries.get(),
                    2,
                    "the admitted contact still delivers its terminal event"
                );
                assert_eq!(
                    hits.get(),
                    expected_hits,
                    "touch Up retains its captured route"
                );
            } else {
                assert_eq!(hits.get(), 1, "healthy input performs a fresh hit test");
            }
        }
    }
}

fn non_finite_hover_move_is_refused() {
    assert_non_finite_motion_or_signal_is_refused(NonFiniteInput::HoverMove);
}
fn non_finite_contact_move_is_refused() {
    assert_non_finite_motion_or_signal_is_refused(NonFiniteInput::ContactMove);
}
fn non_finite_wheel_position_is_refused() {
    assert_non_finite_motion_or_signal_is_refused(NonFiniteInput::Wheel);
}
fn non_finite_pinch_position_is_refused() {
    assert_non_finite_motion_or_signal_is_refused(NonFiniteInput::Pinch);
}
fn non_finite_pinch_during_contact_is_refused() {
    assert_non_finite_motion_or_signal_is_refused(NonFiniteInput::PinchDuringContact);
}

fn hardware_trace_event(
    mut event: flui_interaction::events::PointerEvent,
    nanos: u64,
) -> flui_interaction::events::PointerEvent {
    use flui_interaction::events::PointerEvent;
    match &mut event {
        PointerEvent::Down(data) => {
            data.sample.time = flui_platform_api::EventTime::from_nanos(nanos)
        }
        PointerEvent::Up(data) => {
            data.sample.time = flui_platform_api::EventTime::from_nanos(nanos)
        }
        PointerEvent::Move(data) => {
            let mut sample = *data.current();
            sample.time = flui_platform_api::EventTime::from_nanos(nanos);
            *data = flui_interaction::events::PointerMove::new(data.pointer, data.buttons, sample)
                .with_modifiers(data.modifiers)
                .with_coalesced(data.coalesced().to_vec())
                .with_predicted(data.predicted().to_vec());
        }
        _ => {}
    }
    event
}

fn curved_hardware_moves() -> Vec<flui_interaction::events::PointerEvent> {
    use flui_foundation::geometry::Offset;
    use flui_interaction::{
        PointerId,
        events::{PointerEvent, PointerKind, make_move_event_for_id},
    };
    // x = 20_000 t², in logical pixels and seconds. The hardware's last
    // velocity is 2_000 px/s; retaining only the endpoints loses the curvature.
    let mut moves: Vec<_> = [(20_u64, 8.0), (30, 18.0), (40, 32.0), (50, 50.0)]
        .into_iter()
        .map(|(millis, x)| {
            hardware_trace_event(
                make_move_event_for_id(
                    PointerId::new(core::num::NonZeroU64::MIN),
                    Offset::new(x, 0.0),
                    PointerKind::Touch,
                )
                .expect("finite input"),
                1_000_000_000 + millis * 1_000_000,
            )
        })
        .collect();
    let earlier = hardware_trace_event(
        make_move_event_for_id(
            PointerId::new(core::num::NonZeroU64::MIN),
            Offset::new(2.0, 0.0),
            PointerKind::Touch,
        )
        .expect("finite input"),
        1_010_000_000,
    );
    if let (PointerEvent::Move(first), PointerEvent::Move(earlier)) = (&mut moves[0], earlier) {
        *first = first.clone().with_coalesced(vec![*earlier.current()]);
    }
    moves
}

fn frame_coalescing_preserves_hardware_history() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::{
        GestureBinding, HitTestResult, PointerId,
        events::{PointerEvent, PointerKind, make_down_event_for_id, make_up_event_for_id},
    };
    use std::{cell::RefCell, rc::Rc};
    let binding = GestureBinding::new();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&observed);
    binding
        .pointer_router()
        .add_global_handler(Rc::new(move |event| {
            if let PointerEvent::Move(movement) = event {
                let history: Vec<_> = movement
                    .coalesced()
                    .iter()
                    .map(|sample| (sample.time.as_nanos(), sample.position.get().x))
                    .collect();
                log.borrow_mut().push((
                    movement.current().time.as_nanos(),
                    movement.current().position.get().x,
                    history,
                ));
            }
        }));
    for flush_each in [true, false, true] {
        observed.borrow_mut().clear();
        binding.handle_pointer_event(
            &hardware_trace_event(
                make_down_event_for_id(
                    PointerId::new(core::num::NonZeroU64::MIN),
                    Offset::ZERO,
                    PointerKind::Touch,
                )
                .expect("finite input"),
                1_000_000_000,
            ),
            |_| HitTestResult::new(),
        );
        for event in curved_hardware_moves() {
            binding.handle_pointer_event(&event, |_| HitTestResult::new());
            if flush_each {
                binding.flush_pending_moves();
            }
        }
        binding.flush_pending_moves();
        binding.handle_pointer_event(
            &hardware_trace_event(
                make_up_event_for_id(
                    PointerId::new(core::num::NonZeroU64::MIN),
                    Offset::new(50.0, 0.0),
                    PointerKind::Touch,
                )
                .expect("finite input"),
                1_050_000_000,
            ),
            |_| HitTestResult::new(),
        );
        let collected = observed.borrow();
        assert_eq!(
            collected.len(),
            if flush_each { 4 } else { 1 },
            "one dispatch per frame"
        );
        if flush_each {
            assert_eq!(
                collected[0].2,
                [(1_010_000_000, 2.0)],
                "backend history survives ordinary delivery"
            );
        } else {
            assert_eq!(collected[0].0, 1_050_000_000);
            assert_eq!(collected[0].1, 50.0);
            assert_eq!(
                collected[0].2,
                [
                    (1_010_000_000, 2.0),
                    (1_020_000_000, 8.0),
                    (1_030_000_000, 18.0),
                    (1_040_000_000, 32.0)
                ],
                "earlier hardware samples survive frame replacement in time order"
            );
        }
    }
}

#[derive(Clone, Copy)]
enum HardwareVelocityProducer {
    Drag,
    MultiDrag,
    Scale,
    TapDrag,
}

fn velocity_for_hardware_trace(producer: HardwareVelocityProducer, flush_each: bool) -> f64 {
    use flui_foundation::geometry::Offset;
    use flui_interaction::{
        DragAxis, DragGestureRecognizer, GestureBinding, GestureRecognizer, HitTestResult,
        ManualClock, MultiDragAxis, MultiDragEndDetails, MultiDragGestureRecognizer,
        MultiDragHandle, MultiDragUpdateDetails, PointerId, ScaleGestureRecognizer,
        TapAndDragGestureRecognizer,
        events::{PointerEvent, PointerKind, make_down_event_for_id, make_up_event_for_id},
        routing::PointerDispatch,
    };
    use std::{cell::RefCell, rc::Rc, sync::Arc};
    let binding = GestureBinding::with_clock(Arc::new(ManualClock::new()));
    let velocities = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&velocities);
    struct DragClient(Rc<RefCell<Vec<f64>>>);
    impl MultiDragHandle for DragClient {
        fn update(&self, _: MultiDragUpdateDetails) {}
        fn end(&self, details: MultiDragEndDetails) {
            self.0
                .borrow_mut()
                .push(details.velocity.pixels_per_second.dx);
        }
        fn cancel(&self) {}
    }
    let drag: Rc<dyn GestureRecognizer> = match producer {
        HardwareVelocityProducer::Drag => {
            DragGestureRecognizer::builder(binding.arena().clone(), DragAxis::Horizontal)
                .on_end(move |details| log.borrow_mut().push(details.primary_velocity))
                .build()
        }
        HardwareVelocityProducer::MultiDrag => {
            MultiDragGestureRecognizer::builder(binding.arena().clone(), MultiDragAxis::Horizontal)
                .on_start(move |_, _| Some(Rc::new(DragClient(Rc::clone(&log)))))
                .build()
        }
        HardwareVelocityProducer::Scale => ScaleGestureRecognizer::builder(binding.arena().clone())
            .on_end(move |details| log.borrow_mut().push(details.velocity))
            .build(),
        HardwareVelocityProducer::TapDrag => {
            TapAndDragGestureRecognizer::builder(binding.arena().clone())
                .on_drag_end(move |details| {
                    log.borrow_mut().push(details.velocity.pixels_per_second.dx);
                })
                .build()
        }
    };
    binding
        .pointer_router()
        .add_global_handler(Rc::new(move |event| {
            let dispatch = PointerDispatch::at_root(event);
            if matches!(event, PointerEvent::Down(_)) {
                drag.add_pointer(dispatch);
            } else {
                drag.handle_event(dispatch);
            }
        }));
    binding.handle_pointer_event(
        &hardware_trace_event(
            make_down_event_for_id(
                PointerId::new(core::num::NonZeroU64::MIN),
                Offset::ZERO,
                PointerKind::Touch,
            )
            .expect("finite input"),
            1_000_000_000,
        ),
        |_| HitTestResult::new(),
    );
    let second_pointer = PointerId::try_from(2).expect("nonzero pointer");
    if matches!(producer, HardwareVelocityProducer::Scale) {
        binding.handle_pointer_event(
            &hardware_trace_event(
                make_down_event_for_id(second_pointer, Offset::new(100.0, 0.0), PointerKind::Touch)
                    .expect("finite input"),
                1_000_000_000,
            ),
            |_| HitTestResult::new(),
        );
    }
    for event in curved_hardware_moves() {
        binding.handle_pointer_event(&event, |_| HitTestResult::new());
        if flush_each {
            binding.flush_pending_moves();
        }
    }
    binding.flush_pending_moves();
    binding.handle_pointer_event(
        &hardware_trace_event(
            make_up_event_for_id(
                PointerId::new(core::num::NonZeroU64::MIN),
                Offset::new(50.0, 0.0),
                PointerKind::Touch,
            )
            .expect("finite input"),
            1_050_000_000,
        ),
        |_| HitTestResult::new(),
    );
    if matches!(producer, HardwareVelocityProducer::Scale) {
        binding.handle_pointer_event(
            &hardware_trace_event(
                make_up_event_for_id(second_pointer, Offset::new(100.0, 0.0), PointerKind::Touch)
                    .expect("finite input"),
                1_050_000_000,
            ),
            |_| HitTestResult::new(),
        );
    }
    let values = velocities.borrow();
    assert_eq!(values.len(), 1, "one healthy accepted drag ends");
    assert!(
        binding.arena().is_empty(),
        "terminal delivery retires the arena"
    );
    values[0]
}

fn frame_coalesced_history_reaches_drag_velocity() {
    assert_frame_coalesced_velocity(HardwareVelocityProducer::Drag, 2_000.0);
}

fn frame_coalesced_history_reaches_multi_drag_velocity() {
    assert_frame_coalesced_velocity(HardwareVelocityProducer::MultiDrag, 2_000.0);
}

fn frame_coalesced_history_reaches_scale_velocity() {
    assert_frame_coalesced_velocity(HardwareVelocityProducer::Scale, -20.0);
}

fn frame_coalesced_history_reaches_tap_drag_velocity() {
    assert_frame_coalesced_velocity(HardwareVelocityProducer::TapDrag, 2_000.0);
}

fn assert_frame_coalesced_velocity(producer: HardwareVelocityProducer, expected: f64) {
    let ordinary = velocity_for_hardware_trace(producer, true);
    let tolerance = expected.abs() * 0.01;
    assert!(
        ordinary.is_finite() && (ordinary - expected).abs() < tolerance,
        "ordinary producer proves the hardware curve: {ordinary}"
    );
    let batched = velocity_for_hardware_trace(producer, false);
    let later_ordinary = velocity_for_hardware_trace(producer, true);
    assert!(
        (later_ordinary - ordinary).abs() < 0.01,
        "healthy later dispatch remains reproducible"
    );
    assert!(
        batched.is_finite() && (batched - ordinary).abs() < tolerance,
        "same hardware samples must yield the same end velocity at frame cadence: ordinary={ordinary}, coalesced={batched}"
    );
}

fn capped_contact_never_becomes_hover() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    };
    use flui_interaction::{GestureBinding, HitTestResult, PointerId};
    use std::cell::Cell;

    let binding = GestureBinding::new();
    let hits = Cell::new(0);
    let result = |_| {
        hits.set(hits.get() + 1);
        HitTestResult::new()
    };
    for raw in 1..=33 {
        let pointer = PointerId::try_from(raw).expect("nonzero pointer");
        binding.handle_pointer_event(
            &make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
                .expect("finite input"),
            result,
        );
    }
    assert_eq!(hits.get(), 32, "the thirty-third contact is refused");
    let refused = PointerId::try_from(33).expect("nonzero pointer");
    binding.handle_pointer_event(
        &make_move_event_for_id(refused, Offset::ZERO, PointerKind::Touch).expect("finite input"),
        result,
    );
    binding.flush_pending_moves();
    assert_eq!(hits.get(), 32, "the refused contact does not become hover");
    binding.handle_pointer_event(
        &make_up_event_for_id(refused, Offset::ZERO, PointerKind::Touch).expect("finite input"),
        result,
    );
    binding.handle_pointer_event(
        &make_up_event_for_id(
            PointerId::new(core::num::NonZeroU64::MIN),
            Offset::ZERO,
            PointerKind::Touch,
        )
        .expect("finite input"),
        result,
    );
    binding.handle_pointer_event(
        &make_down_event_for_id(refused, Offset::ZERO, PointerKind::Touch).expect("finite input"),
        result,
    );
    assert_eq!(hits.get(), 33, "a released slot accepts the next sequence");
    binding.handle_lifecycle_pause();
}

fn refusal_saturation_preserves_admitted_contacts_and_resets_with_lifecycle() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerKind, make_cancel_event_for_id, make_down_event_for_id, make_move_event_for_id,
        make_up_event_for_id,
    };
    use flui_interaction::{GestureBinding, HitTestResult, PointerId};
    use std::{cell::Cell, rc::Rc};

    let binding = GestureBinding::new();
    let delivered = Rc::new(Cell::new(0));
    let log = Rc::clone(&delivered);
    binding
        .pointer_router()
        .add_global_handler(Rc::new(move |_| log.set(log.get() + 1)));
    for raw in 1..=65 {
        binding.handle_pointer_event(
            &make_down_event_for_id(
                PointerId::try_from(raw).expect("nonzero pointer"),
                Offset::ZERO,
                PointerKind::Touch,
            )
            .expect("finite input"),
            |_| HitTestResult::new(),
        );
    }
    delivered.set(0);
    binding.handle_pointer_event(
        &make_move_event_for_id(
            PointerId::new(core::num::NonZeroU64::MIN),
            Offset::ZERO,
            PointerKind::Touch,
        )
        .expect("finite input"),
        |_| HitTestResult::new(),
    );
    binding.flush_pending_moves();
    assert_eq!(
        delivered.get(),
        1,
        "saturation preserves an admitted contact's Move"
    );
    delivered.set(0);
    let unknown = PointerId::try_from(200).expect("nonzero pointer");
    binding.handle_pointer_event(
        &make_move_event_for_id(unknown, Offset::ZERO, PointerKind::Touch).expect("finite input"),
        |_| HitTestResult::new(),
    );
    binding.flush_pending_moves();
    binding.handle_pointer_event(
        &make_up_event_for_id(unknown, Offset::ZERO, PointerKind::Touch).expect("finite input"),
        |_| HitTestResult::new(),
    );
    binding.handle_pointer_event(
        &make_cancel_event_for_id(unknown, PointerKind::Touch),
        |_| HitTestResult::new(),
    );
    assert_eq!(
        delivered.get(),
        0,
        "saturation suppresses every untracked tail"
    );
    binding.handle_pointer_event(
        &make_up_event_for_id(
            PointerId::new(core::num::NonZeroU64::MIN),
            Offset::ZERO,
            PointerKind::Touch,
        )
        .expect("finite input"),
        |_| HitTestResult::new(),
    );
    binding.handle_pointer_event(
        &make_down_event_for_id(unknown, Offset::ZERO, PointerKind::Touch).expect("finite input"),
        |_| HitTestResult::new(),
    );
    binding.handle_pointer_event(
        &make_move_event_for_id(unknown, Offset::ZERO, PointerKind::Touch).expect("finite input"),
        |_| HitTestResult::new(),
    );
    binding.flush_pending_moves();
    binding.handle_pointer_event(
        &make_up_event_for_id(unknown, Offset::ZERO, PointerKind::Touch).expect("finite input"),
        |_| HitTestResult::new(),
    );
    assert_eq!(
        delivered.get(),
        4,
        "saturation still admits a fresh Down and delivers its full sequence"
    );
    binding.handle_pointer_event(
        &make_up_event_for_id(
            PointerId::try_from(33).expect("nonzero pointer"),
            Offset::ZERO,
            PointerKind::Touch,
        )
        .expect("finite input"),
        |_| HitTestResult::new(),
    );
    binding.handle_pointer_event(
        &make_move_event_for_id(unknown, Offset::ZERO, PointerKind::Touch).expect("finite input"),
        |_| HitTestResult::new(),
    );
    binding.flush_pending_moves();
    assert_eq!(
        delivered.get(),
        4,
        "an unknown terminal event cannot lift saturation"
    );
    binding.handle_lifecycle_pause();
    binding.handle_pointer_event(
        &make_move_event_for_id(unknown, Offset::ZERO, PointerKind::Touch).expect("finite input"),
        |_| HitTestResult::new(),
    );
    binding.flush_pending_moves();
    assert_eq!(
        delivered.get(),
        5,
        "lifecycle reset restores ordinary hover admission"
    );
}

#[derive(Clone, Copy)]
enum SignalRoute {
    Wheel,
    Pinch,
    CapturedPinch,
}

fn assert_signal_claim_delivery(route: SignalRoute, competing: bool) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerEvent, PointerKind, make_down_event_for_id, make_pinch_gesture_event,
        make_scroll_event, make_up_event_for_id,
    };
    use flui_interaction::{EventPropagation, GestureBinding, HitTestResult, PointerId};
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let failing = Rc::new(Cell::new(true));
    let claims = Rc::new(Cell::new(0));
    let observed = Rc::new(Cell::new(0));
    lane.enter(|| {
        let fail = Rc::clone(&failing);
        let pointer = handle
            .register_pointer(move |dispatch| {
                if matches!(
                    dispatch.local,
                    PointerEvent::Scroll(_) | PointerEvent::PanZoom(_)
                ) && fail.get()
                {
                    panic!("pointer listener first failure");
                }
            })
            .expect("pointer target");
        let later = Rc::clone(&observed);
        let observer = handle
            .register_pointer(move |dispatch| {
                if matches!(
                    dispatch.local,
                    PointerEvent::Scroll(_) | PointerEvent::PanZoom(_)
                ) {
                    later.set(later.get() + 1);
                }
            })
            .expect("later pointer target");
        let calls = Rc::clone(&claims);
        let fail = Rc::clone(&failing);
        let claim = move || {
            calls.set(calls.get() + 1);
            if competing && fail.get() {
                panic!("claim handler second failure");
            }
            EventPropagation::Stop
        };
        let entry = match route {
            SignalRoute::Wheel => hit_entry(pointer).scroll_target(
                handle
                    .register_scroll(move |_| claim())
                    .expect("scroll target"),
            ),
            SignalRoute::Pinch | SignalRoute::CapturedPinch => hit_entry(pointer).pan_zoom_target(
                handle
                    .register_pan_zoom(move |_| claim())
                    .expect("pan-zoom target"),
            ),
        };
        let mut path = HitTestResult::new();
        path.add(entry);
        path.add(hit_entry(observer));
        let signal = match route {
            SignalRoute::Wheel => {
                make_scroll_event(Offset::ZERO, Offset::new(0.0, 10.0)).expect("finite input")
            }
            SignalRoute::Pinch | SignalRoute::CapturedPinch => {
                make_pinch_gesture_event(Offset::ZERO, 0.1).expect("finite input")
            }
        };
        let pointer = PointerId::new(core::num::NonZeroU64::MIN);
        if matches!(route, SignalRoute::CapturedPinch) {
            binding.handle_pointer_event(
                &make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
                    .expect("finite input"),
                |_| path.clone(),
            );
        }
        let failure = catch_unwind(AssertUnwindSafe(|| {
            binding.handle_pointer_event(&signal, |_| path.clone())
        }))
        .expect_err("pointer listener failure propagates");
        assert_eq!(
            flui_foundation::panic::payload_text(&*failure),
            Some("pointer listener first failure"),
            "the first failure remains authoritative"
        );
        assert_eq!(observed.get(), 1, "the pointer observation round finishes");
        assert_eq!(
            claims.get(),
            1,
            "accepted signal reaches the claim walk after pointer failure"
        );
        failing.set(false);
        binding.handle_pointer_event(&signal, |_| path.clone());
        assert_eq!(observed.get(), 2, "healthy observation follows containment");
        assert_eq!(claims.get(), 2, "healthy claim follows containment");
        if matches!(route, SignalRoute::CapturedPinch) {
            binding.handle_pointer_event(
                &make_up_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
                    .expect("finite input"),
                |_| path.clone(),
            );
        }
    });
}

fn wheel_listener_failure_keeps_claim_delivery() {
    assert_signal_claim_delivery(SignalRoute::Wheel, false);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScrollLeaseCase {
    Failure,
    Competing,
    Retired,
    TerminalReentry,
    ClaimReentry,
    FocusLoss,
    KindMetadata,
    RoleMetadata,
    RepeatedStart,
}

fn scroll_selected_failure_keeps_lease() {
    assert_scroll_lease(ScrollLeaseCase::Failure);
}
fn scroll_observer_failure_stays_first() {
    assert_scroll_lease(ScrollLeaseCase::Competing);
}
fn scroll_retired_target_does_not_chain() {
    assert_scroll_lease(ScrollLeaseCase::Retired);
}
fn scroll_terminal_reentry_keeps_new_lease() {
    assert_scroll_lease(ScrollLeaseCase::TerminalReentry);
}
fn scroll_claim_reentry_keeps_new_lease() {
    assert_scroll_lease(ScrollLeaseCase::ClaimReentry);
}
fn scroll_focus_loss_releases_lease() {
    assert_scroll_lease(ScrollLeaseCase::FocusLoss);
}

fn scroll_kind_metadata_keeps_lease() {
    assert_scroll_lease(ScrollLeaseCase::KindMetadata);
}
fn scroll_role_metadata_keeps_lease() {
    assert_scroll_lease(ScrollLeaseCase::RoleMetadata);
}

fn scroll_owner_batch_preserves_reentrant_admission() {
    assert_signal_owner_batch(false, false);
}

fn scroll_device_batch_preserves_reentrant_admission() {
    assert_signal_owner_batch(true, false);
}

fn native_focus_batch_preserves_reentrant_admission() {
    assert_signal_owner_batch(false, true);
}
fn native_device_batch_preserves_reentrant_admission() {
    assert_signal_owner_batch(true, true);
}

fn assert_signal_owner_batch(device_removal: bool, native: bool) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerEvent, PointerKind, make_down_event_for_id, make_scroll_event,
    };
    use flui_interaction::{EventPropagation, GestureBinding, HitTestResult};
    use flui_platform_api::{
        EventTime,
        pointer::{
            DeviceId, PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerDeviceChange, PointerId,
            PointerInfo, PointerPosition, ScrollPhase,
        },
    };
    use std::{cell::Cell, rc::Rc};

    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = Rc::new(GestureBinding::new());
    let selected_calls = Rc::new(Cell::new(0));
    let other_calls = Rc::new(Cell::new(0));
    let device = DeviceId::try_from(1_u64).expect("device");
    let first_pointer = PointerId::try_from(1_u64).expect("pointer");
    let packet = move |phase| {
        if native {
            let native_phase = match phase {
                ScrollPhase::Began => PanZoomPhase::Start,
                ScrollPhase::Changed => PanZoomPhase::Update(
                    PanZoomTransform::try_new(Offset::ZERO, 1.2, 0.0).expect("native transform"),
                ),
                _ => unreachable!("batch fixture phase"),
            };
            return PointerEvent::PanZoom(PanZoomEvent::new(
                PointerInfo::new(
                    PointerId::try_from(3_u64).expect("distinct native pointer"),
                    PointerKind::Trackpad,
                )
                .with_device(device),
                EventTime::from_nanos(0),
                PointerPosition::try_new(flui_foundation::geometry::Point::ZERO).expect("position"),
                native_phase,
            ));
        }
        let PointerEvent::Scroll(mut scroll) =
            make_scroll_event(Offset::ZERO, Offset::new(0.0, 10.0)).expect("finite wheel")
        else {
            unreachable!()
        };
        scroll.pointer.id = PointerId::try_from(3_u64).expect("distinct wheel pointer");
        scroll.pointer = scroll.pointer.with_device(device);
        scroll.phase = Some(phase);
        PointerEvent::Scroll(scroll)
    };
    lane.enter(|| {
        let calls = Rc::clone(&selected_calls);
        let claim = move || {
            calls.set(calls.get() + 1);
            EventPropagation::Stop
        };
        let selected_entry = if native {
            HitTestEntry::new(RenderId::new(1)).pan_zoom_target(
                handle
                    .register_pan_zoom(move |_| claim())
                    .expect("reentrant consumer"),
            )
        } else {
            HitTestEntry::new(RenderId::new(1)).scroll_target(
                handle
                    .register_scroll(move |_| claim())
                    .expect("reentrant consumer"),
            )
        };
        let mut selected_path = HitTestResult::new();
        selected_path.add(selected_entry);
        let weak = Rc::downgrade(&binding);
        let observer = handle
            .register_pointer(move |dispatch| {
                if let PointerEvent::Cancel(cancel) = dispatch.local
                    && cancel.pointer.id == first_pointer
                {
                    weak.upgrade()
                        .expect("binding")
                        .handle_pointer_event(&packet(ScrollPhase::Began), |_| {
                            selected_path.clone()
                        });
                }
            })
            .expect("cancel observer");
        for id in [1_u64, 2] {
            let PointerEvent::Down(mut press) = make_down_event_for_id(
                PointerId::try_from(id).expect("pointer"),
                Offset::ZERO,
                PointerKind::Touch,
            )
            .expect("finite contact") else {
                unreachable!()
            };
            press.pointer = press.pointer.with_device(device);
            binding.handle_pointer_event(&PointerEvent::Down(press), |_| {
                let mut path = HitTestResult::new();
                path.add(hit_entry(observer));
                path
            });
        }
        if device_removal {
            binding.handle_pointer_event(
                &PointerEvent::DeviceRemoved(PointerDeviceChange::new(
                    device,
                    PointerKind::Touch,
                    EventTime::from_nanos(0),
                )),
                |_| panic!("device removal does not hit-test"),
            );
        } else {
            binding.cancel_active_pointers();
        }
        let calls = Rc::clone(&other_calls);
        let claim = move || {
            calls.set(calls.get() + 1);
            EventPropagation::Stop
        };
        let other_entry = if native {
            HitTestEntry::new(RenderId::new(2)).pan_zoom_target(
                handle
                    .register_pan_zoom(move |_| claim())
                    .expect("other consumer"),
            )
        } else {
            HitTestEntry::new(RenderId::new(2)).scroll_target(
                handle
                    .register_scroll(move |_| claim())
                    .expect("other consumer"),
            )
        };
        binding.handle_pointer_event(&packet(ScrollPhase::Changed), |_| {
            let mut path = HitTestResult::new();
            path.add(other_entry);
            path
        });
        assert_eq!(
            selected_calls.get(),
            2,
            "an old cancellation batch cannot withdraw newly admitted scroll work"
        );
        assert_eq!(
            other_calls.get(),
            0,
            "reentrant source keeps exact consumption authority"
        );
    });
}

fn scroll_first_terminal_delta_remains_deliverable() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{PointerEvent, make_scroll_event};
    use flui_interaction::{EventPropagation, GestureBinding, HitTestResult};
    use flui_platform_api::pointer::ScrollPhase;
    use std::{cell::Cell, rc::Rc};

    let packet = |phase| {
        let PointerEvent::Scroll(mut scroll) =
            make_scroll_event(Offset::ZERO, Offset::new(0.0, 10.0)).expect("finite wheel")
        else {
            unreachable!()
        };
        scroll.phase = Some(phase);
        PointerEvent::Scroll(scroll)
    };
    for prior_unconsumed in [false, true] {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = GestureBinding::new();
        let first = Rc::new(Cell::new(0));
        let next = Rc::new(Cell::new(0));
        lane.enter(|| {
            let calls = Rc::clone(&first);
            let target = handle
                .register_scroll(move |_| {
                    calls.set(calls.get() + 1);
                    EventPropagation::Stop
                })
                .expect("terminal consumer");
            let mut path = HitTestResult::new();
            path.add(HitTestEntry::new(RenderId::new(1)).scroll_target(target));
            if prior_unconsumed {
                binding.handle_pointer_event(&packet(ScrollPhase::Began), |_| HitTestResult::new());
            }
            binding.handle_pointer_event(&packet(ScrollPhase::Ended), |_| path.clone());
            assert_eq!(
                first.get(),
                1,
                "first meaningful terminal delta is not discarded"
            );
            let calls = Rc::clone(&next);
            let target = handle
                .register_scroll(move |_| {
                    calls.set(calls.get() + 1);
                    EventPropagation::Stop
                })
                .expect("next consumer");
            let mut path = HitTestResult::new();
            path.add(HitTestEntry::new(RenderId::new(2)).scroll_target(target));
            binding.handle_pointer_event(&packet(ScrollPhase::Changed), |_| path.clone());
            assert_eq!(
                next.get(),
                1,
                "terminal consumption does not retain a scroll lease"
            );
        });
    }
}

fn scroll_claim_survives_capture_retirement() {
    assert_scroll_claim_retirement(false);
}

fn scroll_first_failure_survives_capture_retirement() {
    assert_scroll_claim_retirement(true);
}

fn assert_scroll_claim_retirement(competing: bool) {
    assert_signal_claim_retirement(competing, false);
}

fn native_claim_survives_capture_retirement() {
    assert_signal_claim_retirement(false, true);
}
fn native_first_failure_survives_capture_retirement() {
    assert_signal_claim_retirement(true, true);
}

fn assert_signal_claim_retirement(competing: bool, native: bool) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{PointerEvent, make_scroll_event};
    use flui_interaction::{EventPropagation, GestureBinding, HitTestResult};
    use flui_platform_api::{
        EventTime,
        pointer::{
            PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerId, PointerInfo, PointerKind,
            PointerPosition, ScrollPhase,
        },
    };
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    struct RetiredCapture;
    impl Drop for RetiredCapture {
        fn drop(&mut self) {
            panic!("consumed scroll capture retirement failure");
        }
    }
    let packet = move |phase| {
        if native {
            let native_phase = match phase {
                ScrollPhase::Began => PanZoomPhase::Start,
                ScrollPhase::Changed => PanZoomPhase::Update(
                    PanZoomTransform::try_new(Offset::ZERO, 1.2, 0.0)
                        .expect("finite native transform"),
                ),
                ScrollPhase::Ended => PanZoomPhase::End,
                _ => unreachable!("fixture phase"),
            };
            return PointerEvent::PanZoom(PanZoomEvent::new(
                PointerInfo::new(
                    PointerId::try_from(1_u64).expect("pointer"),
                    PointerKind::Trackpad,
                ),
                EventTime::from_nanos(0),
                PointerPosition::try_new(flui_foundation::geometry::Point::ZERO).expect("position"),
                native_phase,
            ));
        }
        let PointerEvent::Scroll(mut scroll) =
            make_scroll_event(Offset::ZERO, Offset::new(0.0, 10.0)).expect("finite wheel")
        else {
            unreachable!()
        };
        scroll.phase = Some(phase);
        PointerEvent::Scroll(scroll)
    };
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let selected_calls = Rc::new(Cell::new(0));
    let other_calls = Rc::new(Cell::new(0));
    #[derive(Clone, Copy)]
    enum Target {
        Wheel(flui_interaction::ScrollTarget),
        Native(flui_interaction::routing::PanZoomTarget),
    }
    let entry = |id, target| match target {
        Target::Wheel(target) => HitTestEntry::new(RenderId::new(id)).scroll_target(target),
        Target::Native(target) => HitTestEntry::new(RenderId::new(id)).pan_zoom_target(target),
    };
    lane.enter(|| {
        let slot = Rc::new(Cell::new(None));
        let own_slot = Rc::clone(&slot);
        let own_handle = handle.clone();
        let calls = Rc::clone(&selected_calls);
        let capture = RetiredCapture;
        let claim = move || {
            let _ = &capture;
            calls.set(calls.get() + 1);
            match own_slot.get().expect("published target") {
                Target::Wheel(target) => own_handle.unregister_scroll(target),
                Target::Native(target) => own_handle.unregister_pan_zoom(target),
            }
            .expect("withdraw callback ownership");
            EventPropagation::Stop
        };
        let selected = if native {
            Target::Native(
                handle
                    .register_pan_zoom(move |_| claim())
                    .expect("selected target"),
            )
        } else {
            Target::Wheel(
                handle
                    .register_scroll(move |_| claim())
                    .expect("selected target"),
            )
        };
        slot.set(Some(selected));
        let observer = handle
            .register_pointer(move |_| {
                if competing {
                    panic!("scroll observer first failure");
                }
            })
            .expect("observer");
        let mut selected_path = HitTestResult::new();
        selected_path.add(hit_entry(observer));
        selected_path.add(entry(1, selected));
        let payload = catch_unwind(AssertUnwindSafe(|| {
            binding.handle_pointer_event(&packet(ScrollPhase::Began), |_| selected_path.clone());
        }))
        .expect_err("capture retirement propagates after consumption");
        assert_eq!(
            flui_foundation::panic::payload_text(&*payload),
            Some(if competing {
                "scroll observer first failure"
            } else {
                "consumed scroll capture retirement failure"
            })
        );
        assert_eq!(
            selected_calls.get(),
            1,
            "accepted claim runs despite earlier observer failure"
        );
        let calls = Rc::clone(&other_calls);
        let claim = move || {
            calls.set(calls.get() + 1);
            EventPropagation::Stop
        };
        let other = if native {
            Target::Native(
                handle
                    .register_pan_zoom(move |_| claim())
                    .expect("healthy target"),
            )
        } else {
            Target::Wheel(
                handle
                    .register_scroll(move |_| claim())
                    .expect("healthy target"),
            )
        };
        let mut fresh_path = HitTestResult::new();
        fresh_path.add(entry(2, other));
        binding.handle_pointer_event(&packet(ScrollPhase::Changed), |_| fresh_path.clone());
        assert_eq!(
            other_calls.get(),
            0,
            "consumption stays latched even when its captures retire with a panic"
        );
        binding.handle_pointer_event(&packet(ScrollPhase::Ended), |_| fresh_path.clone());
        binding.handle_pointer_event(&packet(ScrollPhase::Began), |_| fresh_path.clone());
        assert_eq!(
            other_calls.get(),
            1,
            "next sequence recovers after retirement containment"
        );
    });
}

fn assert_scroll_lease(case: ScrollLeaseCase) {
    assert_signal_lease(case, false);
}

fn native_selected_failure_keeps_lease() {
    assert_signal_lease(ScrollLeaseCase::Failure, true);
}
fn native_observer_failure_stays_first() {
    assert_signal_lease(ScrollLeaseCase::Competing, true);
}
fn native_retired_target_does_not_chain() {
    assert_signal_lease(ScrollLeaseCase::Retired, true);
}
fn native_terminal_reentry_keeps_new_lease() {
    assert_signal_lease(ScrollLeaseCase::TerminalReentry, true);
}
fn native_claim_reentry_keeps_new_lease() {
    assert_signal_lease(ScrollLeaseCase::ClaimReentry, true);
}
fn native_focus_loss_releases_lease() {
    assert_signal_lease(ScrollLeaseCase::FocusLoss, true);
}

fn native_repeated_start_keeps_selected_owner() {
    assert_signal_lease(ScrollLeaseCase::RepeatedStart, true);
}

fn assert_signal_lease(case: ScrollLeaseCase, native: bool) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{PointerEvent, make_scroll_event};
    use flui_interaction::{EventPropagation, GestureBinding, HitTestResult};
    use flui_platform_api::{
        EventTime,
        pointer::{
            PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerId, PointerInfo, PointerKind,
            PointerPosition, ScrollPhase,
        },
    };
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    let packet = move |phase| {
        if native {
            let native_phase = match phase {
                ScrollPhase::Began => PanZoomPhase::Start,
                ScrollPhase::Changed => PanZoomPhase::Update(
                    PanZoomTransform::try_new(Offset::ZERO, 1.2, 0.0)
                        .expect("finite native transform"),
                ),
                ScrollPhase::Ended => PanZoomPhase::End,
                _ => unreachable!("fixture phase"),
            };
            return PointerEvent::PanZoom(PanZoomEvent::new(
                PointerInfo::new(
                    PointerId::try_from(1_u64).expect("pointer"),
                    PointerKind::Trackpad,
                ),
                EventTime::from_nanos(0),
                PointerPosition::try_new(flui_foundation::geometry::Point::ZERO).expect("position"),
                native_phase,
            ));
        }
        let PointerEvent::Scroll(mut scroll) =
            make_scroll_event(Offset::ZERO, Offset::new(0.0, 10.0)).expect("finite wheel")
        else {
            unreachable!()
        };
        scroll.phase = Some(phase);
        if phase == ScrollPhase::Changed {
            if case == ScrollLeaseCase::KindMetadata {
                scroll.pointer.kind = PointerKind::Pen {
                    tool: flui_platform_api::pointer::PenTool::Eraser,
                };
            }
            if case == ScrollLeaseCase::RoleMetadata {
                scroll.pointer.role = flui_platform_api::pointer::PointerRole::Primary;
            }
        }
        PointerEvent::Scroll(scroll)
    };
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = Rc::new(GestureBinding::new());
    let selected_calls = Rc::new(Cell::new(0));
    let fresh_calls = Rc::new(Cell::new(0));
    let observed = Rc::new(Cell::new(0));
    let failing = Rc::new(Cell::new(false));
    #[derive(Clone, Copy)]
    enum Target {
        Wheel(flui_interaction::ScrollTarget),
        Native(flui_interaction::routing::PanZoomTarget),
    }
    let entry = |id, target| match target {
        Target::Wheel(target) => HitTestEntry::new(RenderId::new(id)).scroll_target(target),
        Target::Native(target) => HitTestEntry::new(RenderId::new(id)).pan_zoom_target(target),
    };
    lane.enter(|| {
        let calls = Rc::clone(&fresh_calls);
        let claim = move || {
            calls.set(calls.get() + 1);
            EventPropagation::Stop
        };
        let fresh = if native {
            Target::Native(
                handle
                    .register_pan_zoom(move |_| claim())
                    .expect("fresh target"),
            )
        } else {
            Target::Wheel(
                handle
                    .register_scroll(move |_| claim())
                    .expect("fresh target"),
            )
        };
        let mut fresh_path = HitTestResult::new();
        fresh_path.add(entry(2, fresh));
        let weak = Rc::downgrade(&binding);
        let replacement = fresh_path.clone();
        let count = Rc::clone(&observed);
        let fail = Rc::clone(&failing);
        let observer = handle
            .register_pointer(move |dispatch| {
                let terminal = match dispatch.local {
                    PointerEvent::Scroll(scroll) => scroll.phase == Some(ScrollPhase::Ended),
                    PointerEvent::PanZoom(event) => event.phase == PanZoomPhase::End,
                    _ => return,
                };
                count.set(count.get() + 1);
                if case == ScrollLeaseCase::TerminalReentry && terminal {
                    weak.upgrade()
                        .expect("binding")
                        .handle_pointer_event(&packet(ScrollPhase::Began), |_| replacement.clone());
                }
                if case == ScrollLeaseCase::Competing && fail.get() {
                    panic!("fresh scroll observer first failure");
                }
            })
            .expect("fresh observer");
        fresh_path.add(hit_entry(observer));
        let calls = Rc::clone(&selected_calls);
        let fail = Rc::clone(&failing);
        let weak = Rc::downgrade(&binding);
        let replacement = fresh_path.clone();
        let claim = move || {
            calls.set(calls.get() + 1);
            if case == ScrollLeaseCase::ClaimReentry && calls.get() == 1 {
                weak.upgrade()
                    .expect("binding")
                    .handle_pointer_event(&packet(ScrollPhase::Began), |_| replacement.clone());
            }
            if fail.get() {
                panic!("selected scroll consumer failure");
            }
            EventPropagation::Stop
        };
        let selected = if native {
            Target::Native(
                handle
                    .register_pan_zoom(move |_| claim())
                    .expect("selected target"),
            )
        } else {
            Target::Wheel(
                handle
                    .register_scroll(move |_| claim())
                    .expect("selected target"),
            )
        };
        let mut selected_path = HitTestResult::new();
        selected_path.add(entry(1, selected));
        binding.handle_pointer_event(&packet(ScrollPhase::Began), |_| selected_path.clone());
        match case {
            ScrollLeaseCase::Failure | ScrollLeaseCase::Competing => {
                failing.set(true);
                let payload = catch_unwind(AssertUnwindSafe(|| {
                    binding.handle_pointer_event(&packet(ScrollPhase::Changed), |_| {
                        fresh_path.clone()
                    });
                }))
                .expect_err("selected consumer must still run off its fresh hit path");
                assert_eq!(
                    flui_foundation::panic::payload_text(&*payload),
                    Some(if case == ScrollLeaseCase::Competing {
                        "fresh scroll observer first failure"
                    } else {
                        "selected scroll consumer failure"
                    })
                );
                failing.set(false);
                binding.handle_pointer_event(&packet(ScrollPhase::Changed), |_| fresh_path.clone());
                assert_eq!(
                    selected_calls.get(),
                    3,
                    "selected consumer recovers after containment"
                );
                assert_eq!(fresh_calls.get(), 0, "later path cannot steal the sequence");
                assert_eq!(observed.get(), 2, "raw observers follow each fresh path");
            }
            ScrollLeaseCase::Retired => {
                match selected {
                    Target::Wheel(target) => handle.unregister_scroll(target),
                    Target::Native(target) => handle.unregister_pan_zoom(target),
                }
                .expect("retire selected target");
                binding.handle_pointer_event(&packet(ScrollPhase::Changed), |_| fresh_path.clone());
                assert_eq!(
                    fresh_calls.get(),
                    0,
                    "retirement does not hand this gesture to an ancestor"
                );
                binding.handle_pointer_event(&packet(ScrollPhase::Ended), |_| fresh_path.clone());
                binding.handle_pointer_event(&packet(ScrollPhase::Began), |_| fresh_path.clone());
                assert_eq!(
                    fresh_calls.get(),
                    1,
                    "the next gesture can select a healthy target"
                );
            }
            ScrollLeaseCase::TerminalReentry => {
                binding.handle_pointer_event(&packet(ScrollPhase::Ended), |_| fresh_path.clone());
                assert_eq!(
                    selected_calls.get(),
                    2,
                    "the admitted old consumer receives terminal before the next packet"
                );
                assert_eq!(
                    fresh_calls.get(),
                    1,
                    "the replacement receives only its reentrant Begin"
                );
                binding
                    .handle_pointer_event(&packet(ScrollPhase::Changed), |_| selected_path.clone());
                assert_eq!(
                    selected_calls.get(),
                    2,
                    "terminal reaches old consumer once"
                );
                assert_eq!(
                    fresh_calls.get(),
                    2,
                    "reentrant begin survives old terminal delivery"
                );
            }
            ScrollLeaseCase::ClaimReentry => {
                binding
                    .handle_pointer_event(&packet(ScrollPhase::Changed), |_| selected_path.clone());
                assert_eq!(
                    selected_calls.get(),
                    1,
                    "old Stop cannot overwrite a replacement begin"
                );
                assert_eq!(
                    fresh_calls.get(),
                    2,
                    "replacement consumer remains selected"
                );
            }
            ScrollLeaseCase::FocusLoss => {
                binding.cancel_active_pointers();
                binding.handle_pointer_event(&packet(ScrollPhase::Changed), |_| fresh_path.clone());
                assert_eq!(
                    fresh_calls.get(),
                    1,
                    "focus loss releases scroll without a Down contact"
                );
            }
            ScrollLeaseCase::KindMetadata | ScrollLeaseCase::RoleMetadata => {
                binding.handle_pointer_event(&packet(ScrollPhase::Changed), |_| fresh_path.clone());
                assert_eq!(
                    selected_calls.get(),
                    2,
                    "tool and role metadata do not replace source identity"
                );
                assert_eq!(fresh_calls.get(), 0, "metadata update cannot steal a lease");
            }
            ScrollLeaseCase::RepeatedStart => {
                binding.handle_pointer_event(&packet(ScrollPhase::Began), |_| fresh_path.clone());
                binding.handle_pointer_event(&packet(ScrollPhase::Changed), |_| fresh_path.clone());
                assert_eq!(
                    selected_calls.get(),
                    3,
                    "replacement cumulative session stays with its admitted consumer"
                );
                assert_eq!(
                    fresh_calls.get(),
                    0,
                    "repeated Start cannot transfer ownership mid-stream"
                );
                binding.handle_pointer_event(&packet(ScrollPhase::Ended), |_| fresh_path.clone());
                binding.handle_pointer_event(&packet(ScrollPhase::Began), |_| fresh_path.clone());
                assert_eq!(
                    fresh_calls.get(),
                    1,
                    "fresh source after terminal can choose a new consumer"
                );
            }
        }
    });
}
fn wheel_first_failure_survives_claim_failure() {
    assert_signal_claim_delivery(SignalRoute::Wheel, true);
}
fn pinch_listener_failure_keeps_claim_delivery() {
    assert_signal_claim_delivery(SignalRoute::Pinch, false);
}
fn pinch_first_failure_survives_claim_failure() {
    assert_signal_claim_delivery(SignalRoute::Pinch, true);
}
fn captured_pinch_listener_failure_keeps_claim_delivery() {
    assert_signal_claim_delivery(SignalRoute::CapturedPinch, false);
}
fn captured_pinch_first_failure_survives_claim_failure() {
    assert_signal_claim_delivery(SignalRoute::CapturedPinch, true);
}

assert_not_impl_any!(InteractionLane: Send, Sync);
assert_impl_all!(InteractionDispatchHandle: Clone, Send, Sync);
assert_impl_all!(PointerTarget: Copy, Send, Sync);
assert_impl_all!(MouseRegionTarget: Copy, Send, Sync);
assert_impl_all!(ResolvedRouteToken: Copy, Send, Sync);

// Lane capability matrix: least-privilege handle and realm recreation.
#[test]
fn lane_capability_matrix() {
    let cases: &[(&str, fn())] = &[
        (
            "lane_mints_a_send_safe_least_privilege_handle",
            lane_mints_a_send_safe_least_privilege_handle,
        ),
        (
            "realm_recreation_rejects_every_old_capability",
            realm_recreation_rejects_every_old_capability,
        ),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("matrix case `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

fn lane_mints_a_send_safe_least_privilege_handle() {
    let lane = InteractionLane::try_new().expect("lane identity should be available");
    let handle = lane.dispatch_handle();
    assert_eq!(format!("{handle:?}"), "InteractionDispatchHandle { .. }");
}

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

// ---------------------------------------------------------------------------
// Fresh hit test — the capability a drag needs to discover targets it has moved
// over, which a replayed pointer-down route can never see.
//
// The handle pairs a realm ticket with ONE presentation's probe: identity is
// realm-wide, the tree is not, and a realm may host several presentations each
// with its own render tree.
// ---------------------------------------------------------------------------

#[test]
fn pointer_route_retirement_reenters_and_preserves_the_next_contact() {
    let cases: &[(&str, fn())] = &[
        ("capture_reentry", pointer_route_capture_reentry),
        (
            "capture_panic_after_reentry",
            pointer_route_capture_panic_after_reentry,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            flui_foundation::panic::retain_opaque_payload(payload);
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "failed retirement rows: {failures:?}");
}

fn pointer_route_capture_reentry() {
    assert_pointer_route_retirement(false);
}

fn pointer_route_capture_panic_after_reentry() {
    assert_pointer_route_retirement(true);
}

fn assert_pointer_route_retirement(panic_after_reentry: bool) {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::{Rc, Weak};

    use flui_interaction::events::{PointerKind, make_down_event, make_up_event};
    use flui_interaction::{GestureBinding, HitTestResult, Offset, PointerId};

    struct RetireRoute {
        binding: Weak<GestureBinding>,
        deliveries: Rc<Cell<usize>>,
        retired: Rc<Cell<bool>>,
        panic_after_reentry: bool,
    }

    impl Drop for RetireRoute {
        fn drop(&mut self) {
            let binding = self.binding.upgrade().expect("binding outlives removal");
            let router = binding.pointer_router();
            assert!(
                !router.has_routes(PointerId::new(core::num::NonZeroU64::MIN)),
                "the retired route must already be absent during destruction"
            );
            // Removing an already-absent ID must also be reentrant.
            router.remove_all_routes(PointerId::new(core::num::NonZeroU64::MIN));
            let deliveries = Rc::clone(&self.deliveries);
            router.add_route(
                PointerId::new(core::num::NonZeroU64::MIN),
                Rc::new(move |_| deliveries.set(deliveries.get() + 1)),
            );
            self.retired.set(true);
            assert!(!self.panic_after_reentry, "retired route capture panic");
        }
    }

    let binding = Rc::new(GestureBinding::new());
    let deliveries = Rc::new(Cell::new(0));
    let retired = Rc::new(Cell::new(false));
    let retire = RetireRoute {
        binding: Rc::downgrade(&binding),
        deliveries: Rc::clone(&deliveries),
        retired: Rc::clone(&retired),
        panic_after_reentry,
    };
    binding.pointer_router().add_route(
        PointerId::new(core::num::NonZeroU64::MIN),
        Rc::new(move |_| {
            let _keep_capture_alive = &retire;
            panic!("a retired route must not receive the next contact");
        }),
    );

    let removal = catch_unwind(AssertUnwindSafe(|| {
        binding
            .pointer_router()
            .remove_all_routes(PointerId::new(core::num::NonZeroU64::MIN));
    }));
    if panic_after_reentry {
        let payload = removal.expect_err("capture panic must propagate");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"retired route capture panic")
        );
    } else {
        removal.expect("capture reentry must not borrow an occupied registry");
    }
    assert!(
        retired.get(),
        "capture must finish its reentrant replacement"
    );

    // Deliver a real new contact through the production binding, rather than
    // checking only registry counters or calling the replacement directly.
    binding.handle_pointer_event(
        &make_down_event(Offset::ZERO, PointerKind::Touch).expect("finite input"),
        |_| HitTestResult::new(),
    );
    binding.handle_pointer_event(
        &make_up_event(Offset::ZERO, PointerKind::Touch).expect("finite input"),
        |_| HitTestResult::new(),
    );
    assert_eq!(deliveries.get(), 2);
    binding
        .pointer_router()
        .remove_all_routes(PointerId::new(core::num::NonZeroU64::MIN));
}

#[derive(Clone, Copy, Debug)]
enum RouterCleanup {
    Pointer,
    Global,
    All,
}

impl RouterCleanup {
    fn remove(self, router: &flui_interaction::PointerRouter) {
        match self {
            Self::Pointer => router
                .remove_all_routes(flui_interaction::PointerId::new(core::num::NonZeroU64::MIN)),
            Self::Global => router.clear_global_handlers(),
            Self::All => router.clear(),
        }
    }
}

#[test]
fn pointer_router_competing_retirement_preserves_first_failure_and_recovery() {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const SELECTED: &str = "FLUI_POINTER_ROUTER_RETIREMENT_CASE";
    let cases = [
        ("pointer_one", RouterCleanup::Pointer, false, false),
        ("pointer_two", RouterCleanup::Pointer, true, false),
        ("pointer_unwind", RouterCleanup::Pointer, true, true),
        ("global_one", RouterCleanup::Global, false, false),
        ("global_two", RouterCleanup::Global, true, false),
        ("global_unwind", RouterCleanup::Global, true, true),
        ("all_one", RouterCleanup::All, false, false),
        ("all_two", RouterCleanup::All, true, false),
        ("all_unwind", RouterCleanup::All, true, true),
    ];
    if let Ok(selected) = std::env::var(SELECTED) {
        if selected == "saved_route_entries" {
            assert_saved_route_entry_retirement();
            return;
        }
        if let Some((competing, active_unwind)) = match selected.as_str() {
            "owner_one" => Some((false, false)),
            "owner_two" => Some((true, false)),
            "owner_unwind" => Some((true, true)),
            _ => None,
        } {
            assert_router_owner_retirement(competing, active_unwind);
            return;
        }
        let &(_, cleanup, competing, active_unwind) = cases
            .iter()
            .find(|(name, _, _, _)| *name == selected)
            .expect("known router retirement child case");
        assert_router_retirement_recovery(cleanup, competing, active_unwind);
        return;
    }

    let mut failures = Vec::new();
    for name in cases.iter().map(|(name, _, _, _)| *name).chain([
        "owner_one",
        "owner_two",
        "owner_unwind",
        "saved_route_entries",
    ]) {
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "interaction_lane::pointer_router_competing_retirement_preserves_first_failure_and_recovery",
                "--nocapture",
            ])
            .env(SELECTED, name)
            .env("RUST_BACKTRACE", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("router retirement child");
        let mut stdout = child.stdout.take().expect("child stdout");
        let mut stderr = child.stderr.take().expect("child stderr");
        let stdout_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).expect("read child stdout");
            text
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stderr.read_to_string(&mut text).expect("read child stderr");
            text
        });
        let started = Instant::now();
        while child.try_wait().expect("child status").is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().expect("kill deadlocked retirement child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("child exit");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if !status.success() || !stdout.contains("1 passed; 0 failed") {
            failures.push(format!("{name}: {status}\n{stdout}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn assert_router_retirement_recovery(cleanup: RouterCleanup, competing: bool, active_unwind: bool) {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::{Rc, Weak};

    use flui_interaction::events::{PointerKind, make_down_event, make_up_event};
    use flui_interaction::{GestureBinding, HitTestResult, Offset, PointerId};

    struct RetireCapture {
        binding: Weak<GestureBinding>,
        drops: Rc<Cell<usize>>,
        deliveries: Rc<Cell<usize>>,
        message: &'static str,
    }
    impl Drop for RetireCapture {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            let binding = self.binding.upgrade().expect("live binding");
            let deliveries = Rc::clone(&self.deliveries);
            binding.pointer_router().add_route(
                PointerId::new(core::num::NonZeroU64::MIN),
                Rc::new(move |_| deliveries.set(deliveries.get() + 1)),
            );
            std::panic::panic_any(self.message);
        }
    }
    struct UnwindCleanup {
        binding: Rc<GestureBinding>,
        cleanup: RouterCleanup,
    }
    impl Drop for UnwindCleanup {
        fn drop(&mut self) {
            self.cleanup.remove(self.binding.pointer_router());
        }
    }

    let binding = Rc::new(GestureBinding::new());
    let deliveries = Rc::new(Cell::new(0));
    let first_drops = Rc::new(Cell::new(0));
    let second_drops = Rc::new(Cell::new(0));
    for (index, (drops, message)) in [
        (&first_drops, "first route capture failure"),
        (&second_drops, "second route capture failure"),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 1 && !competing {
            break;
        }
        let capture = RetireCapture {
            binding: Rc::downgrade(&binding),
            drops: Rc::clone(drops),
            deliveries: Rc::clone(&deliveries),
            message,
        };
        let handler: flui_interaction::PointerRouteHandler = Rc::new(move |_| {
            let _keep_capture_alive = &capture;
            panic!("retired callback must not receive the next contact");
        });
        if matches!(cleanup, RouterCleanup::Global)
            || (matches!(cleanup, RouterCleanup::All) && index == 1)
        {
            binding.pointer_router().add_global_handler(handler);
        } else {
            binding
                .pointer_router()
                .add_route(PointerId::new(core::num::NonZeroU64::MIN), handler);
        }
    }

    // A handler the test still owns sits in the retired tail: dropping the
    // router's clone runs no user code, so retirement must release it.
    let shared: flui_interaction::PointerRouteHandler = Rc::new(|_| {});
    if matches!(cleanup, RouterCleanup::Pointer) {
        binding.pointer_router().add_route(
            PointerId::new(core::num::NonZeroU64::MIN),
            Rc::clone(&shared),
        );
    } else {
        binding
            .pointer_router()
            .add_global_handler(Rc::clone(&shared));
    }

    let removal = catch_unwind(AssertUnwindSafe(|| {
        if active_unwind {
            let _cleanup = UnwindCleanup {
                binding: Rc::clone(&binding),
                cleanup,
            };
            panic!("outer failure owns unwind");
        }
        cleanup.remove(binding.pointer_router());
    }));
    let payload = removal.expect_err("the first failure must propagate");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&if active_unwind {
            "outer failure owns unwind"
        } else {
            "first route capture failure"
        })
    );
    assert_eq!(first_drops.get(), usize::from(!active_unwind));
    assert_eq!(second_drops.get(), 0, "the opaque tail must be retained");
    assert_eq!(
        Rc::strong_count(&shared),
        1,
        "a clone another owner still holds is released, not leaked"
    );

    if active_unwind {
        let deliveries = Rc::clone(&deliveries);
        binding.pointer_router().add_route(
            PointerId::new(core::num::NonZeroU64::MIN),
            Rc::new(move |_| deliveries.set(deliveries.get() + 1)),
        );
    }
    binding.handle_pointer_event(
        &make_down_event(Offset::ZERO, PointerKind::Touch).expect("finite input"),
        |_| HitTestResult::new(),
    );
    binding.handle_pointer_event(
        &make_up_event(Offset::ZERO, PointerKind::Touch).expect("finite input"),
        |_| HitTestResult::new(),
    );
    assert_eq!(
        deliveries.get(),
        2,
        "next contact reaches only the healthy route"
    );
    binding.pointer_router().clear();
}

fn assert_router_owner_retirement(competing: bool, active_unwind: bool) {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::{Rc, Weak};

    use flui_interaction::{PointerId, PointerRouter};

    struct OwnerCapture {
        owner: Weak<PointerRouter>,
        drops: Rc<Cell<usize>>,
        missing_owner: Rc<Cell<bool>>,
        message: &'static str,
    }
    impl Drop for OwnerCapture {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            // Last-owner teardown has no live public handle to reenter. The
            // weak identity must refuse resurrection rather than name a new owner.
            self.missing_owner.set(self.owner.upgrade().is_none());
            std::panic::panic_any(self.message);
        }
    }

    let owner = Rc::new(PointerRouter::new());
    let weak_owner = Rc::downgrade(&owner);
    let first_drops = Rc::new(Cell::new(0));
    let second_drops = Rc::new(Cell::new(0));
    let missing_owner = Rc::new(Cell::new(false));
    for (index, (drops, message)) in [
        (&first_drops, "first owner capture failure"),
        (&second_drops, "second owner capture failure"),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 1 && !competing {
            break;
        }
        let capture = OwnerCapture {
            owner: Rc::downgrade(&owner),
            drops: Rc::clone(drops),
            missing_owner: Rc::clone(&missing_owner),
            message,
        };
        let handler: flui_interaction::PointerRouteHandler = Rc::new(move |_| {
            let _keep_capture_alive = &capture;
        });
        if index == 0 {
            owner.add_route(PointerId::new(core::num::NonZeroU64::MIN), handler);
        } else {
            owner.add_global_handler(handler);
        }
    }
    let retirement = catch_unwind(AssertUnwindSafe(move || {
        if active_unwind {
            let _last_owner = owner;
            panic!("outer owner failure");
        }
        drop(owner);
    }));
    let payload = retirement.expect_err("original owner failure must propagate");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&if active_unwind {
            "outer owner failure"
        } else {
            "first owner capture failure"
        })
    );
    assert_eq!(first_drops.get(), usize::from(!active_unwind));
    assert_eq!(
        second_drops.get(),
        0,
        "the owner retirement tail is retained"
    );
    assert_eq!(missing_owner.get(), !active_unwind);
    assert!(
        weak_owner.upgrade().is_none(),
        "retired owner cannot resurrect"
    );
}

/// A saved route that holds the last owner of two targets' captures: when the
/// first capture's destructor fails, the second is retained rather than
/// dropped during that unwind, and the lane serves the next route.
fn assert_saved_route_entry_retirement() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_interaction::events::{PointerKind, make_down_event};
    use flui_interaction::{HitTestResult, Offset};

    struct FailingCapture {
        drops: Arc<AtomicUsize>,
        message: &'static str,
    }
    impl Drop for FailingCapture {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::Relaxed);
            std::panic::panic_any(self.message);
        }
    }

    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let drops = Arc::new(AtomicUsize::new(0));
    lane.enter(|| {
        let mut result = HitTestResult::new();
        let mut targets = Vec::new();
        for message in [
            "first route capture failure",
            "second route capture failure",
        ] {
            let capture = FailingCapture {
                drops: Arc::clone(&drops),
                message,
            };
            let target = handle
                .register_pointer(move |_| {
                    let _keep_capture_alive = &capture;
                })
                .expect("target");
            result.add(hit_entry(target));
            targets.push(target);
        }
        let token = handle
            .resolve_pointer_route(result.path())
            .expect("route")
            .token();
        for target in targets {
            handle.unregister_pointer(target).expect("unregister");
        }
        let failure = catch_unwind(AssertUnwindSafe(|| handle.release_route(token)))
            .expect_err("the first capture failure propagates");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"first route capture failure")
        );
        assert_eq!(drops.load(Ordering::Relaxed), 1, "the second is retained");

        let delivered = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = std::rc::Rc::clone(&delivered);
        let next = handle
            .register_pointer(move |_| counter.set(counter.get() + 1))
            .expect("next target");
        let token = handle
            .resolve_pointer_route(&[hit_entry(next)])
            .expect("next route")
            .token();
        let event = make_down_event(Offset::ZERO, PointerKind::Touch).expect("finite input");
        assert!(
            handle
                .invoke_pointer_route(token, &event)
                .expect("dispatch")
                .is_none()
        );
        assert_eq!(delivered.get(), 1, "the lane serves the next route");
        handle.release_route(token).expect("release");
        handle.unregister_pointer(next).expect("unregister next");
    });
}

struct DragRetirementProbe(Box<dyn Fn()>);

impl Drop for DragRetirementProbe {
    fn drop(&mut self) {
        (self.0)();
    }
}

#[test]
fn drag_callback_ownership_and_retirement() {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const SELECTED: &str = "FLUI_DRAG_CALLBACK_RETIREMENT_CASE";
    let cases: &[(&str, fn())] = &[
        ("dispose reentry", drag_dispose_capture_reentry),
        (
            "all replacement slots",
            drag_callback_replacements_commit_before_retirement,
        ),
        ("closed admission", drag_dispose_closes_callback_admission),
        (
            "two dispose failures",
            drag_dispose_preserves_first_capture_failure,
        ),
        ("dispose active unwind", drag_dispose_during_active_unwind),
        (
            "previous caller failure",
            drag_caller_keeps_previously_caught_failure,
        ),
        (
            "healthy shared owner",
            drag_callbacks_live_until_the_final_shared_owner,
        ),
        (
            "two owner failures",
            drag_final_callback_owner_preserves_first_failure,
        ),
        (
            "owner active unwind",
            drag_final_callback_owner_during_active_unwind,
        ),
        (
            "callback self dispose",
            drag_callback_can_dispose_its_own_recognizer,
        ),
        (
            "callback body and capture",
            drag_callback_body_failure_retains_its_capture,
        ),
        (
            "rejection diagnostic failure",
            drag_disposal_commits_tracking_before_rejection_diagnostics,
        ),
        (
            "stop diagnostic failure",
            drag_completion_commits_tracking_before_stop_diagnostics,
        ),
        (
            "stop sweep reentry",
            stop_tracking_preserves_reentrant_contact_after_sweep,
        ),
        (
            "stop sweep reentry failure",
            stop_tracking_preserves_reentrant_contact_after_sweep_failure,
        ),
        (
            "stop sweep accepts drag",
            stop_tracking_pointer_sweep_starts_the_unresolved_drag,
        ),
    ];
    if let Ok(selected) = std::env::var(SELECTED) {
        cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("known drag retirement case")
            .1();
        return;
    }
    let mut failures = Vec::new();
    for &(name, _) in cases {
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "interaction_lane::drag_callback_ownership_and_retirement",
                "--nocapture",
            ])
            .env(SELECTED, name)
            .env("RUST_BACKTRACE", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("drag retirement child");
        let mut stdout = child.stdout.take().expect("child stdout");
        let mut stderr = child.stderr.take().expect("child stderr");
        let stdout_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).expect("child stdout");
            text
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stderr.read_to_string(&mut text).expect("child stderr");
            text
        });
        let started = Instant::now();
        while child.try_wait().expect("child status").is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().expect("kill deadlocked drag retirement child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("child exit");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if !status.success() || !stdout.contains("1 passed; 0 failed") {
            failures.push(format!("{name}: {status}\n{stdout}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn fresh_drag_completes_after_retirement() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::events::{PointerKind, make_move_event, make_up_event};
    use flui_interaction::routing::PointerDispatch;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer, Offset, PointerId};
    use std::{cell::Cell, rc::Rc};

    let arena = GestureArena::new();
    let started = Rc::new(Cell::new(0));
    let ended = Rc::new(Cell::new(0));
    let observed_start = started.clone();
    let observed_end = ended.clone();
    let recognizer = DragGestureRecognizer::builder(arena.clone(), DragAxis::Horizontal)
        .on_start(move |_| observed_start.set(observed_start.get() + 1))
        .on_end(move |_| observed_end.set(observed_end.get() + 1))
        .build();
    let down = flui_interaction::events::make_down_event(Offset::ZERO, PointerKind::Touch)
        .expect("finite input");
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(PointerId::new(core::num::NonZeroU64::MIN));
    arena.drain_deferred_resolutions();
    let movement =
        make_move_event(Offset::new(30.0, 0.0), PointerKind::Touch).expect("finite input");
    recognizer.handle_event(PointerDispatch::at_root(&movement));
    let release = make_up_event(Offset::new(30.0, 0.0), PointerKind::Touch).expect("finite input");
    recognizer.handle_event(PointerDispatch::at_root(&release));
    assert_eq!((started.get(), ended.get()), (1, 1));
    assert_eq!(recognizer.cancel(), flui_interaction::CancelOutcome::Idle);
}

fn drag_dispose_capture_reentry() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer};
    use std::{
        cell::{Cell, RefCell},
        rc::{Rc, Weak},
    };

    let owner = Rc::new(RefCell::new(Weak::<DragGestureRecognizer>::new()));
    let weak = owner.clone();
    struct Rival(Rc<Cell<usize>>);
    impl flui_interaction::arena::GestureArenaMember for Rival {
        fn accept_gesture(&self, _: flui_interaction::PointerId) {
            self.0.set(self.0.get() + 1);
        }
        fn reject_gesture(&self, _: flui_interaction::PointerId) {
            panic!("retiring owner must not reject its rival");
        }
    }
    let arena = GestureArena::new();
    let reentrant_arena = arena.clone();
    let accepts = Rc::new(Cell::new(0));
    let rival = Rc::new(Rival(accepts.clone()));
    let retired = Rc::new(Cell::new(false));
    let observed = retired.clone();
    let probe = DragRetirementProbe(Box::new(move || {
        assert!(
            weak.borrow().upgrade().is_none(),
            "final owner is already unavailable to reentry"
        );
        assert_eq!(
            reentrant_arena.drain_deferred_resolutions(),
            1,
            "capture retirement may reenter the exact owner arena"
        );
        assert!(reentrant_arena.is_empty());
        observed.set(true);
    }));
    let recognizer = DragGestureRecognizer::builder(arena.clone(), DragAxis::Horizontal)
        .on_down(move |_| {
            let _capture = &probe;
        })
        .build();
    *owner.borrow_mut() = Rc::downgrade(&recognizer);
    let down = flui_interaction::events::make_down_event(
        flui_interaction::Offset::ZERO,
        flui_interaction::events::PointerKind::Touch,
    )
    .expect("finite input");
    recognizer.add_pointer(flui_interaction::routing::PointerDispatch::at_root(&down));
    arena.add(
        flui_interaction::PointerId::new(core::num::NonZeroU64::MIN),
        &rival,
    );
    arena.close(flui_interaction::PointerId::new(core::num::NonZeroU64::MIN));
    drop(recognizer);
    assert!(retired.get());
    assert_eq!(
        accepts.get(),
        1,
        "reentrant retirement delivers the accepted rival once"
    );
    fresh_drag_completes_after_retirement();
}

fn drag_callback_replacements_commit_before_retirement() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer};
    use std::{cell::Cell, rc::Rc};

    for slot in 0..5 {
        let recognizer =
            DragGestureRecognizer::builder(GestureArena::new(), DragAxis::Horizontal).build();
        let weak = Rc::downgrade(&recognizer);
        let retired = Rc::new(Cell::new(false));
        let observed = retired.clone();
        let probe = DragRetirementProbe(Box::new(move || {
            let recognizer = weak.upgrade().expect("replacement pins public owner");
            assert_eq!(recognizer.cancel(), flui_interaction::CancelOutcome::Idle);
            observed.set(true);
        }));
        let builder = DragGestureRecognizer::builder(GestureArena::new(), DragAxis::Horizontal);
        let builder = match slot {
            0 => builder.on_down(move |_| {
                let _capture = &probe;
            }),
            1 => builder.on_start(move |_| {
                let _capture = &probe;
            }),
            2 => builder.on_update(move |_| {
                let _capture = &probe;
            }),
            3 => builder.on_end(move |_| {
                let _capture = &probe;
            }),
            _ => builder.on_cancel(move || {
                let _capture = &probe;
            }),
        };
        let builder = match slot {
            0 => builder.on_down(|_| {}),
            1 => builder.on_start(|_| {}),
            2 => builder.on_update(|_| {}),
            3 => builder.on_end(|_| {}),
            _ => builder.on_cancel(|| {}),
        };
        assert!(retired.get(), "slot {slot} retires the replaced capture");
        drop(builder.build());
        drop(recognizer);
    }
    fresh_drag_completes_after_retirement();
}

fn drag_dispose_closes_callback_admission() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer};
    use std::{cell::Cell, rc::Rc};

    let retired = Rc::new(Cell::new(0));
    for slot in 0..5 {
        let observed = retired.clone();
        let probe = DragRetirementProbe(Box::new(move || observed.set(observed.get() + 1)));
        let builder = DragGestureRecognizer::builder(GestureArena::new(), DragAxis::Horizontal);
        let builder = match slot {
            0 => builder.on_down(move |_| {
                let _capture = &probe;
            }),
            1 => builder.on_start(move |_| {
                let _capture = &probe;
            }),
            2 => builder.on_update(move |_| {
                let _capture = &probe;
            }),
            3 => builder.on_end(move |_| {
                let _capture = &probe;
            }),
            _ => builder.on_cancel(move || {
                let _capture = &probe;
            }),
        };
        drop(builder);
        assert_eq!(
            retired.get(),
            slot + 1,
            "an abandoned builder retires its capture without admitting a contact"
        );
    }
    fresh_drag_completes_after_retirement();
}

fn drag_dispose_preserves_first_capture_failure() {
    drag_independent_capture_failures(false, false);
}
fn drag_dispose_during_active_unwind() {
    drag_independent_capture_failures(false, true);
}
fn drag_final_callback_owner_preserves_first_failure() {
    drag_independent_capture_failures(true, false);
}
fn drag_final_callback_owner_during_active_unwind() {
    drag_independent_capture_failures(true, true);
}

fn drag_independent_capture_failures(final_owner: bool, active_unwind: bool) {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::{cell::Cell, rc::Rc};

    let first_drops = Rc::new(Cell::new(0));
    let second_drops = Rc::new(Cell::new(0));
    let first = first_drops.clone();
    let second = second_drops.clone();
    let down = DragRetirementProbe(Box::new(move || {
        first.set(first.get() + 1);
        panic!("first drag capture failure");
    }));
    let start = DragRetirementProbe(Box::new(move || {
        second.set(second.get() + 1);
        panic!("second drag capture failure");
    }));
    let builder = DragGestureRecognizer::builder(GestureArena::new(), DragAxis::Horizontal)
        .on_down(move |_| {
            let _capture = &down;
        })
        .on_start(move |_| {
            let _capture = &start;
        });
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if final_owner {
            let recognizer = builder.build();
            let alias = Rc::clone(&recognizer);
            drop(recognizer);
            assert_eq!((first_drops.get(), second_drops.get()), (0, 0));
            if active_unwind {
                let _owner = alias;
                panic!("incoming drag failure");
            }
            drop(alias);
        } else {
            if active_unwind {
                let _builder = builder;
                panic!("incoming drag failure");
            }
            drop(builder);
        }
    }));
    let payload = outcome.expect_err("first drag failure propagates");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&if active_unwind {
            "incoming drag failure"
        } else {
            "first drag capture failure"
        })
    );
    assert_eq!(first_drops.get(), usize::from(!active_unwind));
    assert_eq!(
        second_drops.get(),
        0,
        "independent tail capture is retained"
    );
    fresh_drag_completes_after_retirement();
}

fn drag_caller_keeps_previously_caught_failure() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let caller_failure = catch_unwind(|| panic!("caller already caught failure"))
        .expect_err("caller owns prior payload");
    let probe = DragRetirementProbe(Box::new(|| panic!("new retirement failure")));
    let recognizer = DragGestureRecognizer::builder(GestureArena::new(), DragAxis::Horizontal)
        .on_down(move |_| {
            let _capture = &probe;
        })
        .build();
    let outcome = catch_unwind(AssertUnwindSafe(|| drop(recognizer)));
    assert_eq!(
        outcome
            .expect_err("healthy call reports its own failure")
            .downcast_ref::<&str>(),
        Some(&"new retirement failure")
    );
    assert_eq!(
        caller_failure.downcast_ref::<&str>(),
        Some(&"caller already caught failure")
    );
    fresh_drag_completes_after_retirement();
}

fn drag_callbacks_live_until_the_final_shared_owner() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer};
    use std::{cell::Cell, rc::Rc};
    let dropped = Rc::new(Cell::new(0));
    let mut builder = DragGestureRecognizer::builder(GestureArena::new(), DragAxis::Horizontal);
    for slot in 0..5 {
        let observed = dropped.clone();
        let probe = DragRetirementProbe(Box::new(move || observed.set(observed.get() + 1)));
        builder = match slot {
            0 => builder.on_down(move |_| {
                let _capture = &probe;
            }),
            1 => builder.on_start(move |_| {
                let _capture = &probe;
            }),
            2 => builder.on_update(move |_| {
                let _capture = &probe;
            }),
            3 => builder.on_end(move |_| {
                let _capture = &probe;
            }),
            _ => builder.on_cancel(move || {
                let _capture = &probe;
            }),
        };
    }
    let recognizer = builder.build();
    let alias = Rc::clone(&recognizer);
    drop(recognizer);
    assert_eq!(dropped.get(), 0, "a recognizer alias still owns callbacks");
    drop(alias);
    assert_eq!(
        dropped.get(),
        5,
        "healthy final owner destroys every capture"
    );
    fresh_drag_completes_after_retirement();
}

fn drag_callback_can_dispose_its_own_recognizer() {
    drag_self_dispose_from_callback(false);
}
fn drag_callback_body_failure_retains_its_capture() {
    drag_self_dispose_from_callback(true);
}

fn drag_self_dispose_from_callback(body_failure: bool) {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer, Offset};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::{
        cell::{Cell, RefCell},
        rc::{Rc, Weak},
    };

    let owner = Rc::new(RefCell::new(Weak::<DragGestureRecognizer>::new()));
    let weak = owner.clone();
    let drops = Rc::new(Cell::new(0));
    let observed = drops.clone();
    let probe = DragRetirementProbe(Box::new(move || {
        observed.set(observed.get() + 1);
        assert!(!body_failure, "drag body capture failure");
    }));
    let recognizer = DragGestureRecognizer::builder(GestureArena::new(), DragAxis::Horizontal)
        .on_down(move |_| {
            let _capture = &probe;
            let recognizer = weak.borrow().upgrade().expect("live recognizer callback");
            assert_eq!(
                recognizer.cancel(),
                flui_interaction::CancelOutcome::Cancelled
            );
            assert!(!body_failure, "drag callback body failure");
        })
        .build();
    *owner.borrow_mut() = Rc::downgrade(&recognizer);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let down = flui_interaction::events::make_down_event(
            Offset::ZERO,
            flui_interaction::events::PointerKind::Touch,
        )
        .expect("finite input");
        recognizer.add_pointer(flui_interaction::routing::PointerDispatch::at_root(&down));
    }));
    if body_failure {
        assert_eq!(
            outcome
                .expect_err("body failure propagates")
                .downcast_ref::<&str>(),
            Some(&"drag callback body failure")
        );
        assert_eq!(drops.get(), 0, "failed body retains its opaque capture");
    } else {
        outcome.expect("callback self disposal is reentrant");
        assert_eq!(
            drops.get(),
            0,
            "cancel preserves immutable callbacks for the next contact"
        );
    }
    assert_eq!(recognizer.cancel(), flui_interaction::CancelOutcome::Idle);
    drop(recognizer);
    assert_eq!(
        drops.get(),
        usize::from(!body_failure),
        "only healthy final ownership retires the opaque capture"
    );
    fresh_drag_completes_after_retirement();
}

fn drag_disposal_commits_tracking_before_rejection_diagnostics() {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use flui_interaction::arena::GestureArena;
    use flui_interaction::arena::GestureArenaMember;
    use flui_interaction::{ArenaMembership, GestureSettings, Offset, PointerId, PrimaryContact};

    struct RejectEventPanic(Arc<AtomicBool>);
    impl tracing::Subscriber for RejectEventPanic {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            metadata.name() == "recognizer.reject" && *metadata.level() == tracing::Level::DEBUG
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            assert_eq!(event.metadata().name(), "recognizer.reject");
            self.0.store(true, Ordering::Relaxed);
            panic!("recognizer rejection diagnostic failure");
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }
    struct Sibling(Rc<Cell<usize>>);
    impl GestureArenaMember for Sibling {
        fn accept_gesture(&self, _: PointerId) {
            self.0.set(self.0.get() + 1);
        }
        fn reject_gesture(&self, _: PointerId) {
            panic!("disposing the drag must preserve its sibling");
        }
    }

    struct WithdrawingContact(PrimaryContact);
    impl GestureArenaMember for WithdrawingContact {
        fn accept_gesture(&self, _: PointerId) {
            panic!("withdrawing member must not win its old competition");
        }
        fn reject_gesture(&self, _: PointerId) {
            panic!("locally retired contact must not receive its own rejection");
        }
    }

    let arena = GestureArena::new();
    let recognizer = Rc::new_cyclic(|this: &std::rc::Weak<WithdrawingContact>| {
        WithdrawingContact(PrimaryContact::new(ArenaMembership::new(
            arena.clone(),
            this.clone(),
        )))
    });
    let down = flui_interaction::events::make_down_event(
        Offset::ZERO,
        flui_interaction::events::PointerKind::Touch,
    )
    .expect("finite input");
    recognizer
        .0
        .begin(
            flui_interaction::routing::PointerDispatch::at_root(&down),
            &GestureSettings::default(),
        )
        .expect("contact admitted");
    let sibling_accepts = Rc::new(Cell::new(0));
    let sibling = Rc::new(Sibling(sibling_accepts.clone()));
    arena.add(PointerId::new(core::num::NonZeroU64::MIN), &sibling);
    arena.close(PointerId::new(core::num::NonZeroU64::MIN));
    assert_eq!(
        arena.member_count(PointerId::new(core::num::NonZeroU64::MIN)),
        2
    );

    let diagnostic_ran = Arc::new(AtomicBool::new(false));
    let result =
        tracing::subscriber::with_default(RejectEventPanic(diagnostic_ran.clone()), || {
            catch_unwind(AssertUnwindSafe(|| recognizer.0.withdraw()))
        });
    let payload = result.expect_err("the actual debug event subscriber must fail");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"recognizer rejection diagnostic failure")
    );
    assert!(
        diagnostic_ran.load(Ordering::Relaxed),
        "the event actually ran"
    );
    assert!(
        recognizer.0.current().is_none(),
        "tracking commits before diagnostic code"
    );
    assert_eq!(
        arena.member_count(PointerId::new(core::num::NonZeroU64::MIN)),
        1,
        "only the sibling remains admitted"
    );
    assert_eq!(arena.drain_deferred_resolutions(), 1);
    assert_eq!(
        sibling_accepts.get(),
        1,
        "the sibling still makes progress after diagnostic failure"
    );
    assert!(arena.is_empty());
    assert!(recognizer.0.withdraw().is_none());
    let next = recognizer
        .0
        .begin(
            flui_interaction::routing::PointerDispatch::at_root(&down),
            &GestureSettings::default(),
        )
        .expect("withdrawn owner admits next contact");
    assert!(recognizer.0.is_current(next));
    arena.close(PointerId::new(core::num::NonZeroU64::MIN));
    assert_eq!(
        recognizer.0.withdraw().expect("healthy next withdrawal").id,
        next
    );
    assert!(recognizer.0.current().is_none());
    assert!(arena.is_empty());
    fresh_drag_completes_after_retirement();
}

fn drag_completion_commits_tracking_before_stop_diagnostics() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::events::{PointerKind, make_move_event, make_up_event};
    use flui_interaction::routing::PointerDispatch;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer, Offset, PointerId};
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    struct StopEventPanic(Arc<AtomicBool>);
    impl tracing::Subscriber for StopEventPanic {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            metadata.name() == "recognizer.stop_tracking"
                && *metadata.level() == tracing::Level::DEBUG
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            assert_eq!(event.metadata().name(), "recognizer.stop_tracking");
            self.0.store(true, Ordering::Relaxed);
            panic!("recognizer stop diagnostic failure");
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }
    let arena = GestureArena::new();
    let starts = Rc::new(Cell::new(0));
    let ends = Rc::new(Cell::new(0));
    let observed_start = starts.clone();
    let observed_end = ends.clone();
    let recognizer = DragGestureRecognizer::builder(arena.clone(), DragAxis::Horizontal)
        .on_start(move |_| observed_start.set(observed_start.get() + 1))
        .on_end(move |_| observed_end.set(observed_end.get() + 1))
        .build();
    let down = flui_interaction::events::make_down_event(Offset::ZERO, PointerKind::Touch)
        .expect("finite input");
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(PointerId::new(core::num::NonZeroU64::MIN));
    arena.drain_deferred_resolutions();
    let movement =
        make_move_event(Offset::new(30.0, 0.0), PointerKind::Touch).expect("finite input");
    let release = make_up_event(Offset::new(30.0, 0.0), PointerKind::Touch).expect("finite input");
    recognizer.handle_event(PointerDispatch::at_root(&movement));
    assert_eq!(starts.get(), 1);

    let diagnostic_ran = Arc::new(AtomicBool::new(false));
    let result = tracing::subscriber::with_default(StopEventPanic(diagnostic_ran.clone()), || {
        catch_unwind(AssertUnwindSafe(|| {
            recognizer.handle_event(PointerDispatch::at_root(&release));
        }))
    });
    let payload = result.expect_err("the actual stop debug event must fail");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"recognizer stop diagnostic failure")
    );
    assert!(
        diagnostic_ran.load(Ordering::Relaxed),
        "the event actually ran"
    );
    assert!(
        recognizer.cancel() == flui_interaction::CancelOutcome::Idle,
        "terminal contact committed before diagnostics"
    );
    assert!(arena.is_empty());
    assert_eq!(
        ends.get(),
        0,
        "a failed lifecycle phase must not invoke the end callback"
    );

    // A second actual drag on the same recognizer proves the failed terminal
    // contact did not wedge its tracking or delete subsequent work.
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(PointerId::new(core::num::NonZeroU64::MIN));
    arena.drain_deferred_resolutions();
    recognizer.handle_event(PointerDispatch::at_root(&movement));
    recognizer.handle_event(PointerDispatch::at_root(&release));
    assert_eq!((starts.get(), ends.get()), (2, 1));
    assert_eq!(recognizer.cancel(), flui_interaction::CancelOutcome::Idle);
    fresh_drag_completes_after_retirement();
}

fn stop_tracking_pointer_sweep_starts_the_unresolved_drag() {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::arena::GestureArenaMember;
    use flui_interaction::{DragAxis, DragGestureRecognizer, GestureRecognizer, Offset, PointerId};
    use std::cell::Cell;
    use std::rc::Rc;

    struct Rival(Rc<Cell<usize>>);
    impl GestureArenaMember for Rival {
        fn accept_gesture(&self, _: PointerId) {
            panic!("the sweep accepts the front member");
        }
        fn reject_gesture(&self, _: PointerId) {
            self.0.set(self.0.get() + 1);
        }
    }

    let arena = GestureArena::new();
    let starts = Rc::new(Cell::new(0));
    let observed = starts.clone();
    let recognizer = DragGestureRecognizer::builder(arena.clone(), DragAxis::Horizontal)
        .on_start(move |_| observed.set(observed.get() + 1))
        .build();
    let down = flui_interaction::events::make_down_event(
        Offset::ZERO,
        flui_interaction::events::PointerKind::Touch,
    )
    .expect("finite input");
    recognizer.add_pointer(flui_interaction::routing::PointerDispatch::at_root(&down));
    let rejections = Rc::new(Cell::new(0));
    let rival = Rc::new(Rival(rejections.clone()));
    arena.add(PointerId::new(core::num::NonZeroU64::MIN), &rival);
    arena.close(PointerId::new(core::num::NonZeroU64::MIN));
    assert_eq!(starts.get(), 0, "the competition is still open");

    arena.sweep(PointerId::new(core::num::NonZeroU64::MIN));
    assert_eq!(
        (starts.get(), rejections.get()),
        (1, 1),
        "the sweep accepts the retiring drag and starts it"
    );
    assert_eq!(
        recognizer.cancel(),
        flui_interaction::CancelOutcome::Cancelled
    );
    assert!(arena.is_empty());
    assert_eq!(recognizer.cancel(), flui_interaction::CancelOutcome::Idle);
    fresh_drag_completes_after_retirement();
}

fn stop_tracking_preserves_reentrant_contact_after_sweep() {
    assert_stop_tracking_preserves_reentrant_contact(false);
}

fn stop_tracking_preserves_reentrant_contact_after_sweep_failure() {
    assert_stop_tracking_preserves_reentrant_contact(true);
}

fn assert_stop_tracking_preserves_reentrant_contact(fail_after_admission: bool) {
    use flui_interaction::arena::GestureArena;
    use flui_interaction::arena::GestureArenaMember;
    use flui_interaction::events::{PointerKind, make_down_event};
    use flui_interaction::routing::PointerDispatch;
    use flui_interaction::{ArenaMembership, GestureSettings, Offset, PointerId, PrimaryContact};
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    struct ReentrantSweep {
        contact: PrimaryContact,
        admitted_next: Cell<bool>,
        next_accepts: Rc<Cell<usize>>,
        fail_after_admission: bool,
    }
    impl GestureArenaMember for ReentrantSweep {
        fn accept_gesture(&self, pointer: PointerId) {
            if self.admitted_next.replace(true) {
                self.next_accepts.set(self.next_accepts.get() + 1);
                return;
            }
            assert_eq!(
                self.contact.current().map(|contact| contact.pointer),
                Some(pointer),
                "the retiring contact stays visible to its sweep resolution"
            );
            // Reuse the platform pointer ID while the old exact-generation
            // sweep is delivering. The new arena slot belongs to this contact.
            self.contact.withdraw().expect("retiring contact");
            let down =
                make_down_event(Offset::new(7.0, 3.0), PointerKind::Touch).expect("finite input");
            self.contact
                .begin(PointerDispatch::at_root(&down), &GestureSettings::default())
                .expect("next generation admitted");
            assert!(
                !self.fail_after_admission,
                "sweep failure after next contact admission"
            );
        }
        fn reject_gesture(&self, _: PointerId) {
            panic!("swept front member must be accepted");
        }
    }
    let arena = GestureArena::new();
    let next_accepts = Rc::new(Cell::new(0));
    let member = Rc::new_cyclic(|this: &std::rc::Weak<ReentrantSweep>| ReentrantSweep {
        contact: PrimaryContact::new(ArenaMembership::new(arena.clone(), this.clone())),
        admitted_next: Cell::new(false),
        next_accepts: next_accepts.clone(),
        fail_after_admission,
    });
    let down = make_down_event(Offset::ZERO, PointerKind::Touch).expect("finite input");
    let retiring = member
        .contact
        .begin(PointerDispatch::at_root(&down), &GestureSettings::default())
        .expect("initial generation admitted");
    let result = catch_unwind(AssertUnwindSafe(|| member.contact.finish()));
    if fail_after_admission {
        assert_eq!(
            result
                .expect_err("sweep body failure propagates")
                .downcast_ref::<&str>(),
            Some(&"sweep failure after next contact admission")
        );
    } else {
        result.expect("reentrant sweep finishes");
    }
    let next = member
        .contact
        .current()
        .expect("reentrant contact survives old cleanup");
    assert_ne!(
        next.id, retiring,
        "reused pointer gets a distinct contact generation"
    );
    assert_eq!(next.pointer, PointerId::new(core::num::NonZeroU64::MIN));
    assert_eq!(next.local, Offset::new(7.0, 3.0));
    assert_eq!(next.global, Offset::new(7.0, 3.0));
    assert!(
        member
            .contact
            .tracks(PointerId::new(core::num::NonZeroU64::MIN))
    );
    assert_eq!(
        arena.member_count(PointerId::new(core::num::NonZeroU64::MIN)),
        1
    );
    assert!(arena.is_open(PointerId::new(core::num::NonZeroU64::MIN)));
    member.contact.finish();
    assert!(member.contact.current().is_none());
    assert!(
        !member
            .contact
            .tracks(PointerId::new(core::num::NonZeroU64::MIN))
    );
    assert_eq!(next_accepts.get(), 1, "accepted tail remains deliverable");
    assert!(arena.is_empty());
    fresh_drag_completes_after_retirement();
}

/// A batch snapshots its callbacks before running any of them. When one of
/// them closes its presentation, the rest of the batch from that presentation
/// must not run.
#[test]
fn reentrant_presentation_close_stops_snapshotted_callbacks() {
    let cases: &[(&str, fn())] = &[
        (
            "mouse tracker enter batch",
            enter_batch_stops_after_owner_close,
        ),
        (
            "mouse tracker hover batch",
            hover_batch_stops_after_owner_close,
        ),
        (
            "interleaved hover dispatch",
            interleaved_hover_stops_after_owner_close,
        ),
    ];
    let mut failed = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            failed.push(name);
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
    assert!(failed.is_empty(), "failed cases: {failed:?}");
}

/// Two hover-sensitive regions of one presentation whose callbacks each count
/// a call and close the presentation's dispatch owner.
struct ClosingRegions {
    lane: InteractionLane,
    calls: std::rc::Rc<std::cell::Cell<usize>>,
    result: flui_interaction::HitTestResult,
}

fn closing_regions(on_enter: bool) -> ClosingRegions {
    use flui_interaction::__runtime::{CloseMode, close_dispatch, presentation_dispatch};
    use flui_interaction::routing::{MouseRegionCallbacks, MouseTrackerAnnotation};
    use std::rc::Rc;

    let lane = InteractionLane::try_new().expect("lane");
    let owner = presentation_dispatch(&lane.dispatch_handle());
    let calls = Rc::new(std::cell::Cell::new(0));
    let mut result = flui_interaction::HitTestResult::new();
    lane.enter(|| {
        for region in 1..=2_usize {
            let counted = Rc::clone(&calls);
            let closing = owner.clone();
            let callback: flui_interaction::routing::MouseEnterCallback = Rc::new(move |_, _| {
                counted.set(counted.get() + 1);
                close_dispatch(&closing, CloseMode::Ordinary);
            });
            let callbacks = if on_enter {
                MouseRegionCallbacks {
                    on_enter: Some(callback),
                    ..MouseRegionCallbacks::default()
                }
            } else {
                MouseRegionCallbacks {
                    on_hover: Some(callback),
                    ..MouseRegionCallbacks::default()
                }
            };
            let target = owner.register_mouse_region(callbacks).expect("region");
            result.add(
                HitTestEntry::new(RenderId::new(region))
                    .mouse_annotation(MouseTrackerAnnotation::new(RenderId::new(region), target)),
            );
        }
    });
    ClosingRegions {
        lane,
        calls,
        result,
    }
}

/// A buttonless mouse move: the only shape that carries hover semantics.
fn hover_move() -> flui_interaction::events::PointerEvent {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{PointerButtons, PointerEvent, PointerKind, make_move_event};

    let mut event = make_move_event(Offset::ZERO, PointerKind::Mouse).expect("finite input");
    if let PointerEvent::Move(update) = &mut event {
        update.buttons = PointerButtons::NONE;
    }
    event
}

fn enter_batch_stops_after_owner_close() {
    use flui_interaction::routing::{MouseTracker, PointerMotionKind};

    let regions = closing_regions(true);
    let tracker = MouseTracker::new();
    regions.lane.enter(|| {
        tracker.update_with_motion(&hover_move(), PointerMotionKind::Hover, &regions.result);
    });
    assert_eq!(
        regions.calls.get(),
        1,
        "the closed owner's later region is skipped"
    );
}

fn hover_batch_stops_after_owner_close() {
    use flui_interaction::routing::MouseTracker;

    let regions = closing_regions(false);
    let tracker = MouseTracker::new();
    regions.lane.enter(|| {
        let panic = tracker.dispatch_hover(&hover_move(), &regions.result);
        assert!(panic.is_none());
    });
    assert_eq!(
        regions.calls.get(),
        1,
        "the closed owner's later region is skipped"
    );
}

fn interleaved_hover_stops_after_owner_close() {
    use flui_interaction::GestureBinding;

    let regions = closing_regions(false);
    let binding = GestureBinding::new();
    regions.lane.enter(|| {
        binding.handle_pointer_event(&hover_move(), |_| regions.result.clone());
        binding.flush_pending_moves();
    });
    assert_eq!(
        regions.calls.get(),
        1,
        "the closed owner's later region is skipped"
    );
}

/// Records whether a callback capture was destroyed, and whether by an unwind.
struct CaptureProbe(std::rc::Rc<std::cell::Cell<Option<bool>>>);

impl Drop for CaptureProbe {
    fn drop(&mut self) {
        self.0.set(Some(std::thread::panicking()));
    }
}

/// A scroll, pan-zoom, path-clip or shader-mask callback that closes its
/// presentation and then panics leaves its snapshot and target cell as the
/// capture's last owners: they are retained, not destroyed by that unwind.
#[test]
fn non_pointer_invocation_retains_its_snapshot_across_a_reentrant_close() {
    use flui_foundation::geometry::{Offset, Rect, Size};
    use flui_interaction::__runtime::{CloseMode, close_dispatch, presentation_dispatch};
    use flui_interaction::PointerId;
    use flui_interaction::events::{
        PanZoomEvent, PanZoomPhase, PointerInfo, PointerKind, PointerPosition,
    };
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    for kind in ["scroll", "pan-zoom", "path clip", "shader mask"] {
        let lane = InteractionLane::try_new().expect("lane");
        let owner = presentation_dispatch(&lane.dispatch_handle());
        let dropped = Rc::new(std::cell::Cell::new(None));
        let probe = CaptureProbe(Rc::clone(&dropped));
        let closing = owner.clone();
        let close_then_fail = move || -> ! {
            let _keep = &probe;
            close_dispatch(&closing, CloseMode::Ordinary);
            panic!("reentrant close callback");
        };
        let failure = lane.enter(|| {
            catch_unwind(AssertUnwindSafe(|| match kind {
                "scroll" => {
                    let target = owner
                        .register_scroll(move |_| close_then_fail())
                        .expect("scroll target");
                    let flui_interaction::events::PointerEvent::Scroll(scroll) =
                        flui_interaction::events::make_scroll_event(Offset::ZERO, Offset::ZERO)
                            .expect("finite scroll")
                    else {
                        unreachable!()
                    };
                    let event = scroll;
                    let _ = owner.invoke_scroll_target(target, &event);
                }
                "pan-zoom" => {
                    let target = owner
                        .register_pan_zoom(move |_| close_then_fail())
                        .expect("pan-zoom target");
                    let event = PanZoomEvent::new(
                        PointerInfo::new(
                            PointerId::try_from(1).expect("nonzero pointer id"),
                            PointerKind::Trackpad,
                        ),
                        flui_platform_api::EventTime::from_nanos(0),
                        PointerPosition::try_new(flui_foundation::geometry::Point::ZERO)
                            .expect("finite position"),
                        PanZoomPhase::Start,
                    );
                    let _ = owner.invoke_pan_zoom_target(
                        target,
                        flui_interaction::routing::PanZoomDispatch {
                            local: &event,
                            global: &event,
                        },
                    );
                }
                "path clip" => {
                    let target = owner
                        .register_path_clipper(move |_| close_then_fail())
                        .expect("path clip target");
                    let _ = owner.invoke_path_clipper(target, Size::new(1.0, 1.0));
                }
                _ => {
                    let target = owner
                        .register_shader_mask(move |_| close_then_fail())
                        .expect("shader mask target");
                    let _ = owner.invoke_shader_mask(target, Rect::from_ltwh(0.0, 0.0, 1.0, 1.0));
                }
            }))
            .expect_err("the callback's failure propagates")
        });
        assert_eq!(
            flui_foundation::panic::payload_text(&*failure),
            Some("reentrant close callback"),
            "{kind}"
        );
        flui_foundation::panic::retain_opaque_payload(failure);
        assert_eq!(
            dropped.get(),
            None,
            "{kind}: the capture is retained, not destroyed by the unwind"
        );
        assert_eq!(
            owner.check_realm(),
            Err(InteractionDispatchError::OwnerGone),
            "{kind}: the reentrant close took effect"
        );
    }
}

/// A presentation-scoped handle mutates only the targets its own owner
/// registered; a sibling presentation sharing the realm is refused.
#[test]
fn scoped_handle_cannot_mutate_a_sibling_owners_targets() {
    use flui_interaction::__runtime::presentation_dispatch;
    use flui_interaction::routing::MouseRegionCallbacks;

    let lane = InteractionLane::try_new().expect("lane");
    let realm = lane.dispatch_handle();
    let owner = presentation_dispatch(&realm);
    let sibling = presentation_dispatch(&realm);
    lane.enter(|| {
        let pointer = owner.register_pointer(|_| {}).expect("pointer target");
        let scroll = owner
            .register_scroll(|_| flui_interaction::EventPropagation::Continue)
            .expect("scroll target");
        let region = owner
            .register_mouse_region(MouseRegionCallbacks::default())
            .expect("mouse region");
        assert_eq!(
            sibling.replace_pointer(pointer, |_| {}),
            Err(InteractionDispatchError::TargetGone)
        );
        assert_eq!(
            sibling.unregister_pointer(pointer),
            Err(InteractionDispatchError::TargetGone)
        );
        assert_eq!(
            sibling.unregister_scroll(scroll),
            Err(InteractionDispatchError::TargetGone)
        );
        assert!(
            sibling.detach_mouse_region(region).is_err(),
            "a sibling cannot detach the region"
        );
        assert_eq!(owner.replace_pointer(pointer, |_| {}), Ok(()));
        assert_eq!(owner.unregister_pointer(pointer), Ok(()));
        assert_eq!(owner.unregister_scroll(scroll), Ok(()));
        assert!(owner.detach_mouse_region(region).is_ok());
    });
}
