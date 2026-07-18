//! Application runner - entry points for running FLUI apps.
//!
//! This module provides platform-agnostic entry points that delegate
//! to platform-specific implementations via flui-platform.

use flui_view::{StatelessView, View};

use super::{AppBinding, AppConfig};

#[cfg(not(target_os = "ios"))]
use flui_foundation::HasInstance;
#[cfg(not(target_os = "ios"))]
use flui_scheduler::{AppLifecycleState, Scheduler};

/// Run a FLUI application with default configuration.
///
/// This is the internal implementation called by `run_app()`.
pub fn run_app_impl<V>(root: V)
where
    V: View + StatelessView + Clone + 'static,
{
    run_app_with_config_impl(root, AppConfig::default());
}

/// Run a FLUI application with custom configuration.
///
/// This is the internal implementation called by `run_app_with_config()`.
pub fn run_app_with_config_impl<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    // Initialize logging
    init_logging();

    // `target_fps` is logged as advisory, not enforced: the desktop runner's
    // steady-state pacing comes from the GPU-side blocking Fifo present
    // (`flui-engine::wgpu::Renderer::render_scene`), not from this value —
    // see `AppConfig::target_fps`'s doc for the full consumer audit.
    tracing::info!(
        title = %config.title,
        size = ?config.size,
        target_fps_advisory = config.target_fps,
        "Starting FLUI application"
    );

    // Run platform-specific event loop
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    {
        run_desktop(root, config);
    }

    #[cfg(target_os = "android")]
    {
        let _ = (root, config);
        panic!(
            "On Android, use flui_app::run_app_android() from android_main() \
             instead of run_app(). AndroidApp must be provided by the system."
        );
    }

    #[cfg(target_os = "ios")]
    {
        run_ios(config);
    }

    #[cfg(target_arch = "wasm32")]
    {
        run_web(root, config);
    }
}

/// Initialize logging based on environment.
fn init_logging() {
    // Use flui_foundation::log for cross-platform logging (desktop, Android, iOS, WASM).
    // Module was merged from the standalone flui-log crate.
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| {
        "info,flui_app=debug,flui_view=debug,flui_rendering=debug,wgpu=warn".to_string()
    });

    flui_foundation::log::Logger::new()
        .with_filter(&filter)
        // TRACE ceiling: the per-target filter (RUST_LOG / the default
        // string above) decides what's emitted — a DEBUG ceiling here
        // silently made every trace! unreachable no matter what the
        // user put in RUST_LOG.
        .with_level(flui_foundation::log::Level::TRACE)
        .init();
}

// ============================================================================
// Platform-neutral owner-thread realm host (ADR-0027)
// ============================================================================

#[cfg(not(target_os = "ios"))]
thread_local! {
    /// Transitional owner-thread host shared by desktop, Android, and wasm.
    /// The platform callback surface still requires `Send`, so the `!Send`
    /// realm remains in owner TLS until that seam is retired. Access is only
    /// through the stamped FIFO dispatcher below.
    static PLATFORM_REALM_HOST: std::cell::RefCell<RealmHost> =
        const { std::cell::RefCell::new(RealmHost::new()) };
}

#[cfg(not(target_os = "ios"))]
struct RealmHost {
    realm: Option<super::ui_realm::UiRealm>,
    queue: std::collections::VecDeque<RealmEvent>,
    draining: bool,
    owner_thread: Option<std::thread::ThreadId>,
    realm_id: Option<flui_foundation::RealmId>,

    /// Single-window `(visible, focused)` tracking for the
    /// `AppLifecycleState` derivation (ADR-0035 PR2) — `RealmEvent::
    /// WindowFocus`/`WindowVisibility` each update one half of this pair and
    /// re-derive. Both default `true`: a window is assumed visible and
    /// focused until a platform callback says otherwise (matches every
    /// backend's actual startup state).
    visible: bool,
    focused: bool,
}

#[cfg(not(target_os = "ios"))]
impl RealmHost {
    const fn new() -> Self {
        Self {
            realm: None,
            queue: std::collections::VecDeque::new(),
            draining: false,
            owner_thread: None,
            realm_id: None,
            visible: true,
            focused: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg(not(target_os = "ios"))]
struct RealmDispatcher {
    owner_thread: std::thread::ThreadId,
    realm_id: flui_foundation::RealmId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(not(target_os = "ios"))]
enum RealmDispatchError {
    WrongThread,
    StaleRealm,
    RealmUnavailable,
}

#[cfg(not(target_os = "ios"))]
enum RealmEvent {
    Input(flui_platform::traits::PlatformInput),
    Resize {
        size: flui_types::Size<flui_types::geometry::Pixels>,
        scale_factor: f32,
        apply_surface: Box<dyn FnOnce()>,
    },
    /// Window focus changed (winit's `WindowEvent::Focused`, or the
    /// equivalent per-backend signal).
    ///
    /// PR1 (ADR-0035) named this `Active` and left it a deliberate no-op.
    /// PR2 renames it (same source, no transport change) and feeds it into
    /// the `(visible, focused)` -> `AppLifecycleState` derivation below,
    /// alongside [`WindowVisibility`](Self::WindowVisibility).
    WindowFocus(bool),
    /// Window visibility/occlusion changed (winit's `WindowEvent::Occluded`,
    /// negated — see `PlatformWindow::on_visibility_status_change`).
    ///
    /// New in PR2. Combined with [`WindowFocus`](Self::WindowFocus) via
    /// [`derive_lifecycle_state`] to produce the `AppLifecycleState` the
    /// ladder in [`emit_lifecycle_transition`] steps toward.
    // Not yet constructed on wasm32: `run_web` only wires `WindowFocus` in
    // this PR — no occlusion signal for the web backend yet (see run_web's
    // comment at its `on_active_status_change` registration).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    WindowVisibility(bool),
    Frame(Box<dyn FnOnce(&super::ui_realm::UiRealm)>),
}

#[cfg(not(target_os = "ios"))]
impl RealmEvent {
    fn run(self, realm: &super::ui_realm::UiRealm) {
        match self {
            Self::Input(input) => AppBinding::instance().handle_input(input),
            Self::Resize {
                size,
                scale_factor,
                apply_surface,
            } => {
                apply_surface();
                AppBinding::instance()
                    .render_pipeline_mut()
                    .set_device_pixel_ratio(scale_factor);
                AppBinding::instance().request_redraw();
                tracing::trace!(?size, scale_factor, "realm resize committed");
            }
            Self::WindowFocus(focused) => {
                let (old, new) = PLATFORM_REALM_HOST.with(|slot| {
                    let mut state = slot.borrow_mut();
                    let old = derive_lifecycle_state(state.visible, state.focused);
                    state.focused = focused;
                    (old, derive_lifecycle_state(state.visible, state.focused))
                });
                emit_lifecycle_transition(realm, old, new);
            }
            Self::WindowVisibility(visible) => {
                let (old, new) = PLATFORM_REALM_HOST.with(|slot| {
                    let mut state = slot.borrow_mut();
                    let old = derive_lifecycle_state(state.visible, state.focused);
                    state.visible = visible;
                    (old, derive_lifecycle_state(state.visible, state.focused))
                });
                emit_lifecycle_transition(realm, old, new);
            }
            Self::Frame(run) => run(realm),
        }
    }
}

// ============================================================================
// Lifecycle derivation and ladder synthesis (ADR-0035 PR2)
// ============================================================================

/// Derives the Flutter-parity [`AppLifecycleState`] from the two window
/// signals FLUI tracks per window: visibility (occlusion) and focus.
///
/// Pure and order-insensitive: the result depends only on the final
/// `(visible, focused)` pair, never on which of the two changed most
/// recently — occlusion-before-focus-loss and focus-loss-before-occlusion
/// converge to the same derived state once both signals have landed.
#[cfg(not(target_os = "ios"))]
fn derive_lifecycle_state(visible: bool, focused: bool) -> AppLifecycleState {
    if !visible {
        AppLifecycleState::Hidden
    } else if focused {
        AppLifecycleState::Resumed
    } else {
        AppLifecycleState::Inactive
    }
}

/// The intermediate `AppLifecycleState` steps between `old` and `new`,
/// inclusive of `new`, exclusive of `old` — Flutter's `AppLifecycleState`
/// ladder order (`Resumed < Inactive < Hidden < Paused < Detached`, exactly
/// the enum's own discriminant order). Oracle: `platform_dispatcher.dart`
/// (Flutter 3.44.0) synthesizes every skipped state so no observer misses a
/// step when a transition jumps straight from `Resumed` to `Hidden` without
/// visiting `Inactive`.
///
/// Returns an empty `Vec` when `old == new` — this is where change-detection
/// for the whole re-derivation lives: a wake that doesn't change the derived
/// state emits nothing, to neither the scheduler nor `WidgetsBinding`
/// observers.
#[cfg(not(target_os = "ios"))]
fn lifecycle_ladder(old: AppLifecycleState, new: AppLifecycleState) -> Vec<AppLifecycleState> {
    let (old_ord, new_ord) = (old as u8, new as u8);
    match old_ord.cmp(&new_ord) {
        std::cmp::Ordering::Equal => Vec::new(),
        std::cmp::Ordering::Less => (old_ord + 1..=new_ord)
            .filter_map(AppLifecycleState::try_from_u8)
            .collect(),
        std::cmp::Ordering::Greater => (new_ord..old_ord)
            .rev()
            .filter_map(AppLifecycleState::try_from_u8)
            .collect(),
    }
}

/// Emits the full ladder from `old` to `new` (see [`lifecycle_ladder`]), one
/// step at a time, to both the canonical `Scheduler` and the realm's
/// `WidgetsBinding` observers — mirroring Flutter's single platform-message
/// stream driving both `SchedulerBinding` and `WidgetsBinding` from the same
/// synthesized sequence of states.
///
/// Installed as a direct call in the same `RealmEvent` handler (never a
/// `Scheduler`-listener closure): a listener captured at bootstrap time would
/// have to resolve `realm`/`WidgetsBinding` lazily at fire time, which is
/// exactly the thread-local-resolution/Send-capture trap
/// `AppBinding::instance()`'s own installer avoids elsewhere in this crate.
/// `realm` is already in scope here (`RealmEvent::run`'s parameter), so no
/// such resolution is needed.
#[cfg(not(target_os = "ios"))]
fn emit_lifecycle_transition(
    realm: &super::ui_realm::UiRealm,
    old: AppLifecycleState,
    new: AppLifecycleState,
) {
    for step in lifecycle_ladder(old, new) {
        Scheduler::instance().handle_app_lifecycle_state_change(step);
        realm.widgets().handle_app_lifecycle_state_changed(step);
    }
}

#[cfg(all(test, not(target_os = "ios")))]
mod lifecycle_derivation_tests {
    use super::{AppLifecycleState, derive_lifecycle_state, lifecycle_ladder};

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
    /// Mirrors `RealmHost`'s actual update pattern (mutate one signal,
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
}

#[cfg(not(target_os = "ios"))]
fn install_platform_realm(realm: super::ui_realm::UiRealm) -> RealmDispatcher {
    let owner_thread = std::thread::current().id();
    let realm_id = realm.realm_id();
    PLATFORM_REALM_HOST.with(|slot| {
        let mut state = slot.borrow_mut();
        state.realm = Some(realm);
        state.owner_thread = Some(owner_thread);
        state.realm_id = Some(realm_id);
    });
    RealmDispatcher {
        owner_thread,
        realm_id,
    }
}

#[cfg(not(target_os = "ios"))]
fn dispatch_platform_realm(
    dispatcher: RealmDispatcher,
    event: RealmEvent,
) -> Result<(), RealmDispatchError> {
    if std::thread::current().id() != dispatcher.owner_thread {
        tracing::error!(?dispatcher, "rejecting realm callback on non-owner thread");
        return Err(RealmDispatchError::WrongThread);
    }
    let realm = PLATFORM_REALM_HOST.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.realm_id != Some(dispatcher.realm_id) {
            return Err(if state.realm_id.is_some() {
                tracing::debug!(
                    ?dispatcher,
                    current_realm_id = ?state.realm_id,
                    "dropping realm callback: a newer realm replaced the one it was dispatched for"
                );
                RealmDispatchError::StaleRealm
            } else {
                tracing::debug!(
                    ?dispatcher,
                    "dropping realm callback: no realm installed (not yet ready, or already torn down)"
                );
                RealmDispatchError::RealmUnavailable
            });
        }
        state.queue.push_back(event);
        if state.draining || state.realm.is_none() {
            return Ok(None);
        }
        let first = state
            .queue
            .pop_front()
            .expect("BUG: event was enqueued before starting realm dispatch");
        state.draining = true;
        Ok(state.realm.take().map(|realm| (realm, first)))
    })?;
    let Some((realm, first)) = realm else {
        return Ok(());
    };

