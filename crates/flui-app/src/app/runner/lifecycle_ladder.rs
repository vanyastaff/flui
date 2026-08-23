#[cfg(not(target_os = "ios"))]
use flui_scheduler::AppLifecycleState;

// ============================================================================
// Lifecycle derivation and ladder synthesis (see ADR-0035)
// ============================================================================

/// Derives the Flutter-parity [`AppLifecycleState`] from the two window
/// signals FLUI tracks per window: visibility (occlusion) and focus.
///
/// Pure and order-insensitive: the result depends only on the final
/// `(visible, focused)` pair, never on which of the two changed most
/// recently — occlusion-before-focus-loss and focus-loss-before-occlusion
/// converge to the same derived state once both signals have landed.
#[cfg(not(target_os = "ios"))]
pub(super) fn derive_lifecycle_state(visible: bool, focused: bool) -> AppLifecycleState {
    if !visible {
        AppLifecycleState::Hidden
    } else if focused {
        AppLifecycleState::Resumed
    } else {
        AppLifecycleState::Inactive
    }
}

/// The intermediate `AppLifecycleState` steps between `old` and `new`,
/// inclusive of `new`, exclusive of `old`.
///
/// Faithful port of `ServicesBinding._generateStateTransitions`
/// (`packages/flutter/lib/src/services/binding.dart` @ 3.44.0) — NOT a walk
/// over this enum's own `#[repr(u8)]` discriminants, which exist for
/// FLUI's `frames_enabled` derivation and do not match Flutter's ladder
/// order. Flutter's `dart:ui` `AppLifecycleState` enum declares `detached`
/// **first** (`engine/.../platform_dispatcher.dart`: `detached, resumed,
/// inactive, hidden, paused` — `detached` is the state the engine starts in
/// *before* initialization, not a terminal "highest" state), which is
/// exactly [`AppLifecycleState::ALL`]'s order — the array this function
/// walks, not `as u8`.
///
/// Three cases, mirroring the oracle exactly:
/// - **Target is `Detached`**: walk forward from `old` to the end of `ALL`
///   (through every remaining non-detached state), then append `Detached`
///   itself. This is Flutter's dedicated `state == detached` branch — going
///   to `Detached` always visits every state after `old`, regardless of
///   where `old` sits.
/// - **Going backward** (`old`'s index > `new`'s index, e.g. `Paused` ->
///   `Resumed`): the intermediate states in *descending* index order,
///   ending at `new` (Flutter's `insert(0, ...)` loop, which prepends and
///   so reverses the ascending walk).
/// - **Going forward** (otherwise): the intermediate states in ascending
///   index order, ending at `new`.
///
/// Because `Detached` sits at index 0 (the lowest), a transition FROM
/// `Detached` to anything else always takes the forward branch: `Detached
/// -> Resumed` is the single step `[Resumed]`, not a crawl through
/// `Paused`/`Hidden`/`Inactive` first — reachable via Android's Pause/Resume
/// reroute if `UpdateScheduler::lifecycle_state()`'s corrupt-byte fallback
/// (`try_from_u8`'s `unwrap_or(AppLifecycleState::Detached)`) is ever hit.
///
/// Returns an empty `Vec` when `old == new` — this is where change-detection
/// for the whole re-derivation lives: a wake that doesn't change the derived
/// state emits nothing, to neither the scheduler nor `WidgetsBinding`
/// observers.
#[cfg(not(target_os = "ios"))]
fn lifecycle_ladder(old: AppLifecycleState, new: AppLifecycleState) -> Vec<AppLifecycleState> {
    if old == new {
        return Vec::new();
    }

    let order = AppLifecycleState::ALL;
    let old_idx = order
        .iter()
        .position(|&s| s == old)
        .expect("BUG: every AppLifecycleState variant must appear in AppLifecycleState::ALL");
    let new_idx = order
        .iter()
        .position(|&s| s == new)
        .expect("BUG: every AppLifecycleState variant must appear in AppLifecycleState::ALL");

    if new == AppLifecycleState::Detached {
        let mut steps: Vec<AppLifecycleState> = order[old_idx + 1..].to_vec();
        steps.push(AppLifecycleState::Detached);
        steps
    } else if old_idx > new_idx {
        order[new_idx..old_idx].iter().rev().copied().collect()
    } else {
        order[old_idx + 1..=new_idx].to_vec()
    }
}

