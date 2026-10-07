//! The application lifecycle a presentation observes: derived from its
//! window's execution state and the host's lifecycle, and delivered as the
//! ordered ladder of states between the old and the new one.

use flui_scheduler::AppLifecycleState;

// ============================================================================
// Lifecycle derivation and ladder synthesis (see ADR-0035)
// ============================================================================

/// Derives the [`AppLifecycleState`] from the two window
/// signals FLUI tracks per window: visibility (occlusion) and focus.
///
/// Pure and order-insensitive: the result depends only on the final
/// `(visible, focused)` pair, never on which of the two changed most
/// recently — occlusion-before-focus-loss and focus-loss-before-occlusion
/// converge to the same derived state once both signals have landed.
pub(crate) fn derive_lifecycle_state(visible: bool, focused: bool) -> AppLifecycleState {
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
/// The walk is over [`AppLifecycleState::ALL`]'s order — NOT over this
/// enum's own `#[repr(u8)]` discriminants, which exist for FLUI's
/// `frames_enabled` derivation and do not match the ladder order. `ALL`
/// lists `Detached` **first** (it is the state a window starts in *before*
/// initialization, not a terminal "highest" state).
///
/// Three cases:
/// - **Target is `Detached`**: walk forward from `old` to the end of `ALL`
///   (through every remaining non-detached state), then append `Detached`
///   itself — going to `Detached` always visits every state after `old`,
///   regardless of where `old` sits.
/// - **Going backward** (`old`'s index > `new`'s index, e.g. `Paused` ->
///   `Resumed`): the intermediate states in *descending* index order,
///   ending at `new`.
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
pub(crate) fn lifecycle_ladder(
    old: AppLifecycleState,
    new: AppLifecycleState,
) -> Vec<AppLifecycleState> {
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

/// Keep the first panic of a multi-step lifecycle teardown: a later
/// `candidate` payload is leaked rather than dropped (its destructor could
/// panic again) and only logged, under `phase`, so the caller resumes the
/// first failure once every step has run.
pub fn preserve_first_lifecycle_panic(
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
        // Arbitrary payload destruction or diagnostics must not replace the first failure.
        std::mem::forget(candidate);
        if let Err(payload) = std::panic::catch_unwind(|| {
            tracing::error!(
                phase,
                "lifecycle phase panicked after an earlier phase; only the first panic is resumed"
            );
        }) {
            std::mem::forget(payload);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod lifecycle_derivation_tests {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    use flui_foundation::geometry::Offset;
    use flui_interaction::{
        HitTestEntry, HitTestResult, InteractionLane, RenderId,
        events::{PointerType, make_down_event},
    };
    use flui_view::WidgetsBindingObserver;

    use super::{AppLifecycleState, derive_lifecycle_state, lifecycle_ladder};

    struct LifecycleSeen(Mutex<Vec<AppLifecycleState>>);

    impl WidgetsBindingObserver for LifecycleSeen {
        fn did_change_app_lifecycle_state(&self, state: AppLifecycleState) {
            self.0.lock().expect("lifecycle log lock").push(state);
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

    /// Pause's ladder: Resumed -> Paused must visit Inactive, then Hidden,
    /// then Paused, in that order.
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

    fn multi_step_lifecycle_commits_the_target_before_the_first_panic_resumes() {
        let ui_runtime = crate::ui_runtime::UiRuntime::for_test();
        ui_runtime.synchronize_window_lifecycle();
        let lane = InteractionLane::try_new().expect("test interaction lane");
        let handle = lane.dispatch_handle();
        let observer = Arc::new(LifecycleSeen(Mutex::new(Vec::new())));
        let observer_handle: Arc<dyn WidgetsBindingObserver> = observer.clone();
        ui_runtime.widgets().add_observer(observer_handle.clone());
        let scheduler_listener_panicked = Arc::new(AtomicBool::new(false));
        let scheduler_probe = Arc::clone(&scheduler_listener_panicked);
        let scheduler_listener = ui_runtime
            .scheduler()
            .add_lifecycle_state_listener(Arc::new(move |state| {
                if state == AppLifecycleState::Paused {
                    scheduler_probe.store(true, Ordering::Release);
                    panic!("scheduler lifecycle listener panic");
                }
            }));
        let widget_listener_panicked = Arc::new(AtomicBool::new(false));
        let panicking_observer: Arc<dyn WidgetsBindingObserver> = Arc::new(
            PanickingLifecycleObserver(Arc::clone(&widget_listener_panicked)),
        );
        ui_runtime
            .widgets()
            .add_observer(panicking_observer.clone());

        ui_runtime.enter(|ui_runtime| {
            lane.enter(|| {
                let owner = PanicOnLifecycleRouteDrop;
                let target = handle
                    .register_pointer(move |_| {
                        let _keep_owner_alive = &owner;
                    })
                    .expect("register lifecycle target");
                let mut result = HitTestResult::new();
                result.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
                let down = make_down_event(Offset::new(3.0, 5.0), PointerType::Touch);
                ui_runtime
                    .gestures()
                    .handle_pointer_event(&down, |_| result);
                handle
                    .unregister_pointer(target)
                    .expect("cached route retains lifecycle target");

                let unwind = catch_unwind(AssertUnwindSafe(|| {
                    ui_runtime.update_host_lifecycle(AppLifecycleState::Paused);
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
                assert_eq!(ui_runtime.gestures().active_pointer_count(), 0);
                assert_eq!(
                    ui_runtime.scheduler().lifecycle_state(),
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
                    ui_runtime
                        .scheduler()
                        .remove_lifecycle_state_listener(scheduler_listener),
                    "test scheduler listener must be removable"
                );
                ui_runtime.widgets().remove_observer(&panicking_observer);
                ui_runtime.widgets().remove_observer(&observer_handle);
                ui_runtime.update_host_lifecycle(AppLifecycleState::Resumed);
            });
        });
    }

    #[test]
    fn lifecycle_derivation_matrix() {
        crate::table_test::run_table(
            "lifecycle_derivation_matrix",
            &[
                ("derivation_truth_table", derivation_truth_table as fn()),
                (
                    "ladder_steps_forward_through_every_intermediate_state_in_order",
                    ladder_steps_forward_through_every_intermediate_state_in_order as fn(),
                ),
                (
                    "multi_step_lifecycle_commits_the_target_before_the_first_panic_resumes",
                    multi_step_lifecycle_commits_the_target_before_the_first_panic_resumes
                        as fn(),
                ),
            ],
        );
    }
}