    // Never hold the TLS RefCell borrow across user/platform callbacks. Catch
    // only to restore the host invariants; the original panic is resumed.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut next = Some(first);
        while let Some(event) = next {
            realm.enter(|realm| event.run(realm));
            next = PLATFORM_REALM_HOST.with(|slot| slot.borrow_mut().queue.pop_front());
        }
    }));
    PLATFORM_REALM_HOST.with(|slot| {
        let mut state = slot.borrow_mut();
        state.realm = Some(realm);
        state.draining = false;
    });
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
    Ok(())
}

/// Drains the per-frame owner-inbox commands and reports whether the drain
/// itself asked for a redraw.
///
/// Every platform's frame callback must call this exactly once per wake, at
/// the Idle frame boundary — before the dirty gate, and before any
/// early-return fast path a platform's frame callback takes (e.g. Android's
/// hot-reload plugin scene) — never inside the frame transaction below.
/// Running it unconditionally on every wake is what keeps
/// `UiCommandSender`'s bounded inbox draining: a wake that skips the drain
/// lets the inbox fill until it hard-errors, and a coalesced redraw request
/// that nothing consumes never wakes the loop again (`take_redraw_request`
/// only flips back to `false` once observed here).
#[cfg(not(target_os = "ios"))]
fn drain_owner_inbox(realm: &super::ui_realm::UiRealm) -> bool {
    let report = realm.drain_commands();
    if report != super::ui_realm::DrainReport::default() {
        tracing::trace!(?report, "owner inbox drained");
    }
    realm.take_redraw_request()
}

#[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
fn teardown_platform_realm() {
    let (realm, queued) = PLATFORM_REALM_HOST.with(|slot| {
        let mut state = slot.borrow_mut();
        let realm = state.realm.take();
        let queued = std::mem::take(&mut state.queue);
        state.draining = false;
        state.owner_thread = None;
        state.realm_id = None;
        (realm, queued)
    });
    // Destructors may re-enter platform/framework code. Drop only after the
    // TLS borrow and incarnation identity have been released.
    drop(queued);
    drop(realm);

    // ADR-0034's install/teardown symmetry: the event loop has exited (this
    // runs from both `run_desktop` and `run_android`, after their respective
    // `platform.run(...)` returns), so drop the platform clipboard now rather
    // than let a live platform resource (arboard on X11 owns a live X11
    // connection) sit pinned behind `AppBinding` for the rest of the
    // process's life.
    AppBinding::instance().clear_platform_clipboard();
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn queued_hot_reload_hook(
    sender: super::ui_realm::UiCommandSender,
) -> impl Fn() + Send + Sync + 'static {
    move || {
        if let Err(error) = sender.request_hot_reload(flui_hot_reload::HotReloadTier::HotReload) {
            tracing::warn!(
                ?error,
                "ignoring hot-reload request for a dead or busy realm"
            );
        }
    }
}

#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod realm_dispatch_tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    fn install_test_realm() -> RealmDispatcher {
        let app = AppBinding::instance();
        install_platform_realm(super::super::ui_realm::UiRealm::for_test(app))
    }

    #[test]
    fn reentrant_frame_event_is_queued_fifo() {
        let dispatcher = install_test_realm();
        let order = Rc::new(RefCell::new(Vec::new()));
        let outer = Rc::clone(&order);
        dispatch_platform_realm(
            dispatcher,
            RealmEvent::Frame(Box::new(move |_| {
                outer.borrow_mut().push(1);
                let nested = Rc::clone(&outer);
                dispatch_platform_realm(
                    dispatcher,
                    RealmEvent::Frame(Box::new(move |_| {
                        nested.borrow_mut().push(3);
                    })),
                )
                .expect("nested event queues");
                outer.borrow_mut().push(2);
            })),
        )
        .expect("outer event dispatches");
        assert_eq!(*order.borrow(), vec![1, 2, 3]);
    }

    #[test]
    fn late_event_never_crosses_realm_incarnations() {
        let stale = install_test_realm();
        PLATFORM_REALM_HOST.with(|slot| {
            let mut state = slot.borrow_mut();
            let realm = state.realm.take();
            state.queue.clear();
            state.realm_id = None;
            drop(state);
            drop(realm);
        });
        assert_eq!(
            dispatch_platform_realm(stale, RealmEvent::Frame(Box::new(|_| {}))),
            Err(RealmDispatchError::RealmUnavailable)
        );

        let current = install_test_realm();
        assert_eq!(
            dispatch_platform_realm(stale, RealmEvent::Frame(Box::new(|_| {}))),
            Err(RealmDispatchError::StaleRealm)
        );
        dispatch_platform_realm(current, RealmEvent::Frame(Box::new(|_| {})))
            .expect("current incarnation dispatches");
    }

    #[test]
    fn panic_restores_dispatch_host_for_next_event() {
        let dispatcher = install_test_realm();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = dispatch_platform_realm(
                dispatcher,
                RealmEvent::Frame(Box::new(|_| panic!("test panic"))),
            );
        }));
        assert!(panic.is_err());

        let ran = Rc::new(RefCell::new(false));
        let ran_in_event = Rc::clone(&ran);
        dispatch_platform_realm(
            dispatcher,
            RealmEvent::Frame(Box::new(move |_| {
                *ran_in_event.borrow_mut() = true;
            })),
        )
        .expect("host restored");
        assert!(*ran.borrow());
    }

    #[test]
    fn callback_on_wrong_thread_is_rejected() {
        let dispatcher = install_test_realm();
        let result = std::thread::spawn(move || {
            dispatch_platform_realm(dispatcher, RealmEvent::Frame(Box::new(|_| {})))
        })
        .join()
        .expect("worker test thread");
        assert_eq!(result, Err(RealmDispatchError::WrongThread));
    }

    #[test]
    fn nested_resize_and_window_focus_wait_until_frame_returns() {
        let dispatcher = install_test_realm();
        let order = Rc::new(RefCell::new(Vec::new()));
        let outer = Rc::clone(&order);
        dispatch_platform_realm(
            dispatcher,
            RealmEvent::Frame(Box::new(move |_| {
                outer.borrow_mut().push(1);
                dispatch_platform_realm(dispatcher, RealmEvent::WindowFocus(true))
                    .expect("window focus queues");
                let resize = Rc::clone(&outer);
                dispatch_platform_realm(
                    dispatcher,
                    RealmEvent::Resize {
                        size: flui_types::Size::new(
                            flui_types::geometry::px(640.0),
                            flui_types::geometry::px(480.0),
                        ),
                        scale_factor: 2.0,
                        apply_surface: Box::new(move || resize.borrow_mut().push(3)),
                    },
                )
                .expect("resize queues");
                outer.borrow_mut().push(2);
            })),
        )
        .expect("frame dispatches");
        // Two different `RealmEvent` variants nested inside a `Frame` still
        // queue FIFO rather than running immediately — the property
        // `reentrant_frame_event_is_queued_fifo` proves for same-variant
        // nesting, this proves it holds across variant types too.
        assert_eq!(*order.borrow(), vec![1, 2, 3]);
    }

    #[test]
    fn teardown_drops_queued_destructors_outside_tls_borrow() {
        struct ReenterOnDrop {
            dispatcher: RealmDispatcher,
            dropped: Rc<RefCell<bool>>,
        }

        impl Drop for ReenterOnDrop {
            fn drop(&mut self) {
                let result =
                    dispatch_platform_realm(self.dispatcher, RealmEvent::Frame(Box::new(|_| {})));
                assert_eq!(result, Err(RealmDispatchError::RealmUnavailable));
                *self.dropped.borrow_mut() = true;
            }
        }

        let dispatcher = install_test_realm();
        let dropped = Rc::new(RefCell::new(false));
        let probe = ReenterOnDrop {
            dispatcher,
            dropped: Rc::clone(&dropped),
        };
        PLATFORM_REALM_HOST.with(|slot| {
            slot.borrow_mut()
                .queue
                .push_back(RealmEvent::Frame(Box::new(move |_| drop(probe))));
        });
        teardown_platform_realm();
        assert!(*dropped.borrow());
    }

    #[test]
    fn old_registered_hot_reload_hook_cannot_touch_recreated_realm() {
        use flui_hot_reload::{register_request_rebuild, request_rebuild};

        let runtime_a = super::super::ui_realm::UiRealm::for_test(AppBinding::instance());
        let sender_a = runtime_a.command_sender();
        let old_a_hook = queued_hot_reload_hook(sender_a.clone());
        let registration_a = register_request_rebuild(queued_hot_reload_hook(sender_a));
        let _realm_a = install_platform_realm(runtime_a);
        teardown_platform_realm();

        let runtime_b = super::super::ui_realm::UiRealm::for_test(AppBinding::instance());
        let sender_b = runtime_b.command_sender();
        let realm_b = install_platform_realm(runtime_b);
        let registration_b = register_request_rebuild(queued_hot_reload_hook(sender_b));
        drop(registration_a);

        old_a_hook();
        let after_old = Rc::new(RefCell::new(None));
        let after_old_in_frame = Rc::clone(&after_old);
        dispatch_platform_realm(
            realm_b,
            RealmEvent::Frame(Box::new(move |realm| {
                *after_old_in_frame.borrow_mut() = Some(realm.drain_commands());
            })),
        )
        .expect("B frame dispatches");
        assert_eq!(
            *after_old.borrow(),
            Some(super::super::ui_realm::DrainReport::default()),
            "stale A hook must not enqueue into B"
        );

        std::thread::spawn(request_rebuild)
            .join()
            .expect("worker-side rebuild request");
        let after_current = Rc::new(RefCell::new(None));
        let after_current_in_frame = Rc::clone(&after_current);
        dispatch_platform_realm(
            realm_b,
            RealmEvent::Frame(Box::new(move |realm| {
                *after_current_in_frame.borrow_mut() = Some(realm.drain_commands());
            })),
        )
        .expect("B frame dispatches");
        assert_eq!(
            after_current.borrow().as_ref().map(|report| report.invoked),
            Some(1),
            "current B hook must dispatch exactly once"
        );

        drop(registration_b);
        teardown_platform_realm();
    }

    #[test]
    fn whole_frame_event_keeps_realm_global_key_scope_active() {
        let app = AppBinding::instance();
        let realm = super::super::ui_realm::UiRealm::for_test(app);
        let key = flui_view::GlobalKey::<()>::new();
        let element = flui_foundation::ElementId::new(91);
        realm
            .widgets()
            .with_build_owner_mut(|owner| owner.register_global_key(key.id(), element));
        let dispatcher = install_platform_realm(realm);
        let key_after_frame = key.clone();

        assert_eq!(key.current_element(), None, "scope starts inactive");
        dispatch_platform_realm(
            dispatcher,
            RealmEvent::Frame(Box::new(move |_| {
                assert_eq!(key.current_element(), Some(element));
            })),
        )
        .expect("frame dispatches");
        assert_eq!(
            key_after_frame.current_element(),
            None,
            "frame scope is restored"
        );
        teardown_platform_realm();
    }
}