/// Emits the full ladder from `old` to `new` (see [`lifecycle_ladder`]), one
/// step at a time, to both the realm's own `UpdateScheduler` and its
/// `WidgetsBinding` observers — mirroring Flutter's single platform-message
/// stream driving both `SchedulerBinding` and `WidgetsBinding` from the same
/// synthesized sequence of states.
///
/// Installed as a direct call in the same `PlatformToUi` handler (never an
/// `UpdateScheduler`-listener closure): a listener captured at bootstrap time
/// would have to resolve `realm`/`WidgetsBinding` lazily at fire time,
/// which is unsound here specifically because every production caller of
/// this function runs from inside `dispatch_platform_realm`'s dispatch
/// window — the window during which the realm is taken OUT of
/// `APP_RUNTIME` and only restored once the dispatched task returns. A
/// listener resolving `APP_RUNTIME` at fire time would see `None` on every
/// real transition and silently no-op (this shipped once and was caught by
/// `frames_reenable_redirties_root_when_dispatched_through_the_realm_queue`
/// in `realm_dispatch_tests`, which reproduces via a real dispatched
/// `PlatformToUi::Lifecycle` sequence rather than driving `UpdateScheduler`
/// directly). `realm` is already in scope here (`PlatformToUi::run`'s
/// parameter), so no such resolution is ever needed — the frames-reenable
/// redirty below reads and writes it directly, in the same stack frame
/// that owns it for the whole call.
#[cfg(not(target_os = "ios"))]
pub(super) fn emit_lifecycle_transition(
    realm: &crate::app::ui_realm::UiRealm,
    old: AppLifecycleState,
    new: AppLifecycleState,
) {
    let mut first_panic = None;
    for step in lifecycle_ladder(old, new) {
        let presentation_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            realm.handle_presentation_lifecycle(step);
        }))
        .err();
        preserve_first_lifecycle_panic(
            &mut first_panic,
            presentation_panic,
            "presentation lifecycle transition",
        );

        let gesture_cleanup_panic = if matches!(
            step,
            AppLifecycleState::Hidden | AppLifecycleState::Paused | AppLifecycleState::Detached
        ) {
            // A hidden or suspended platform is not required to send the Up
            // or Cancel matching an in-flight Down. Drain this realm's input
            // transaction before lifecycle observers can retain stale gesture
            // state into the next visible frame.
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                realm.gestures().handle_lifecycle_pause();
            }))
            .err()
        } else {
            None
        };
        preserve_first_lifecycle_panic(&mut first_panic, gesture_cleanup_panic, "gesture cleanup");

        // Frames-disabled->enabled re-dirty: FLUI has no retained-scene
        // re-present, so an app that was `Hidden`/`Paused`/`Detached` and
        // comes back to `Resumed`/`Inactive` needs the root explicitly
        // re-dirtied, or the next frame finds nothing dirty and silently
        // stays Idle instead of repainting the stale window. Read
        // `frames_enabled()` immediately before and after the scheduler
        // call below so this observes exactly the edge THIS step produced,
        // whichever named state it is — `handle_app_lifecycle_state_change`
        // flips the flag via one atomic swap per call, so bracketing a
        // single call this way cannot miss or double-count an edge.
        let frames_were_enabled = realm.scheduler().frames_enabled();

        let scheduler_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            realm.scheduler().handle_app_lifecycle_state_change(step);
        }))
        .err();
        preserve_first_lifecycle_panic(
            &mut first_panic,
            scheduler_panic,
            "scheduler lifecycle dispatch",
        );

        if !frames_were_enabled && realm.scheduler().frames_enabled() {
            let redirty_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                realm.redirty_root_for_frames_reenable();
                realm.wake_frame();
            }))
            .err();
            preserve_first_lifecycle_panic(
                &mut first_panic,
                redirty_panic,
                "frames-reenable redirty",
            );
        }

        let widgets_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            realm.widgets().handle_app_lifecycle_state_changed(step);
        }))
        .err();
        preserve_first_lifecycle_panic(
            &mut first_panic,
            widgets_panic,
            "widgets lifecycle dispatch",
        );
    }

    // Every sink has now observed (or attempted) the complete synthesized
    // ladder. The earliest payload keeps transaction ordering deterministic.
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
}