// ============================================================================
// Desktop frame-pacing gate (App.1 vsync pacing)
// ============================================================================
//
// Extracted as free functions — pure, no realm/window/GPU state — so the
// decisions each platform's frame callback makes each wake are unit
// testable without a live event loop. See the frame-pacing ADR for the
// full design: Fifo present blocks every PRESENTED frame at display
// cadence (the steady-state pacing); these functions cover what happens on
// the frames that path never blocks: a spurious wake with nothing to do or
// a backgrounded app (`wake_action`), and a frame that ran the pipeline but
// never reached `present()` (`no_present_fallback_pace`).

/// What a platform wake should do: run the full frame pipeline, pump only
/// the async driver, or nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WakeAction {
    /// Run the full frame pipeline — the pre-existing path, unchanged:
    /// frames are enabled and there is real work or a scheduled ticker.
    Render,
    /// Frames are disabled (`AppLifecycleState::Hidden`/`Paused`/
    /// `Detached`): poll only [`Scheduler::drive_async_tasks`] — never
    /// begin/draw a frame, tick, run the pipeline, or present. Dirty work
    /// is left untouched; it accumulates until frames re-enable.
    PumpAsync,
    /// A spurious wake while frames are enabled: nothing dirty, no
    /// scheduled ticker. No render, no pump, no sleep.
    Skip,
}

/// Decides what a platform wake should do, given the scheduler's
/// [`Scheduler::frames_enabled`] fact (ADR-0035) alongside the pre-existing
/// dirty/scheduled-ticker signals.
///
/// `frames_enabled == false` takes priority over everything else — even
/// with `dirty` work pending, a backgrounded app pumps only the async
/// driver; the dirty work is left alone (it accumulates untouched) rather
/// than running a full frame nobody can see. This is the ONLY thing that
/// keeps a spawned future progressing while the app is backgrounded: the
/// mid-frame `drive_async_tasks` poll inside `handle_begin_frame` never
/// runs in `PumpAsync` mode (no frame runs at all), so this explicit call
/// is the only pump.
///
/// `dirty` is true when there is real work (an inbox redraw request,
/// `needs_redraw`, or dirty pipeline nodes); `frame_scheduled` is true when
/// the global `Scheduler` has a pending ticker callback (a running
/// `AnimationController` with no other dirty state).
fn wake_action(frames_enabled: bool, dirty: bool, frame_scheduled: bool) -> WakeAction {
    if !frames_enabled {
        return WakeAction::PumpAsync;
    }
    if dirty || frame_scheduled {
        WakeAction::Render
    } else {
        WakeAction::Skip
    }
}

/// Whether another frame will be requested regardless of this one's
/// outcome: `needs_redraw`, a scheduled ticker, or dirty
/// pipeline/build work left over from the frame that just ran.
///
/// This only feeds [`no_present_fallback_pace`]'s THROTTLE decision below —
/// it cannot itself wake anything. A `ControlFlow::Wait` loop only wakes on
/// an actual `wake_frame()`/platform `request_redraw()` call or external
/// input; a dropped/errored frame's retry wake comes from
/// `render_frame_entered`'s `retry_needed` path, not from this function.
///
/// The pending-work leg matters when a frame that left dirty pipeline/build
/// nodes behind is ALSO being re-invoked by some other wake source without
/// ever reaching `present()`: without this leg, such a frame would read
/// `keeps_gate_open == false`, skip the fallback sleep, and the loop could
/// spin at full CPU speed re-processing the same leftover work on every
/// rapid re-wake instead of being bounded like any other no-present,
/// gate-open frame.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn keeps_frame_gate_open(
    needs_redraw: bool,
    frame_scheduled: bool,
    has_pending_work: bool,
) -> bool {
    needs_redraw || frame_scheduled || has_pending_work
}

/// Coarse fallback pace for a frame that ran the pipeline but never reached
/// `present()`, applied only while a ticker keeps re-requesting a frame.
///
/// This throttles; it does not pace. An un-presented frame carries no vsync
/// signal (Fifo's blocking present never engaged), so this is a fixed CPU-time
/// bound, not frame-accurate cadence — good enough to keep a repeating
/// controller behind a minimized/occluded window (or a `SurfaceLost` retry
/// loop) from busy-spinning at CPU speed (observed pre-fix: ~30 000 fps).
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
const NO_PRESENT_FALLBACK_PACE: std::time::Duration = std::time::Duration::from_millis(16);

/// Decides whether [`NO_PRESENT_FALLBACK_PACE`] applies this frame.
///
/// `presented` is `false` when `render_frame_entered`'s scene never reached
/// `present()` — no damage, an occluded surface, or a lost surface.
/// `keeps_gate_open` is `true` when another frame will be requested
/// regardless (`AppBinding::needs_redraw` or the scheduler still has a
/// ticker scheduled). The fallback is needed only when both hold: no vsync
/// block happened AND something is about to wake this loop again anyway —
/// that combination is the only busy-spin risk left once the fixed
/// frame-budget sleep is gone. A presented frame needs no fallback (Fifo
/// already paced it); an un-presented frame with nothing re-requesting a
/// wake needs no fallback either (the loop just goes idle).
///
/// Occlusion semantics differ by platform: on Wayland, frame callbacks stop
/// while a window is hidden, so no redraws arrive and tickers freeze (this
/// fallback never fires); on Windows/X11, redraw requests keep arriving for a
/// minimized window and this fallback bounds them. Timeout-shaped animations
/// (e.g. the snack-bar auto-dismiss controller) do not progress while frozen —
/// a future platform Timer service is the correctness seam for those.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn no_present_fallback_pace(presented: bool, keeps_gate_open: bool) -> Option<std::time::Duration> {
    (!presented && keeps_gate_open).then_some(NO_PRESENT_FALLBACK_PACE)
}

/// App.1 vsync-pacing gate tests.
///
/// `run_desktop` itself opens a real window and GPU device, so it cannot
/// run headlessly; `wake_action` and `no_present_fallback_pace` were pulled
/// out specifically so the decisions the frame callback makes each wake are
/// covered here without one. Coverage map for the four invariants the
/// frame-pacing ADR calls out:
///
/// - **Wake coalescing** (N `wake_frame` calls -> one draw): a
///   PRE-EXISTING invariant, unchanged by this diff — pinned by
///   `ui_realm::tests::redraw_requests_coalesce_to_one_flag_and_one_wake`.
/// - **Idle = zero frames**: a PRE-EXISTING invariant (the dirty gate
///   itself predates this diff; only its migration onto `wake_action` is
///   new, ADR-0035 PR2) — pinned by
///   `idle_wake_with_no_dirty_work_and_no_scheduled_frame_skips`
///   below.
/// - **No-present fallback bound**: the actual delta the frame-pacing ADR
///   introduces — pinned by `no_present_fallback_bounds_repeating_no_present_wakes`.
/// - **Ticker keeps the gate open**: the fallback's AND condition — pinned
///   by `no_present_fallback_pace_requires_both_no_present_and_an_open_gate`
///   (this module) and, at the binding layer, by
///   `binding::tests::vsync_continuation_keeps_gate_open_while_running_and_closes_on_settle`.
#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod desktop_pacing_tests {
    use std::time::{Duration, Instant};

    use super::{
        NO_PRESENT_FALLBACK_PACE, WakeAction, keeps_frame_gate_open, no_present_fallback_pace,
        wake_action,
    };

    #[test]
    fn idle_wake_with_no_dirty_work_and_no_scheduled_frame_skips() {
        assert_eq!(
            wake_action(true, false, false),
            WakeAction::Skip,
            "a spurious wake with frames enabled, nothing dirty, and no scheduled ticker must \
             render zero frames"
        );
    }

    #[test]
    fn dirty_work_or_a_scheduled_ticker_alone_renders_a_frame() {
        assert_eq!(
            wake_action(true, true, false),
            WakeAction::Render,
            "dirty work alone renders"
        );
        assert_eq!(
            wake_action(true, false, true),
            WakeAction::Render,
            "a scheduled ticker alone renders (keeps animations alive with no other dirty state)"
        );
        assert_eq!(wake_action(true, true, true), WakeAction::Render);
    }

    #[test]
    fn frames_disabled_always_pumps_async_regardless_of_dirty_or_scheduled() {
        // The load-bearing case (ADR-0035 PR2): a backgrounded app must
        // never render, even with real dirty work or a scheduled ticker —
        // dirty work accumulates untouched until frames re-enable.
        assert_eq!(wake_action(false, false, false), WakeAction::PumpAsync);
        assert_eq!(wake_action(false, true, false), WakeAction::PumpAsync);
        assert_eq!(wake_action(false, false, true), WakeAction::PumpAsync);
        assert_eq!(wake_action(false, true, true), WakeAction::PumpAsync);
    }

    /// Async-keeps-running-while-Background (ADR-0035 PR2): a spawned
    /// future must keep progressing through `PumpAsync`'s
    /// `Scheduler::drive_async_tasks` call while frames are disabled, with
    /// no frame ever advancing — and a `Resumed` transition afterward must
    /// produce exactly one frame.
    ///
    /// Standalone `Scheduler::new()`, not the process singleton: this test
    /// mirrors what `run_desktop`'s frame callback does on a `PumpAsync`
    /// wake, without needing a live window/event loop.
    ///
    /// Red-check: gate the pump too (only call `drive_async_tasks` when
    /// `wake_action` returns `Render` — a mistaken "no work while
    /// backgrounded" fix) and this fails: the future never completes (RUN
    /// IT — see the test module doc for how this is verified).
    #[test]
    fn frames_disabled_pump_async_keeps_futures_running_without_advancing_frames() {
        use std::sync::atomic::{AtomicBool, AtomicUsize};

        use flui_scheduler::{AppLifecycleState, Scheduler};

        let scheduler = Scheduler::new();
        let polls = std::sync::Arc::new(AtomicUsize::new(0));
        let completed = std::sync::Arc::new(AtomicBool::new(false));
        let polls_for_task = std::sync::Arc::clone(&polls);
        let completed_for_task = std::sync::Arc::clone(&completed);
        // Needs two polls to complete, so the loop below observes both the
        // Pending and the Ready poll — proving `drive_async_tasks` is what
        // actually advances it, not a single incidental call.
        let _token = scheduler.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
            let n = polls_for_task.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if n < 2 {
                // Re-arm itself for the next `drive_async_tasks` call —
                // without this, `poll_ready` would only ever poll it once
                // (nothing else wakes it), and the second loop iteration
                // below would silently poll zero tasks.
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            } else {
                completed_for_task.store(true, std::sync::atomic::Ordering::SeqCst);
                std::task::Poll::Ready(())
            }
        })));

        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
        assert!(!scheduler.frames_enabled());

        let frame_count_before = scheduler.frame_count();
        for _ in 0..2 {
            assert_eq!(
                wake_action(
                    scheduler.frames_enabled(),
                    true,
                    scheduler.is_frame_scheduled()
                ),
                WakeAction::PumpAsync,
                "frames disabled must always pump, even with dirty work"
            );
            scheduler.drive_async_tasks();
        }

        assert!(
            completed.load(std::sync::atomic::Ordering::SeqCst),
            "the future must complete via PumpAsync's drive_async_tasks calls alone"
        );
        assert_eq!(
            scheduler.frame_count(),
            frame_count_before,
            "no frame may run while the app is backgrounded"
        );

        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Resumed);
        assert_eq!(
            wake_action(
                scheduler.frames_enabled(),
                true,
                scheduler.is_frame_scheduled()
            ),
            WakeAction::Render
        );
        scheduler.drive_frame(web_time::Instant::now(), || {});
        assert_eq!(
            scheduler.frame_count(),
            frame_count_before + 1,
            "resuming must produce exactly one frame"
        );
    }

    #[test]
    fn pending_work_alone_keeps_the_gate_open() {
        assert!(
            keeps_frame_gate_open(false, false, true),
            "a frame that left dirty pipeline/build nodes behind must keep the fallback-pace \
             gate open (so the busy-spin throttle still applies on a rapid re-wake) even with \
             no `needs_redraw` and no scheduled ticker"
        );
    }

    #[test]
    fn needs_redraw_or_scheduled_ticker_alone_keeps_the_gate_open() {
        assert!(keeps_frame_gate_open(true, false, false));
        assert!(keeps_frame_gate_open(false, true, false));
    }

    #[test]
    fn no_signal_at_all_closes_the_gate() {
        assert!(
            !keeps_frame_gate_open(false, false, false),
            "with no redraw request, no scheduled ticker, and no pending work, the gate \
             must close so the loop can go idle"
        );
    }

    #[test]
    fn pending_work_drives_the_no_present_fallback_pace_like_any_other_open_gate() {
        // A frame that never presents (surface lost / no damage) but left dirty
        // pipeline work behind must still get the busy-spin-bounding fallback pace —
        // exactly as if `needs_redraw` or a ticker had kept the gate open.
        let keeps_gate_open = keeps_frame_gate_open(false, false, true);
        assert_eq!(
            no_present_fallback_pace(false, keeps_gate_open),
            Some(NO_PRESENT_FALLBACK_PACE)
        );
    }

    #[test]
    fn no_present_fallback_pace_requires_both_no_present_and_an_open_gate() {
        assert_eq!(
            no_present_fallback_pace(true, true),
            None,
            "a presented frame needs no fallback — Fifo present already paced it"
        );
        assert_eq!(
            no_present_fallback_pace(true, false),
            None,
            "a presented frame with a closing gate needs no fallback either"
        );
        assert_eq!(
            no_present_fallback_pace(false, false),
            None,
            "an un-presented frame with nothing re-requesting a wake needs no fallback \
             — the loop simply goes idle, no busy-spin risk"
        );
        assert_eq!(
            no_present_fallback_pace(false, true),
            Some(NO_PRESENT_FALLBACK_PACE),
            "the only busy-spin risk: no present AND a ticker keeps re-requesting a frame"
        );
    }

    /// Mutation-run target for the no-present fallback bound: simulates the shape of
    /// `run_desktop`'s frame callback for a window that never presents
    /// (e.g. minimized/occluded) while a repeating ticker keeps
    /// re-requesting a frame every wake — the exact scenario that used to
    /// busy-spin at CPU speed (observed pre-fix: ~30 000 fps) once the
    /// fixed frame-budget sleep this diff removes was the only thing
    /// bounding it.
    ///
    /// This cannot drive the real winit closure (it requires a live event
    /// loop), so it exercises the same predicate + `thread::sleep` pairing
    /// `run_desktop` calls, in a tight loop bounded by wall-clock time.
    /// Deleting the `sleep` (or the `if let Some` guard around it) turns
    /// this from ~5 iterations in the test window into whatever the CPU
    /// can spin through in that time — comfortably over the assertion's
    /// generous ceiling.
    #[test]
    fn no_present_fallback_bounds_repeating_no_present_wakes() {
        let window = Duration::from_millis(80);
        let deadline = Instant::now() + window;
        let mut iterations = 0u32;

        while Instant::now() < deadline {
            iterations += 1;
            let presented = false; // simulated: no damage / occluded / surface lost
            let keeps_gate_open = true; // simulated: a repeating AnimationController
            if let Some(pace) = no_present_fallback_pace(presented, keeps_gate_open) {
                std::thread::sleep(pace);
            }
        }

        assert!(
            iterations < 50,
            "no-present fallback failed to bound the loop: {iterations} iterations in \
             {window:?} (expected roughly window / NO_PRESENT_FALLBACK_PACE, generously \
             capped) — a busy-spin without it would rack up orders of magnitude more",
        );
    }
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn run_desktop<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    use std::{cell::RefCell, rc::Rc, sync::Arc};

    use flui_engine::wgpu::Renderer;
    use flui_hot_reload::{
        HotReloadTier, RebuildHookRegistration, WorkerPollOutcome, WorkerReloadDriver, engine::env,
        register_request_rebuild,
    };
    use flui_platform::{
        Platform, WindowOptions,
        traits::{DispatchEventResult, PlatformInput},
    };
    use parking_lot::Mutex;

    use crate::embedder::PlatformWindowHandle;

    tracing::info!("Starting desktop platform via flui-platform");

    let worker_driver = config
        .worker_plugin_path
        .clone()
        .or_else(|| std::env::var(env::WORKER_PLUGIN).ok().map(Into::into))
        .map(WorkerReloadDriver::new);
    let has_worker_driver = worker_driver.is_some();
    let worker_driver = Arc::new(Mutex::new(worker_driver));

    // Platform init is an environment failure (missing display server, unsupported
    // OS, driver problem), not a `BUG:` invariant — no `bootstrap_error_slot` exists
    // yet to route this through (that cell, and the `platform` it needs for
    // `quit()`, only exist once `on_ready` is running), so this is the one desktop
    // failure this function still surfaces via `panic!` directly rather than the
    // deferred-teardown path below. It still gets a full error log and the same
    // "desktop bootstrap failed" wording as that deferred path, instead of a bare
    // `.expect()`'s terse, context-free message.
    let platform = match flui_platform::current_platform() {
        Ok(platform) => platform,
        Err(error) => {
            tracing::error!(%error, "Failed to initialize platform");
            panic!("desktop bootstrap failed: platform initialization error: {error:?}");
        }
    };

    // `rebuild_registration`'s `Drop` detaches the hot-reload hook and must
    // stay alive until the event loop exits — but it (like the window and
    // every callback below) can only be created from inside `on_ready`, so
    // it is threaded back out through this cell instead of a plain local.
    let rebuild_registration: Rc<RefCell<Option<RebuildHookRegistration>>> =
        Rc::new(RefCell::new(None));
    let rebuild_registration_slot = Rc::clone(&rebuild_registration);

    // Bootstrap can fail fatally (GPU init, `UiRealm` construction, root
    // widget attach) from inside `on_ready`, which has no return path back
    // to this function's caller — thread the failure out through this cell
    // instead, same pattern as `rebuild_registration`.
    let bootstrap_error: Rc<RefCell<Option<anyhow::Error>>> = Rc::new(RefCell::new(None));
    let bootstrap_error_slot = Rc::clone(&bootstrap_error);

    /// The actual desktop bootstrap: opens the window, initializes the GPU
    /// renderer, mounts the widget tree, and wires every platform/window
    /// callback. Runs exactly once, synchronously, inside `on_ready` (see
    /// `Platform::run`'s doc) — never before, since the winit backend can
    /// only create a window from inside a running event loop
    /// (`ActiveEventLoop` is unreachable beforehand, and `open_window` fails
    /// fast rather than deadlock if called too early).
    ///
    /// Pulled out of the `on_ready` closure into a named fn so rustfmt
    /// actually formats it — rustfmt does not reliably reformat very large
    /// closure literals passed as call arguments.
    fn bootstrap_desktop<V>(
        platform: &dyn Platform,
        root: V,
        config: AppConfig,
        has_worker_driver: bool,
        worker_driver: Arc<Mutex<Option<WorkerReloadDriver>>>,
        rebuild_registration_slot: Rc<RefCell<Option<RebuildHookRegistration>>>,
        bootstrap_error_slot: Rc<RefCell<Option<anyhow::Error>>>,
    ) where
        V: View + StatelessView + Clone + 'static,
    {
        tracing::info!("Platform ready");

        // 0. Wire the platform clipboard (ADR-0034) before anything else can
        // observe `AppBinding::clipboard()`. `platform` is `&dyn Platform`
        // here, still fully intact — `on_ready` runs before `Platform::run`
        // returns, so this is not the pre-`run()` extraction Android/web
        // need, just an early call on a reference that stays valid for the
        // rest of this function.
        AppBinding::instance().set_platform_clipboard(platform.clipboard());

        // 1. Open window now that the event loop is running. Window creation is
        // an environment failure (display server hiccup, resource exhaustion),
        // not a `BUG:` invariant, and — unlike platform init above — this DOES
        // run inside `on_ready` with a live `platform` and `bootstrap_error_slot`
        // available, so it gets the same deferred-panic-after-teardown handling
        // as the GPU/realm/attach failures below instead of an immediate bare
        // `.expect()` panic mid-`on_ready`.
        let options: WindowOptions = (&config).into();
        let window = match platform.open_window(options) {
            Ok(window) => window,
            Err(error) => {
                tracing::error!(%error, "Window creation failed");
                *bootstrap_error_slot.borrow_mut() = Some(error.context("Window creation failed"));
                platform.quit();
                return;
            }
        };

        // 2. Create GPU renderer directly (no DesktopEmbedder)
        let phys_size = window.physical_size();
        let renderer = pollster::block_on(async {
            let handle = PlatformWindowHandle(window.as_ref());
            Renderer::new(&handle).await
        });
        let mut renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("GPU init failed: {:?}", e);
                *bootstrap_error_slot.borrow_mut() =
                    Some(anyhow::anyhow!(e).context("GPU init failed"));
                platform.quit();
                return;
            }
        };
        renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

        // 3. Mount root widget at the LOGICAL size; the framework lays out
        // in logical pixels and the paint root's DPR transform maps to the
        // physical surface. Set the DPR BEFORE attach so the RenderView
        // configuration and the first frame agree on the scale.
        let scale_factor = window.scale_factor() as f32;
        AppBinding::instance()
            .render_pipeline_mut()
            .set_device_pixel_ratio(scale_factor);
        let ui_realm =
            match super::ui_realm::UiRealm::new(AppBinding::instance().frame_wake_callback()) {
                Ok(realm) => realm,
                Err(e) => {
                    tracing::error!(error = %e, "UiRealm construction failed");
                    *bootstrap_error_slot.borrow_mut() =
                        Some(anyhow::anyhow!(e).context("UiRealm construction failed"));
                    platform.quit();
                    return;
                }
            };
        ui_realm.bind_to_app(AppBinding::instance());
        let logical = window.logical_size();
        let attach = ui_realm.enter(|realm| {
            AppBinding::instance().attach_root_widget_with_size(
                realm,
                &root,
                logical.width.0,
                logical.height.0,
            )
        });
        if let Err(e) = attach {
            tracing::error!("Root widget attach failed: {:?}", e);
            *bootstrap_error_slot.borrow_mut() =
                Some(anyhow::anyhow!(e).context("Root widget attach failed"));
            platform.quit();
            return;
        }

        // 3b. Wire the wake chain (E0a).
        //
        // `on_need_frame` fires whenever `handle_build_scheduled` determines a new
        // frame is required (e.g. after setState).  The closure calls `wake_frame`
        // which sets `needs_redraw` atomically AND calls `PlatformWindow::
        // request_redraw()` so the winit event loop wakes from idle.
        //
        // Deadlock analysis:
        // * `wake_frame` acquires only `active_window` (leaf Mutex).
        // * The closure is called from `handle_build_scheduled`, which holds no
        //   `inner`/`widgets` lock (see `WidgetsBinding::handle_build_scheduled`
        //   doc).
        // * `on_need_frame` itself is a separate `RwLock` on `WidgetsBinding`,
        //   never held across any `inner` critical section.
        // Therefore: no lock ordering conflict.
        {
            let widgets = ui_realm.widgets();
            let wake = AppBinding::instance().frame_wake_callback();
            widgets.set_on_need_frame(move || wake());
        }

        // Wire `on_build_scheduled` on the BuildOwner so a dirty-element
        // registration (e.g. from setState inside an element build) wakes the
        // platform loop. The callback fires from inside `schedule_build_for`,
        // which runs during a build while the AppBinding `widgets` write lock is
        // held — so it must NOT re-lock `widgets`. It calls `wake_frame`
        // directly (the same effect as the `on_need_frame` callback above),
        // which touches only the `active_window` leaf lock. The callback must not
        // re-enter widget state while `BuildOwner` is scheduling; realm entry is
        // reserved for the outer event/frame dispatch boundary.
        {
            let widgets = ui_realm.widgets();
            widgets.with_build_owner_mut(|build_owner| {
                let wake = AppBinding::instance().frame_wake_callback();
                build_owner.set_on_build_scheduled(move || wake());
            });
        }

        // 3c. Construct the per-window owner and its bounded command inbox.
        // The wake is the existing chain: `wake_frame` sets
        // `needs_redraw` and queues a `RedrawRequested`, so a command sent to an
        // idle loop produces the frame whose drain observes it.
        //
        tracing::info!(
            realm_id = ?ui_realm.realm_id(),
            inbox_capacity = ui_realm.command_sender().capacity(),
            "UiRealm constructed"
        );
        let hot_reload_sender = ui_realm.command_sender();
        let realm_dispatch = install_platform_realm(ui_realm);
        *rebuild_registration_slot.borrow_mut() = has_worker_driver
            .then(|| register_request_rebuild(queued_hot_reload_hook(hot_reload_sender)));

        // 4. Wrap renderer for callback sharing
        let renderer = Arc::new(Mutex::new(renderer));

        // 5. Register input callback -> AppBinding::handle_input()
        window.on_input(Box::new(move |input: PlatformInput| {
            let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::Input(input));
            DispatchEventResult::resolved(false, true)
        }));

        // 6. Register frame callback -> scheduler + AppBinding::render_frame()
        let renderer_frame = Arc::clone(&renderer);
        let worker_driver_frame = Arc::clone(&worker_driver);
        window.on_request_frame(Box::new(move || {
        let renderer_frame = Arc::clone(&renderer_frame);
        let worker_driver_frame = Arc::clone(&worker_driver_frame);
        let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::Frame(Box::new(move |realm| {
            if let Some(ref mut driver) = *worker_driver_frame.lock()
                && matches!(driver.poll(), WorkerPollOutcome::Reloaded { .. })
            {
                AppBinding::instance()
                    .perform_hot_reload_entered(realm, HotReloadTier::HotReload);
            }

            let binding = AppBinding::instance();
            let scheduler = Scheduler::instance();

        // Owner-inbox drain: commands and worker results
        // commit HERE, at the frame boundary while the scheduler phase is
        // Idle — never inside the frame transaction below. Runs before the
        // dirty gate so a command-driven redraw request is observed by the
        // very frame its wake produced.
        //
        // The runtime is TAKEN out of the slot for the drain (and restored
        // after) so drained user closures never run under the RefCell
        // borrow: a command that re-enters this frame callback through a
        // nested platform pump then finds an empty slot and skips the
        // drain, instead of panicking the borrow.
            let inbox_redraw = drain_owner_inbox(realm);

            let dirty =
                inbox_redraw || binding.needs_redraw() || binding.has_pending_work(realm);
            match wake_action(scheduler.frames_enabled(), dirty, scheduler.is_frame_scheduled()) {
                WakeAction::Skip => return,
                WakeAction::PumpAsync => {
                    // Frames disabled (Hidden/Paused/Detached): the mid-frame
                    // `drive_async_tasks` poll inside `handle_begin_frame`
                    // never runs because no frame runs at all — this
                    // explicit call is the ONLY thing keeping a spawned
                    // future progressing while backgrounded. No begin/draw
                    // frame, no tickers, no pipeline, no present.
                    scheduler.drive_async_tasks();
                    // Reuse the existing no-present throttle: a backgrounded
                    // wake with dirty/pending work re-requesting another
                    // wake every loop tick has the identical busy-spin risk
                    // an un-presented frame with an open gate has, and
                    // nothing else paces it while frames are disabled.
                    let keeps_gate_open = keeps_frame_gate_open(
                        binding.needs_redraw(),
                        scheduler.is_frame_scheduled(),
                        binding.has_pending_work(realm),
                    );
                    if let Some(pace) = no_present_fallback_pace(false, keeps_gate_open) {
                        std::thread::sleep(pace);
                    }
                    return;
                }
                WakeAction::Render => {}
            }

            let now = web_time::Instant::now();

        // Scheduler callbacks (animations). NOTE: the global `Scheduler` is driven
        // off this per-frame `Instant::now()`, while the tree-bound `Vsync`
        // (AppBinding::draw_frame) ticks off `AppBinding`'s own `start` origin —
        // two separate clocks ON PURPOSE: the controller sets are disjoint (implicit
        // animations register with `Vsync`; plain controllers carry a private
        // `Scheduler` ticker, never the global one), so the origins never need to
        // agree and no controller is advanced twice.
        // The ONE shared frame ordering — begin (transient +
        // microtasks + the single async-driver poll) -> persistent callbacks ->
        // the pipeline below -> post-frame callbacks -> Idle. `HeadlessBinding`
        // drives the same helper on its binding-local scheduler.
            let presented = scheduler.drive_frame(now, || {
            // Render frame via AppBinding
            let mut r = renderer_frame.lock();
                let did_present = binding.render_frame_entered(realm, &mut *r);

            // GPU device-loss recovery: if the device was lost during this frame
            // (detected by the wgpu callback that fired between render_frame calls),
            // attempt a synchronous rebuild on the runner thread. `pollster` is
            // already a dep and safe to use here — the desktop runner owns this
            // synchronous callback, not an async executor.
            if r.is_device_lost() {
                match pollster::block_on(r.recover()) {
                    Ok(()) => {
                        tracing::warn!("GPU device lost — recovered successfully");
                        // `wake_frame` (not `request_redraw`) so an idle winit loop
                        // actually queues a `RedrawRequested`: device loss is
                        // detected on a quiescent loop, where only flipping the
                        // `needs_redraw` flag would leave the recovered renderer
                        // idle until the next external input/resize.
                        AppBinding::instance().wake_frame();
                    }
                    Err(e) => {
                        // Driver may still be resetting. Log and let the next frame
                        // retry; the device-lost flag remains set so recover() will
                        // be tried again.
                        tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                    }
                }
            }
                did_present
            });

        // No-present fallback throttle. Fifo present (the default, see
        // `select_present_mode`) blocks every PRESENTED frame at display
        // cadence — that IS the steady-state pacing, which is why the fixed
        // frame-budget sleep this replaced is gone. A frame that never
        // reaches `present()` (no damage, occluded surface, surface lost)
        // gets none of that blocking, so if nothing else is going to wake
        // this loop, an unpaced wake is harmless: the loop falls back to
        // `ControlFlow::Wait` and blocks on the next real event. The
        // busy-spin this guards against (observed: ~30 000 fps) only
        // happens when a ticker/animation keeps re-requesting a frame every
        // wake with nothing pacing it — `no_present_fallback_pace` fires
        // only in exactly that combination.
            let keeps_gate_open = keeps_frame_gate_open(
                binding.needs_redraw(),
                scheduler.is_frame_scheduled(),
                binding.has_pending_work(realm),
            );
            if let Some(pace) = no_present_fallback_pace(presented, keeps_gate_open) {
                // This runs on the platform event-loop thread, so the sleep
                // blocks input dispatch for its duration — acceptable here
                // because this path only fires for an occluded/undamaged
                // window with a ticker still running, not an interactive one.
                std::thread::sleep(pace);
            }
        })));
    }));

        // 7. Register resize callback -> renderer.resize()
        let renderer_resize = Arc::clone(&renderer);
        window.on_resize(Box::new(move |size, scale_factor| {
            let apply_size = size;
            let renderer_resize = Arc::clone(&renderer_resize);
            let _ = dispatch_platform_realm(
                realm_dispatch,
                RealmEvent::Resize {
                    size,
                    scale_factor,
                    apply_surface: Box::new(move || {
                        let w = (apply_size.width.0 * scale_factor) as u32;
                        let h = (apply_size.height.0 * scale_factor) as u32;
                        renderer_resize.lock().resize(w, h);
                    }),
                },
            );
        }));

        // 8. Lifecycle callbacks
        //
        // ADR-0035 (PR1): Started/Terminating call the canonical
        // `flui_scheduler::Scheduler` lifecycle directly rather than
        // round-tripping through `RealmEvent`/`dispatch_platform_realm` —
        // neither needs `&UiRealm` access, and both already run on the
        // realm's owner thread (the platform event-loop thread), so the
        // debug_assert below is the cheap, in-scope way to verify that
        // invariant instead of teaching the scheduler about realm ownership.

        // Platform quit -> Detached (frames disabled, listeners notified).
        platform.on_quit(Box::new(move || {
            tracing::info!("Platform quit");
            debug_assert_eq!(
                std::thread::current().id(),
                realm_dispatch.owner_thread,
                "platform on_quit must fire on the realm's owner thread"
            );
            Scheduler::instance().handle_app_lifecycle_state_change(AppLifecycleState::Detached);
        }));

        // Window close -> log and let the platform handle quit
        // (Windows window proc already calls PostQuitMessage on WM_DESTROY)
        window.on_close(Box::new(move || {
            tracing::info!("Window closed");
        }));

        // Window should-close -> allow by default
        window.on_should_close(Box::new(|| {
            tracing::debug!("Window close requested, allowing");
            true
        }));

        // Window focus/visibility -> the `(visible, focused)`
        // `AppLifecycleState` derivation (ADR-0035 PR2). `on_visibility_
        // status_change` rides winit's `Occluded` event; Wayland delivery
        // is compositor-conditional (see that callback's doc) — where a
        // compositor never sends it, the window is treated as always
        // visible, matching pre-PR2 behavior.
        window.on_active_status_change(Box::new(move |focused| {
            let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::WindowFocus(focused));
        }));
        window.on_visibility_status_change(Box::new(move |visible| {
            let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::WindowVisibility(visible));
        }));

        // 9. Store window in AppBinding for runtime access — BEFORE
        // marking the lifecycle Resumed or requesting the initial redraw.
        // Both of those can synchronously run the first frame through
        // `dispatch_platform_realm`; if `active_window` were still `None`
        // at that point, anything resolving it during that frame (an
        // autofocus `EditableText` attaching its IME client, for instance)
        // would silently no-op instead of attaching.
        AppBinding::instance().set_window(window);

        // Mark lifecycle as started (Resumed).
        debug_assert_eq!(
            std::thread::current().id(),
            realm_dispatch.owner_thread,
            "desktop bootstrap must run on the realm's owner thread"
        );
        Scheduler::instance().handle_app_lifecycle_state_change(AppLifecycleState::Resumed);

        // 10. Request initial redraw, now that the window is stored.
        // `wake_frame` (not `with_window(|w| w.request_redraw())`): it clones
        // the window out from under `active_window`'s lock before calling
        // through, so a backend whose `request_redraw` re-enters `AppBinding`
        // synchronously (headless, in this crate's own tests) cannot
        // deadlock on that same lock — the same clone-then-call discipline
        // `TextInputPlatformBridge`/`perform_haptic_feedback` follow.
        AppBinding::instance().wake_frame();

        tracing::info!("Desktop platform initialized with callbacks");
    }

    // Window creation, GPU/renderer setup, and callback wiring all run
    // inside `on_ready` rather than before `run()`. The winit backend can
    // only create a window from inside a running event loop (`ActiveEventLoop`
    // is unreachable beforehand); opening it earlier would deadlock forever
    // waiting for a pump that never started. `on_ready` runs exactly once,
    // synchronously, on this same thread — see `Platform::run`'s doc.
    platform.run(Box::new(move |platform: &dyn Platform| {
        bootstrap_desktop(
            platform,
            root,
            config,
            has_worker_driver,
            worker_driver,
            rebuild_registration_slot,
            bootstrap_error_slot,
        );
    }));

    // Event loop exited: drop the runtime now (releases the at-most-one
    // claim; outstanding senders turn `OwnerGone`) instead of at thread
    // death.
    drop(rebuild_registration.borrow_mut().take());
    teardown_platform_realm();

    // Surface a fatal bootstrap failure (GPU init, `UiRealm` construction,
    // root widget attach) now that the event loop has exited — those
    // failures happen inside `on_ready`, with no return path back here
    // except through `bootstrap_error`, and quitting the platform on them
    // (see `bootstrap_desktop`) must not look like a clean exit.
    if let Some(err) = bootstrap_error.borrow_mut().take() {
        panic!("desktop bootstrap failed: {err:?}");
    }
}