#[cfg(not(target_os = "ios"))]
fn preserve_first_lifecycle_panic(
    first: &mut Option<Box<dyn std::any::Any + Send>>,
    candidate: Option<Box<dyn std::any::Any + Send>>,
    phase: &'static str,
) {
    let Some(candidate) = candidate else {
        return;
    };
    if first.is_none() {
        *first = Some(candidate);
    } else {
        tracing::error!(
            phase,
            "lifecycle phase panicked after an earlier phase; only the first panic is resumed"
        );
        // A secondary user panic may carry a payload whose destructor also
        // panics. Leaking that exceptional payload prevents it from replacing
        // the first lifecycle failure or aborting while the first unwinds.
        std::mem::forget(candidate);
    }
}

#[cfg(all(test, not(target_os = "ios")))]
mod lifecycle_derivation_tests {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    use flui_interaction::{
        HitTestEntry, HitTestResult, InteractionLane, RenderId,
        events::{PointerType, make_down_event, make_move_event},
    };
    use flui_types::geometry::{Offset, Pixels};
    use flui_view::WidgetsBindingObserver;

    use super::{
        AppLifecycleState, derive_lifecycle_state, emit_lifecycle_transition, lifecycle_ladder,
    };

    struct GestureStateObserver {
        cleanup_committed: Arc<AtomicBool>,
        hidden_saw_cleanup: AtomicBool,
    }

    impl WidgetsBindingObserver for GestureStateObserver {
        fn did_change_app_lifecycle_state(&self, state: AppLifecycleState) {
            if state == AppLifecycleState::Hidden {
                self.hidden_saw_cleanup.store(
                    self.cleanup_committed.load(Ordering::Acquire),
                    Ordering::Release,
                );
            }
        }
    }

    struct LifecycleSeen(Mutex<Vec<AppLifecycleState>>);

    impl WidgetsBindingObserver for LifecycleSeen {
        fn did_change_app_lifecycle_state(&self, state: AppLifecycleState) {
            self.0.lock().expect("lifecycle log lock").push(state);
        }
    }

    struct SetCleanupOnDrop(Arc<AtomicBool>);

    impl Drop for SetCleanupOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    struct PanicOnLifecycleRouteDrop;

    impl Drop for PanicOnLifecycleRouteDrop {
        fn drop(&mut self) {
            panic!("lifecycle route cleanup panic");
        }
    }

    struct PanickingLifecycleObserver(Arc<AtomicBool>);

    impl WidgetsBindingObserver for PanickingLifecycleObserver {
        fn did_change_app_lifecycle_state(&self, state: AppLifecycleState) {
            if state == AppLifecycleState::Hidden {
                self.0.store(true, Ordering::Release);
                panic!("widgets lifecycle listener panic");
            }
        }
    }

    #[test]
    fn derivation_truth_table() {
        assert_eq!(
            derive_lifecycle_state(true, true),
            AppLifecycleState::Resumed
        );
        assert_eq!(
            derive_lifecycle_state(true, false),
            AppLifecycleState::Inactive
        );
        assert_eq!(
            derive_lifecycle_state(false, true),
            AppLifecycleState::Hidden,
            "not visible must win over focused — a hidden window cannot be Resumed"
        );
        assert_eq!(
            derive_lifecycle_state(false, false),
            AppLifecycleState::Hidden
        );
    }