// ============================================================================
// Android Implementation
// ============================================================================

/// Run a FLUI application on Android with default configuration.
///
/// This is the primary entry point for Android apps. Call this from your
/// `android_main()` function:
///
/// ```rust,ignore
/// #[no_mangle]
/// fn android_main(app: AndroidApp) {
///     flui_app::run_app_android(app, MyRootView);
/// }
/// ```
#[cfg(target_os = "android")]
pub fn run_app_android<V>(app: android_activity::AndroidApp, root: V)
where
    V: View + StatelessView + Clone + 'static,
{
    run_app_android_with_config(app, root, AppConfig::default());
}

/// Run a FLUI application on Android with custom configuration.
///
/// Like [`run_app_android`] but allows specifying app configuration.
///
/// ```rust,ignore
/// #[no_mangle]
/// fn android_main(app: AndroidApp) {
///     let config = AppConfig::new()
///         .with_title("My App")
///         .with_size(800, 600);
///     flui_app::run_app_android_with_config(app, MyRootView, config);
/// }
/// ```
#[cfg(target_os = "android")]
pub fn run_app_android_with_config<V>(app: android_activity::AndroidApp, root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    init_logging();

    tracing::info!(
        title = %config.title,
        "Starting FLUI application on Android"
    );

    run_android(root, config, app);
}