    /// Occlusion-before-focus-loss and focus-loss-before-occlusion must
    /// converge to the same derived state — the derivation depends only on
    /// the final `(visible, focused)` pair, never on update order.
    /// Mirrors `AppRuntime`'s actual update pattern (mutate one signal,
    /// re-derive) so this test exercises real ordering, not just two calls
    /// to a pure function with identical arguments.
    struct WindowSignals {
        visible: bool,
        focused: bool,
    }

    impl WindowSignals {
        fn new() -> Self {
            Self {
                visible: true,
                focused: true,
            }
        }

        fn set_visible(&mut self, visible: bool) -> AppLifecycleState {
            self.visible = visible;
            derive_lifecycle_state(self.visible, self.focused)
        }

        fn set_focused(&mut self, focused: bool) -> AppLifecycleState {
            self.focused = focused;
            derive_lifecycle_state(self.visible, self.focused)
        }
    }

    #[test]
    fn derivation_is_order_insensitive() {
        // Occlusion before focus loss.
        let mut occlusion_first = WindowSignals::new();
        let _after_occlusion = occlusion_first.set_visible(false);
        let occlusion_then_focus_loss = occlusion_first.set_focused(false);

        // The same two updates, reverse order: focus loss before occlusion.
        let mut focus_loss_first = WindowSignals::new();
        let _after_focus_loss = focus_loss_first.set_focused(false);
        let focus_loss_then_occlusion = focus_loss_first.set_visible(false);

        assert_eq!(
            occlusion_then_focus_loss, focus_loss_then_occlusion,
            "both orderings of the same two updates must land on the same derived state"
        );
        assert_eq!(occlusion_then_focus_loss, AppLifecycleState::Hidden);
    }

    #[test]
    fn ladder_is_empty_for_an_unchanged_state() {
        assert!(
            lifecycle_ladder(AppLifecycleState::Resumed, AppLifecycleState::Resumed).is_empty(),
            "a no-op transition must emit nothing — this is where change-detection for the \
             whole re-derivation lives (neither the scheduler nor WidgetsBinding observers see \
             a same-state call)"
        );
        assert!(lifecycle_ladder(AppLifecycleState::Hidden, AppLifecycleState::Hidden).is_empty());
    }