#[cfg(target_os = "android")]
fn run_android<V>(root: V, config: AppConfig, app: android_activity::AndroidApp)
where
    V: View + StatelessView + Clone + 'static,
{
    use std::{path::PathBuf, sync::Arc};

    use flui_engine::wgpu::Renderer;
    use flui_hot_reload::HotReloadDriver;
    use flui_platform::{
        AndroidPlatform, Platform, WindowOptions,
        traits::{DispatchEventResult, PlatformInput},
    };
    use parking_lot::Mutex;

    use crate::embedder::PlatformWindowHandle;

    tracing::info!("Starting Android platform via flui-platform");

    // Hot-reload: build plugin path from app's internal data directory
    let plugin_path: PathBuf = app
        .internal_data_path()
        .map(|p| p.join("libflui_scene.so"))
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/libflui_scene.so"));

    let hot_reload = Arc::new(Mutex::new(HotReloadDriver::new(&plugin_path)));

    let platform: Box<dyn Platform> = Box::new(AndroidPlatform::new(app));

    // 0. Wire the platform clipboard (ADR-0034). Extracted from the `Box`
    // now, while `platform` is still intact: `platform.run(...)` below takes
    // `self: Box<Self>` by value, and the `on_ready` closure it invokes
    // discards its `platform` parameter, so there is no later point at which
    // this platform's `clipboard()` is reachable at all.
    AppBinding::instance().set_platform_clipboard(platform.clipboard());

    // 1. Open window (wraps the existing ANativeWindow) before run()
    let options: WindowOptions = (&config).into();
    let window = platform
        .open_window(options)
        .expect("Failed to create Android window");

    // 2. Create GPU renderer (Vulkan backend on Android)
    let phys_size = window.physical_size();
    let renderer = pollster::block_on(async {
        let handle = PlatformWindowHandle(window.as_ref());
        Renderer::new(&handle).await
    });
    let mut renderer = match renderer {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("GPU init failed: {:?}", e);
            return;
        }
    };
    renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

    // 3. Mount root widget (used when no plugin is active) at the
    // LOGICAL size; the paint root's DPR transform maps to physical.
    let scale_factor = window.scale_factor() as f32;
    AppBinding::instance()
        .render_pipeline_mut()
        .set_device_pixel_ratio(scale_factor);
    let ui_realm = match super::ui_realm::UiRealm::new(AppBinding::instance().frame_wake_callback())
    {
        Ok(realm) => realm,
        Err(error) => {
            tracing::error!(%error, "UiRealm construction failed");
            return;
        }
    };
    ui_realm.bind_to_app(AppBinding::instance());
    let logical = window.logical_size();
    let attach = ui_realm.enter(|realm| {
        AppBinding::instance().attach_root_widget_with_size(
            realm,
            &root,
            logical.width.0 as f32,
            logical.height.0 as f32,
        )
    });
    if let Err(e) = attach {
        tracing::error!("Root widget attach failed: {:?}", e);
        return;
    }
    let realm_dispatch = install_platform_realm(ui_realm);

    // 4. Wrap renderer for callback sharing
    let renderer = Arc::new(Mutex::new(renderer));

    // 5. Register input callback -> AppBinding::handle_input()
    window.on_input(Box::new(move |input: PlatformInput| {
        let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::Input(input));
        DispatchEventResult::resolved(false, true)
    }));

    // 6. Register frame callback -- with hot-reload plugin override
    let renderer_frame = Arc::clone(&renderer);
    let hot_reload_frame = Arc::clone(&hot_reload);
    window.on_request_frame(Box::new(move || {
        let renderer_frame = Arc::clone(&renderer_frame);
        let hot_reload_frame = Arc::clone(&hot_reload_frame);
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmEvent::Frame(Box::new(move |realm| {
                // Owner-inbox drain: commands and worker results commit HERE,
                // at the frame boundary while the scheduler phase is Idle —
                // never inside the frame transaction below. Runs before
                // everything else in this callback, including the hot-reload
                // plugin scene fast path below, so a command-driven redraw
                // request is observed by the very frame its wake produced
                // regardless of which rendering path this frame takes.
                let inbox_redraw = drain_owner_inbox(realm);

                let mut r = renderer_frame.lock();
                let (w, h) = r.size();
                let mut hr = hot_reload_frame.lock();

                // Poll for plugin updates (mtime check, auto-reload).
                hr.poll(w as f32, h as f32);

                // If a scene plugin is active it owns this presentation frame,
                // but the callback still executes inside the realm entry scope.
                if let Some(scene) = hr.build_scene(w as f32, h as f32) {
                    drop(hr);
                    if let Err(e) = r.render_scene(&scene) {
                        tracing::error!("Plugin render failed: {:?}", e);
                    }
                    return;
                }
                drop(hr);
                drop(r);

                let binding = AppBinding::instance();
                let has_pending = binding.has_pending_work(realm);
                let dirty = inbox_redraw || binding.needs_redraw() || has_pending;
                let scheduler = Scheduler::instance();
                match wake_action(scheduler.frames_enabled(), dirty, scheduler.is_frame_scheduled())
                {
                    WakeAction::Skip => return,
                    WakeAction::PumpAsync => {
                        // Frames disabled: pump only the async driver — no
                        // begin/draw frame, no tickers, no pipeline, no
                        // present. See `wake_action`'s doc for why this is
                        // the only thing keeping a spawned future
                        // progressing while backgrounded.
                        scheduler.drive_async_tasks();
                        return;
                    }
                    WakeAction::Render => {}
                }

                let now = web_time::Instant::now();
                // Scheduler callbacks and rendering share ONE `UiRealm::enter`
                // dynamic extent; callbacks may legally resolve realm-local
                // capabilities throughout the complete frame transaction.
                scheduler.drive_frame(now, || {
                    let mut r = renderer_frame.lock();
                    binding.render_frame_entered(realm, &mut *r);

                    if r.is_device_lost() {
                        match pollster::block_on(r.recover()) {
                            Ok(()) => {
                                tracing::warn!("GPU device lost — recovered successfully");
                                AppBinding::instance().wake_frame();
                            }
                            Err(e) => {
                                tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                            }
                        }
                    }
                });
            })),
        );
    }));

    // 7. Register resize callback -> renderer.resize()
    let renderer_resize = Arc::clone(&renderer);
    window.on_resize(Box::new(move |size, scale_factor| {
        let apply_size = size;
        let renderer_resize = Arc::clone(&renderer_resize);
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmEvent::Resize {
                size,
                scale_factor,
                apply_surface: Box::new(move || {
                    let w = (apply_size.width.0 * scale_factor) as u32;
                    let h = (apply_size.height.0 * scale_factor) as u32;
                    renderer_resize.lock().resize(w, h);
                }),
            },
        );
    }));

    // 8. Lifecycle callbacks
    //
    // ADR-0035 (PR1): Started/Terminating call the canonical
    // `flui_scheduler::Scheduler` lifecycle directly — see `run_desktop`'s
    // identical comment for why this bypasses `RealmEvent`/
    // `dispatch_platform_realm` and why the debug_assert lives here rather
    // than in the scheduler.

    // Platform quit -> Detached (frames disabled, listeners notified).
    platform.on_quit(Box::new(move || {
        tracing::info!("Platform quit");
        debug_assert_eq!(
            std::thread::current().id(),
            realm_dispatch.owner_thread,
            "platform on_quit must fire on the realm's owner thread"
        );
        Scheduler::instance().handle_app_lifecycle_state_change(AppLifecycleState::Detached);
    }));

    // Window close (fired by Android Destroy event)
    window.on_close(Box::new(move || {
        tracing::info!("Window closed");
    }));

    // Window active status. On Android this one callback conflates real
    // window focus (`MainEvent::GainedFocus`/`LostFocus`) with the app's
    // actual pause/resume signal (`MainEvent::Resume`/`Pause` currently fire
    // the identical `dispatch_active_status_change` — see
    // `flui-platform`'s `platforms/android/mod.rs`); a dedicated
    // `MainEvent` -> lifecycle callback that tells them apart is a named
    // follow-up (ADR-0035), not this PR. Until that split lands, this keeps
    // the existing transport but fixes the mapping: `false` ladders all the
    // way to `Paused` and `true` back to `Resumed` — Android's
    // backgrounding signal needs the deeper ladder the desktop/web
    // `(visible, focused)` derivation (which only ever reaches
    // `Inactive`/`Hidden`) does not produce.
    window.on_active_status_change(Box::new(move |resumed| {
        let target = if resumed {
            AppLifecycleState::Resumed
        } else {
            AppLifecycleState::Paused
        };
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmEvent::Frame(Box::new(move |realm| {
                let old = Scheduler::instance().lifecycle_state();
                emit_lifecycle_transition(realm, old, target);
            })),
        );
    }));

    // 9. Store window in AppBinding for runtime access — BEFORE marking the
    // lifecycle Resumed or requesting the initial redraw. Both of those can
    // synchronously run the first frame through `dispatch_platform_realm`;
    // if `active_window` were still `None` at that point, anything
    // resolving it during that frame (an autofocus `EditableText`
    // attaching its IME client, for instance) would silently no-op instead
    // of attaching.
    AppBinding::instance().set_window(window);

    // Mark lifecycle as started (Resumed).
    debug_assert_eq!(
        std::thread::current().id(),
        realm_dispatch.owner_thread,
        "android bootstrap must run on the realm's owner thread"
    );
    Scheduler::instance().handle_app_lifecycle_state_change(AppLifecycleState::Resumed);

    // 10. Request initial redraw, now that the window is stored.
    // `wake_frame` (not `with_window(|w| w.request_redraw())`): it clones
    // the window out from under `active_window`'s lock before calling
    // through, so a backend whose `request_redraw` re-enters `AppBinding`
    // synchronously (headless, in this crate's own tests) cannot deadlock
    // on that same lock — the same clone-then-call discipline
    // `TextInputPlatformBridge`/`perform_haptic_feedback` follow.
    AppBinding::instance().wake_frame();

    tracing::info!("Android platform initialized with callbacks (hot-reload enabled)");

    // Run the event loop (takes ownership of the platform)
    platform.run(Box::new(|_platform| {
        tracing::info!("Android platform ready");
    }));
    teardown_platform_realm();
}