    /// Pause's ladder: Resumed -> Paused must visit Inactive, then Hidden,
    /// then Paused, in that order.
    #[test]
    fn ladder_steps_forward_through_every_intermediate_state_in_order() {
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Resumed, AppLifecycleState::Paused),
            vec![
                AppLifecycleState::Inactive,
                AppLifecycleState::Hidden,
                AppLifecycleState::Paused,
            ]
        );
    }

    /// Resume's ladder: the exact reverse of Pause's.
    #[test]
    fn ladder_steps_backward_through_every_intermediate_state_in_order() {
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Paused, AppLifecycleState::Resumed),
            vec![
                AppLifecycleState::Hidden,
                AppLifecycleState::Inactive,
                AppLifecycleState::Resumed,
            ]
        );
    }

    #[test]
    fn ladder_single_step_transitions_emit_exactly_that_step() {
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Resumed, AppLifecycleState::Inactive),
            vec![AppLifecycleState::Inactive]
        );
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Inactive, AppLifecycleState::Resumed),
            vec![AppLifecycleState::Resumed]
        );
    }

    /// Regression: `Detached` sits FIRST in Flutter's real `AppLifecycleState`
    /// order (`AppLifecycleState::ALL`: `Detached, Resumed, Inactive, Hidden,
    /// Paused` — the engine's "before initialization" state), not last. A
    /// transition FROM `Detached` is therefore a single forward step to
    /// whatever `new` is, never a crawl through every OTHER state first — the
    /// oracle's dedicated `state == detached` branch only fires when
    /// `Detached` is the TARGET, not the source.
    ///
    /// Reachable via Android's Pause/Resume reroute if `UpdateScheduler::
    /// lifecycle_state()`'s corrupt-byte fallback (`try_from_u8`'s
    /// `unwrap_or(AppLifecycleState::Detached)`) is ever hit as "old".
    #[test]
    fn ladder_from_detached_is_a_single_forward_step() {
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Detached, AppLifecycleState::Resumed),
            vec![AppLifecycleState::Resumed],
            "Detached -> Resumed must NOT synthesize Paused/Hidden/Inactive first"
        );
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Detached, AppLifecycleState::Inactive),
            vec![AppLifecycleState::Resumed, AppLifecycleState::Inactive]
        );
    }

    /// `Detached` as the TARGET is the oracle's special case: walk every
    /// remaining state after `old`, in order, then append `Detached` itself.
    #[test]
    fn ladder_to_detached_walks_every_remaining_state_then_appends_detached() {
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Resumed, AppLifecycleState::Detached),
            vec![
                AppLifecycleState::Inactive,
                AppLifecycleState::Hidden,
                AppLifecycleState::Paused,
                AppLifecycleState::Detached,
            ]
        );
        assert_eq!(
            lifecycle_ladder(AppLifecycleState::Hidden, AppLifecycleState::Detached),
            vec![AppLifecycleState::Paused, AppLifecycleState::Detached]
        );
    }

    #[test]
    fn hidden_transition_drains_the_realms_interrupted_pointer_sequence() {
        let realm = crate::app::ui_realm::UiRealm::for_test();
        let lane = InteractionLane::try_new().expect("test interaction lane");
        let handle = lane.dispatch_handle();
        let cleanup_committed = Arc::new(AtomicBool::new(false));
        let observer = Arc::new(GestureStateObserver {
            cleanup_committed: Arc::clone(&cleanup_committed),
            hidden_saw_cleanup: AtomicBool::new(false),
        });
        let observer_handle: Arc<dyn WidgetsBindingObserver> = observer.clone();
        realm.widgets().add_observer(observer_handle.clone());

        realm.enter(|realm| {
            lane.enter(|| {
                realm
                    .gestures()
                    .set_resampling_enabled(true)
                    .expect("test realm has no active pointer before configuration");
                let owner = SetCleanupOnDrop(Arc::clone(&cleanup_committed));
                let target = handle
                    .register_pointer(move |_| {
                        let _keep_owner_alive = &owner;
                    })
                    .expect("register lifecycle target");
                let mut result = HitTestResult::new();
                result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
                let down =
                    make_down_event(Offset::new(Pixels(8.0), Pixels(13.0)), PointerType::Touch);
                realm.gestures().handle_pointer_event(&down, |_| result);
                let move_event =
                    make_move_event(Offset::new(Pixels(9.0), Pixels(14.0)), PointerType::Touch);
                realm
                    .gestures()
                    .handle_pointer_event(&move_event, |_| HitTestResult::new());
                handle
                    .unregister_pointer(target)
                    .expect("cached route retains lifecycle target");
                assert_eq!(realm.gestures().active_pointer_count(), 1);
                assert_eq!(realm.gestures().active_resampler_count(), 1);
                assert_eq!(realm.gestures().pending_move_count(), 1);

                emit_lifecycle_transition(
                    realm,
                    AppLifecycleState::Resumed,
                    AppLifecycleState::Hidden,
                );

                assert_eq!(realm.gestures().active_pointer_count(), 0);
                assert_eq!(realm.gestures().active_resampler_count(), 0);
                assert_eq!(realm.gestures().pending_move_count(), 0);
                assert!(realm.gestures().arena().is_empty());
                assert!(
                    observer.hidden_saw_cleanup.load(Ordering::Acquire),
                    "lifecycle observers must see gesture teardown already committed"
                );

                // Restore Resumed before this test-local realm drops -- tidy,
                // not required for isolation (each realm owns its own
                // scheduler now, so there is nothing left to leak between
                // tests).
                emit_lifecycle_transition(
                    realm,
                    AppLifecycleState::Hidden,
                    AppLifecycleState::Resumed,
                );
            });
        });
        realm.widgets().remove_observer(&observer_handle);
    }

    #[test]
    fn multi_step_lifecycle_commits_the_target_before_the_first_panic_resumes() {
        let realm = crate::app::ui_realm::UiRealm::for_test();
        let lane = InteractionLane::try_new().expect("test interaction lane");
        let handle = lane.dispatch_handle();
        let observer = Arc::new(LifecycleSeen(Mutex::new(Vec::new())));
        let observer_handle: Arc<dyn WidgetsBindingObserver> = observer.clone();
        realm.widgets().add_observer(observer_handle.clone());
        let scheduler_listener_panicked = Arc::new(AtomicBool::new(false));
        let scheduler_probe = Arc::clone(&scheduler_listener_panicked);
        let scheduler_listener =
            realm
                .scheduler()
                .add_lifecycle_state_listener(Arc::new(move |state| {
                    if state == AppLifecycleState::Hidden {
                        scheduler_probe.store(true, Ordering::Release);
                        panic!("scheduler lifecycle listener panic");
                    }
                }));
        let widget_listener_panicked = Arc::new(AtomicBool::new(false));
        let panicking_observer: Arc<dyn WidgetsBindingObserver> = Arc::new(
            PanickingLifecycleObserver(Arc::clone(&widget_listener_panicked)),
        );
        realm.widgets().add_observer(panicking_observer.clone());

        realm.enter(|realm| {
            lane.enter(|| {
                let owner = PanicOnLifecycleRouteDrop;
                let target = handle
                    .register_pointer(move |_| {
                        let _keep_owner_alive = &owner;
                    })
                    .expect("register lifecycle target");
                let mut result = HitTestResult::new();
                result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
                let down =
                    make_down_event(Offset::new(Pixels(3.0), Pixels(5.0)), PointerType::Touch);
                realm.gestures().handle_pointer_event(&down, |_| result);
                handle
                    .unregister_pointer(target)
                    .expect("cached route retains lifecycle target");

                let unwind = catch_unwind(AssertUnwindSafe(|| {
                    emit_lifecycle_transition(
                        realm,
                        AppLifecycleState::Resumed,
                        AppLifecycleState::Paused,
                    );
                }));
                let payload = unwind.expect_err("route cleanup panic must propagate");

                assert_eq!(
                    payload.downcast_ref::<&str>(),
                    Some(&"lifecycle route cleanup panic")
                );
                assert_eq!(
                    *observer.0.lock().expect("lifecycle log lock"),
                    vec![
                        AppLifecycleState::Inactive,
                        AppLifecycleState::Hidden,
                        AppLifecycleState::Paused,
                    ],
                    "the complete synthesized ladder must reach widget observers"
                );
                assert_eq!(realm.gestures().active_pointer_count(), 0);
                assert_eq!(
                    realm.scheduler().lifecycle_state(),
                    AppLifecycleState::Paused,
                    "the target state must commit before the first panic resumes"
                );
                assert!(
                    scheduler_listener_panicked.load(Ordering::Acquire),
                    "scheduler lifecycle sink must run after cleanup"
                );
                assert!(
                    widget_listener_panicked.load(Ordering::Acquire),
                    "widgets lifecycle sink must run after a scheduler listener panic"
                );

                assert!(
                    realm
                        .scheduler()
                        .remove_lifecycle_state_listener(scheduler_listener),
                    "test scheduler listener must be removable"
                );
                realm.widgets().remove_observer(&panicking_observer);
                realm.widgets().remove_observer(&observer_handle);
                emit_lifecycle_transition(
                    realm,
                    AppLifecycleState::Paused,
                    AppLifecycleState::Resumed,
                );
            });
        });
    }
}