// ============================================================================
// iOS Implementation
// ============================================================================

#[cfg(target_os = "ios")]
fn run_ios(_config: AppConfig) {
    // Native iOS (UIKit windowing + surface) is a Cross.P (Platform breadth)
    // deliverable — see docs/ROADMAP.md's Cross.P section. This stub exists
    // only so `#[cfg(target_os = "ios")]` builds compile; there is no
    // UIKit-backed `flui-platform` implementation to call into yet.
    tracing::info!("iOS platform - not yet implemented");
}

// ============================================================================
// Web Implementation
// ============================================================================

#[cfg(target_arch = "wasm32")]
fn run_web<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    use std::sync::Arc;

    use flui_engine::wgpu::Renderer;
    use flui_platform::{
        WindowOptions,
        traits::{DispatchEventResult, PlatformInput},
    };
    use parking_lot::Mutex;

    use crate::embedder::PlatformWindowHandle;

    tracing::info!("Starting web platform via flui-platform");

    let platform = flui_platform::current_platform().expect("Failed to initialize web platform");

    // 0. Wire the platform clipboard (ADR-0034). Extracted from the `Box`
    // now, while `platform` is still intact — see `run_android`'s identical
    // comment for why there is no later point at which this platform's
    // `clipboard()` is reachable.
    AppBinding::instance().set_platform_clipboard(platform.clipboard());

    // 1. Open window (creates canvas) before run() since run() takes ownership
    let options: WindowOptions = (&config).into();
    let window: Arc<dyn flui_platform::PlatformWindow> = Arc::from(
        platform
            .open_window(options)
            .expect("Failed to create canvas window"),
    );

    // 2. Shared renderer slot — starts as None, filled async once the WebGPU
    //    adapter is available. `Option` lets the frame callback skip frames that
    //    arrive before the renderer is ready.
    let renderer: Arc<Mutex<Option<Renderer>>> = Arc::new(Mutex::new(None));

    let phys_size = window.physical_size();
    let renderer_init = Arc::clone(&renderer);
    let renderer_window = Arc::clone(&window);

    // The future owns a strong window reference. This is required because the
    // browser platform installs RAF and returns immediately, and startup can
    // also return early before the window reaches AppBinding.
    wasm_bindgen_futures::spawn_local(async move {
        let handle = PlatformWindowHandle(renderer_window.as_ref());
        let mut r = match Renderer::new(&handle).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("GPU init failed: {:?}", e);
                return;
            }
        };
        r.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);
        tracing::info!("WebGPU renderer initialized");
        *renderer_init.lock() = Some(r);
    });

    // 3. Mount root widget at the LOGICAL size; the paint root's DPR
    // transform maps to the physical canvas.
    let scale_factor = window.scale_factor() as f32;
    AppBinding::instance()
        .render_pipeline_mut()
        .set_device_pixel_ratio(scale_factor);
    let ui_realm = match super::ui_realm::UiRealm::new(AppBinding::instance().frame_wake_callback())
    {
        Ok(realm) => realm,
        Err(error) => {
            tracing::error!(%error, "UiRealm construction failed");
            return;
        }
    };
    ui_realm.bind_to_app(AppBinding::instance());
    let logical = window.logical_size();
    let attach = ui_realm.enter(|realm| {
        AppBinding::instance().attach_root_widget_with_size(
            realm,
            &root,
            logical.width.0 as f32,
            logical.height.0 as f32,
        )
    });
    if let Err(e) = attach {
        tracing::error!("Root widget attach failed: {:?}", e);
        return;
    }
    let realm_dispatch = install_platform_realm(ui_realm);

    // 4. Register input callback
    window.on_input(Box::new(move |input: PlatformInput| {
        let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::Input(input));
        DispatchEventResult::resolved(false, true)
    }));

    // 5. Register frame callback
    let renderer_frame = Arc::clone(&renderer);
    window.on_request_frame(Box::new(move || {
        let renderer_frame = Arc::clone(&renderer_frame);
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmEvent::Frame(Box::new(move |realm| {
                // Owner-inbox drain: commands and worker results commit HERE,
                // at the frame boundary while the scheduler phase is Idle —
                // never inside the frame transaction below. Runs before the
                // dirty gate so a command-driven redraw request is observed
                // by the very frame its wake produced.
                let inbox_redraw = drain_owner_inbox(realm);

                let binding = AppBinding::instance();
                let has_pending = binding.has_pending_work(realm);
                let dirty = inbox_redraw || binding.needs_redraw() || has_pending;
                let scheduler = Scheduler::instance();
                match wake_action(scheduler.frames_enabled(), dirty, scheduler.is_frame_scheduled())
                {
                    WakeAction::Skip => return,
                    WakeAction::PumpAsync => {
                        // Frames disabled: pump only the async driver — see
                        // `wake_action`'s doc for why this is the only thing
                        // keeping a spawned future progressing while
                        // backgrounded.
                        scheduler.drive_async_tasks();
                        return;
                    }
                    WakeAction::Render => {}
                }

                let now = web_time::Instant::now();
                // Scheduler callbacks and rendering share one realm entry.
                scheduler.drive_frame(now, || {
                    let mut slot = renderer_frame.lock();
                    let Some(r) = slot.as_mut() else {
                        return;
                    };

                    binding.render_frame_entered(realm, r);

                    if r.is_device_lost() {
                        drop(slot);
                        let renderer_recover = Arc::clone(&renderer_frame);
                        wasm_bindgen_futures::spawn_local(async move {
                            // Never hold the renderer mutex across `.await`.
                            let Some(mut renderer) = renderer_recover.lock().take() else {
                                return;
                            };
                            let result = renderer.recover().await;
                            *renderer_recover.lock() = Some(renderer);
                            match result {
                                Ok(()) => {
                                    tracing::warn!("GPU device lost — recovered successfully");
                                    AppBinding::instance().wake_frame();
                                }
                                Err(e) => {
                                    tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                                }
                            }
                        });
                    }
                });
            })),
        );
    }));

    let renderer_resize = Arc::clone(&renderer);
    window.on_resize(Box::new(move |size, scale_factor| {
        let apply_size = size;
        let renderer_resize = Arc::clone(&renderer_resize);
        let _ = dispatch_platform_realm(
            realm_dispatch,
            RealmEvent::Resize {
                size,
                scale_factor,
                apply_surface: Box::new(move || {
                    if let Some(renderer) = renderer_resize.lock().as_mut() {
                        let width = (apply_size.width.0 * scale_factor) as u32;
                        let height = (apply_size.height.0 * scale_factor) as u32;
                        renderer.resize(width, height);
                    }
                }),
            },
        );
    }));

    // 6. Lifecycle callbacks
    //
    // ADR-0035 (PR1): Started/Terminating call the canonical
    // `flui_scheduler::Scheduler` lifecycle directly — see `run_desktop`'s
    // identical comment for why this bypasses `RealmEvent`/
    // `dispatch_platform_realm` and why the debug_assert lives here rather
    // than in the scheduler.
    platform.on_quit(Box::new(move || {
        tracing::info!("Web platform quit");
        debug_assert_eq!(
            std::thread::current().id(),
            realm_dispatch.owner_thread,
            "platform on_quit must fire on the realm's owner thread"
        );
        Scheduler::instance().handle_app_lifecycle_state_change(AppLifecycleState::Detached);
    }));

    window.on_close(Box::new(move || {
        tracing::info!("Canvas window closed");
        // On web, no explicit quit mechanism needed
    }));

    // No `on_visibility_status_change` registration on web (yet): there is
    // no occlusion signal wired for this backend in this PR (winit's
    // `Occluded` is desktop-only) — a DOM `visibilitychange` listener is a
    // future follow-up, not this PR's scope.
    window.on_active_status_change(Box::new(move |focused| {
        let _ = dispatch_platform_realm(realm_dispatch, RealmEvent::WindowFocus(focused));
    }));

    // 7. Store window — BEFORE marking the lifecycle Resumed, which can
    // synchronously run the first frame through `dispatch_platform_realm`;
    // anything resolving `active_window` during that frame (an autofocus
    // `EditableText` attaching its IME client, for instance) must not see
    // `None`.
    AppBinding::instance().set_shared_window(window);

    debug_assert_eq!(
        std::thread::current().id(),
        realm_dispatch.owner_thread,
        "web bootstrap must run on the realm's owner thread"
    );
    Scheduler::instance().handle_app_lifecycle_state_change(AppLifecycleState::Resumed);

    tracing::info!("Web platform initialized with callbacks");

    // Run the event loop (takes ownership of the platform)
    platform.run(Box::new(|_platform| {
        tracing::info!("Web platform ready");
    }));
    // `WebPlatform::run` installs the RAF callback and returns immediately;
    // tearing down here would destroy the realm before the first frame. The
    // host therefore remains owner-TLS resident for the page lifetime. An
    // explicit web detach/quit ownership hook is deferred until the platform
    // exposes a callback whose lifetime encloses the RAF registration.
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use flui_types::geometry::px;
    use flui_view::{BuildContext, IntoView, View, ViewExt};

    use super::*;

    /// Trivial leaf fixture: an empty view used as the terminal node under
    /// `OwnerLocalRoot` below, and constructible on its own wherever a test
    /// needs a minimal `View + StatelessView` root.
    #[derive(Clone)]
    struct TestView;

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            TestView.boxed()
        }
    }

    impl View for TestView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    #[derive(Clone)]
    struct OwnerLocalRoot {
        value: Rc<Cell<usize>>,
    }

    impl StatelessView for OwnerLocalRoot {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.value.set(self.value.get() + 1);
            TestView.boxed()
        }
    }

    impl View for OwnerLocalRoot {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    #[test]
    fn runner_entrypoints_accept_owner_local_root_state() {
        static_assertions::assert_not_impl_any!(OwnerLocalRoot: Send, Sync);

        std::hint::black_box(run_app_impl::<OwnerLocalRoot> as fn(OwnerLocalRoot));
        std::hint::black_box(
            run_app_with_config_impl::<OwnerLocalRoot> as fn(OwnerLocalRoot, AppConfig),
        );
    }

    #[test]
    fn test_config_creation() {
        let config = AppConfig::new().with_title("Test").with_size(800, 600);

        assert_eq!(config.title, "Test");
        assert_eq!(config.size.width, px(800.0));
    }

    /// Serializes tests that read/write `AppBinding::instance()`'s active
    /// window (the repo rule for tests mutating shared binding state —
    /// AGENTS.md "Testing quirks"). nextest gives each test its own process;
    /// `cargo test` (also a stated gate for this crate) runs them on threads
    /// in one process, where two tests each setting the singleton's window
    /// could interleave.
    static SINGLETON_WINDOW_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Bootstrap ordering invariant shared by `bootstrap_desktop`, `run_android`,
    /// and `run_web`: the window must be stored in `AppBinding` before anything
    /// that could synchronously observe `active_window` (the initial redraw
    /// request, `Lifecycle::Started`) runs — otherwise the first such observer
    /// (an autofocus `EditableText` attaching its IME client, for instance)
    /// silently sees `None`.
    ///
    /// `bootstrap_desktop`/`run_android`/`run_web` themselves cannot run in a
    /// unit test: each opens its window from inside a live platform event loop
    /// (`ActiveEventLoop` is unreachable outside `Platform::run`) and creates a
    /// real GPU `Renderer`, gated behind the separate `enable-wgpu-tests` CI job
    /// (WARP), not this one. This instead drives the exact ordering invariant
    /// headlessly: `HeadlessWindow::request_redraw` (flui-platform's headless
    /// backend, used elsewhere in this crate's tests) dispatches its
    /// `on_request_frame` callback SYNCHRONOUSLY — unlike a real winit window,
    /// where a queued `RedrawRequested` would not fire until `on_ready` (and
    /// this reordering) has already returned. That is exactly why the ordering
    /// bug was invisible in a real window's actual first frame but is directly
    /// observable here.
    ///
    /// Checks a unique window *size* rather than mere `is_some()`, so this
    /// cannot pass merely because an earlier test left SOME window installed
    /// on the singleton — only THIS test's window, with THIS test's
    /// unmistakable marker size, proves `set_window` ran before the callback.
    ///
    /// Red-check: swap the order of the two `AppBinding::instance()` calls
    /// below (request the redraw, then store the window — the pre-fix shape)
    /// and this fails: `wake_frame` finds no active window yet, never calls
    /// `request_redraw` on it, and the callback never fires at all.
    #[test]
    fn desktop_bootstrap_stores_the_window_before_the_first_synchronous_redraw_observes_it() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let _serialized = SINGLETON_WINDOW_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let marker_size = flui_types::Size::new(px(4001.0), px(4002.0));

        let platform = flui_platform::headless_platform();
        let window = platform
            .open_window(flui_platform::traits::WindowOptions {
                size: marker_size,
                ..Default::default()
            })
            .expect("headless platform always opens a window");

        // `on_request_frame` requires `Send` on the callback; `AppBinding` is
        // not `Send` (owner-thread-affine gesture arena state — ADR-0027), so
        // the closure below cannot capture a specific `&AppBinding`/`Arc`.
        // Resolving `AppBinding::instance()` fresh inside the closure (zero
        // captures for the binding itself) sidesteps that entirely — the same
        // pattern the production scheduler wake hook (`AppBinding::new`) uses
        // to avoid capturing one specific instance.
        //
        // Reads through `with_window`, NOT `wake_frame`/`request_redraw`: a
        // headless window's `request_redraw` dispatches this very callback
        // synchronously, so calling anything that re-locks `active_window`
        // from in here (the two are on the same thread, same call stack)
        // would deadlock on `AppBinding`'s own non-reentrant lock.
        let saw_marker_window = Arc::new(AtomicBool::new(false));
        let saw_marker_window_cb = Arc::clone(&saw_marker_window);
        window.on_request_frame(Box::new(move || {
            let matches_marker = AppBinding::instance()
                .with_window(|w| w.bounds().size == marker_size)
                .unwrap_or(false);
            saw_marker_window_cb.store(matches_marker, Ordering::SeqCst);
        }));

        // Mirrors the FIXED order in `bootstrap_desktop`/`run_android`:
        // store the window BEFORE requesting the initial redraw. `wake_frame`
        // (not `with_window(|w| w.request_redraw())`) clones the window out
        // from under the lock before calling through, so this call cannot
        // deadlock against the callback's own `with_window` re-entry above —
        // see `wake_frame`'s doc and `bootstrap_desktop`'s matching comment.
        AppBinding::instance().set_window(window);
        AppBinding::instance().wake_frame();

        assert!(
            saw_marker_window.load(Ordering::SeqCst),
            "set_window must have taken effect before the initial redraw fires \
             the frame callback that could read active_window",
        );
    }
}
