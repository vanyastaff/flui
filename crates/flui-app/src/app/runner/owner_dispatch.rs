use std::collections::VecDeque;

use flui_foundation::UiRuntimeId;
use flui_scheduler::AppLifecycleState;

use super::host::APP_RUNTIME;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use super::secondary_window::drain_pending_secondary_window_completions;
use crate::app::lifecycle_state::preserve_first_lifecycle_panic;
use crate::app::runtime::RuntimeSlot;

/// A registration-lifetime renderer-surface applier: `FnMut(size,
/// scale_factor)`. Named so [`RuntimeSlot`]'s `surface_applier` field
/// declaration reads plainly instead of spelling out the boxed closure type
/// inline. `pub(in crate::app)` (rather than private) so [`RuntimeSlot`]'s struct
/// definition in the sibling `runtime` module can name this type.
pub(in crate::app) type SurfaceApplier = Box<dyn FnMut(flui_foundation::geometry::Size<f64>, f64)>;

/// Restores a taken [`SurfaceApplier`] back into its UI runtime's slot in
/// [`APP_RUNTIME`]'s registry when dropped — including during an unwinding
/// drop, so a panic inside the applier's own call (caught by
/// `dispatch_platform_ui_runtime`'s outer `catch_unwind`) cannot permanently
/// strand resizing. Without this, the applier taken out before the call is
/// simply never restored once the call panics, and every later `Resized`
/// event for that UI runtime finds the slot empty forever, silently coalescing at
/// the `None` arm's trace instead of ever applying again.
///
/// Addressed by [`UiRuntimeId`] (not the old bare `Option`): if the UI runtime was
/// torn down while the applier's own call was still running, restoring into
/// a now-missing slot is a silent no-op, matching this file's existing "the
/// UI runtime may be gone by the time a destructor runs" discipline.
#[must_use = "dropping this immediately restores the applier with no call in between"]
struct SurfaceApplierRestoreGuard {
    ui_runtime_id: UiRuntimeId,
    applier: Option<SurfaceApplier>,
}

impl SurfaceApplierRestoreGuard {
    fn call(&mut self, size: flui_foundation::geometry::Size<f64>, scale_factor: f64) {
        if let Some(applier) = self.applier.as_mut() {
            applier(size, scale_factor);
        }
    }
}

impl Drop for SurfaceApplierRestoreGuard {
    fn drop(&mut self) {
        if let Some(applier) = self.applier.take() {
            let ui_runtime_id = self.ui_runtime_id;
            APP_RUNTIME.with(|slot| {
                if let Some(ui_runtime_slot) = slot.borrow_mut().ui_runtimes.get_mut(&ui_runtime_id)
                {
                    ui_runtime_slot.surface_applier = Some(applier);
                }
            });
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct PresentationDispatcher {
    pub(super) owner_thread: std::thread::ThreadId,
    pub(super) address: flui_foundation::PresentationAddress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DispatchError {
    WrongThread,
    /// The UI runtime incarnation this dispatcher was minted for is gone — the
    /// common path: `ui_runtime_id`/`presentation_id` mint from one shared
    /// counter, so teardown+reinstall always changes both, and this check
    /// (UI runtime first) catches it before the presentation half is even
    /// compared.
    StaleRuntime,
    /// The UI runtime is live and matches, but the presentation incarnation does
    /// not — reachable today only via a forged/mixed address (a dispatcher
    /// whose presentation half was swapped for another incarnation's), and,
    /// once one UI runtime can host more than one presentation, via real
    /// presentation replacement within a live ui_runtime. Kept as its own
    /// variant now: the design-for-N contract, not dead code.
    StalePresentation,
    /// A terminal close for this exact presentation incarnation was already
    /// accepted. Later work must not jump the deferred close at a bounded
    /// batch boundary.
    PresentationClosing,
    RuntimeUnavailable,
}

/// Maximum logical owner operations in one continuation callback. Fresh
/// native roots and deferred FIFO entries share this budget; an individual
/// operation is cooperative and is never preempted internally.
const OWNER_TURN_BUDGET: usize = 32;

/// Typed observations applied on a UI runtime's owner turn: native input and
/// lifecycle, host font changes, and surface restoration. The payload is
/// `Send` (ADR-0037 §3); execution still goes through exact-address admission
/// on the owner thread, never through an arbitrary cross-thread callback.
// `pub(in crate::app)` because `RuntimeTask::Event` (also `pub(in crate::app)`, for
// `AppRuntime`'s sake) carries this type in a field the compiler considers
// reachable at that same visibility.
// The window-event variants (`WindowFocus`, `WindowVisibility`,
// `AppearanceChanged`, `WindowHover`) are produced only by the desktop
// runner's `on_window_event` wiring; the mobile and web runners drive their
// lifecycle from platform callbacks instead, so those variants are
// unconstructed there.
#[cfg_attr(
    all(
        not(test),
        // Only android and iOS drop the window-event variants: the web runner
        // constructs `WindowFocus`/`WindowHover` through the browser's
        // visibility/focus signals, so on wasm32 they are live.
        any(target_os = "android", target_os = "ios")
    ),
    expect(
        dead_code,
        reason = "window-event variants are produced only by the desktop runner"
    )
)]
pub(in crate::app) enum RuntimeEvent {
    /// Platform input for the stamped presentation. `consumed`, present only
    /// for keyboard input, receives whether a handler took it once it runs;
    /// see [`dispatch_platform_input`].
    Input {
        input: flui_platform::traits::PlatformInput,
        consumed: Option<std::sync::Arc<std::sync::OnceLock<bool>>>,
    },
    Resized {
        size: flui_foundation::geometry::Size<f64>,
        scale_factor: f64,
    },
    /// Window focus changed (winit's `WindowEvent::Focused`, or the
    /// equivalent per-backend signal; same source as the deleted `Active`
    /// variant this one replaces). Feeds the `(visible, focused)` ->
    /// `AppLifecycleState` derivation below, alongside
    /// [`WindowVisibility`](Self::WindowVisibility).
    WindowFocus(bool),
    /// One atomic observation after callback registration. Keep this lossless:
    /// a suspended or unfocused snapshot can cancel active input sequences.
    #[cfg_attr(
        all(not(test), any(target_arch = "wasm32", target_os = "android")),
        expect(
            dead_code,
            reason = "desktop and UIKit seed batched window observations"
        )
    )]
    WindowSnapshot {
        execution: flui_platform::WindowExecutionState,
        focused: bool,
        visible: bool,
    },
    /// Reversible native execution eligibility for one presentation.
    #[cfg_attr(
        any(target_arch = "wasm32", target_os = "android"),
        expect(
            dead_code,
            reason = "these runners retain their existing host lifecycle transport"
        )
    )]
    WindowExecution(flui_platform::WindowExecutionState),
    /// Addressed logical content-view safe area.
    ///
    /// The iOS runner is the only producer; this module's own tests construct
    /// it directly to pin the addressed-write contract (a report for a
    /// presentation closed before delivery is dropped, not a panic).
    #[cfg_attr(
        not(target_os = "ios"),
        expect(
            dead_code,
            reason = "safe-area reports are produced only by the UIKit runner"
        )
    )]
    SafeAreaChanged(flui_foundation::geometry::EdgeInsets),
    /// Window visibility/occlusion changed (winit's `WindowEvent::Occluded`,
    /// negated — see `PlatformWindow::on_visibility_status_change`).
    ///
    /// Combined with [`WindowFocus`](Self::WindowFocus) via
    /// the addressed presentation's lifecycle reconciliation. The UI runtime
    /// scheduler derives its aggregate from all live presentations.
    // Not yet constructed on wasm32: `run_web` only wires `WindowFocus` —
    // no occlusion signal for the web backend yet (see run_web's comment at
    // its `on_active_status_change` registration).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    WindowVisibility(bool),
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    SynchronizeLifecycle,
    /// The OS light/dark appearance changed (winit's `ThemeChanged`, or the
    /// equivalent per-backend signal). Republishes
    /// `MediaQueryData::platform_brightness` through the root media query.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    AppearanceChanged(flui_platform::WindowAppearance),
    /// The pointer entered (`true`) or left (`false`) the window (winit's
    /// `CursorEntered`/`CursorLeft`, via
    /// `PlatformWindow::on_hover_status_change`). Leave sweeps the addressed
    /// presentation's hover state — `MouseRegion::on_exit` fires and the
    /// cursor resets; without this a widget hovered at the moment the cursor
    /// crosses the window edge keeps its hover visuals forever. Enter is a
    /// no-op today: the next `CursorMoved` re-primes hover from a fresh hit
    /// test on its own.
    // Constructed by both the desktop and web bootstraps' registrations, so
    // every backend whose `PlatformWindow` dispatches
    // `on_hover_status_change` routes here; a backend that never fires the
    // callback simply never constructs the event.
    WindowHover(bool),
    /// Drive a lifecycle target that requires owner-local UI runtime cleanup (most
    /// notably Detached during platform shutdown).
    Lifecycle(AppLifecycleState),
    /// The host's shared font collection changed; invalidate every presentation.
    FontsChanged,
    /// A recreated native surface has no previous scene to display. Surface
    /// recovery still belongs to the primary presentation, not every sibling.
    #[cfg(any(test, target_os = "android", target_os = "ios"))]
    PrimarySurfaceRestored,
}

/// One queued unit of owner-thread work. Shared-UI runtime notifications use
/// typed operations; arbitrary shared-UI runtime callbacks are test-only.
/// Events carry observations; pumps own the host's frame execution protocol.
/// This enum never crosses a thread (ADR-0037 §3).
///
/// `Frame` addresses an installed driver. Like `ClosePresentation` it needs
/// `&mut UiRuntime` — [`UiRuntime::pump`](crate::app::ui_runtime::UiRuntime::pump)
/// takes the UI runtime exclusively and enters it itself — so the drain loop runs
/// it on the checked-out UI runtime without entering it first; the driver enters
/// the UI runtime explicitly for its backend gate and prelude.
pub(in crate::app) enum RuntimeTask {
    Event(RuntimeEvent),
    #[cfg(test)]
    TestCallback(Box<dyn FnOnce(&crate::app::ui_runtime::UiRuntime)>),
    Frame(super::frame_driver::FrameBinding),
    /// Commit the owner inbox, then poll async work without running a frame.
    #[cfg(any(test, target_os = "ios"))]
    BackgroundPump,
    ClosePresentation(flui_foundation::PresentationId),
}

impl RuntimeTask {
    /// Applies a typed notification against the UI runtime's shared capabilities.
    ///
    /// `presentation_id` is the [`flui_foundation::PresentationId`] the
    /// enqueueing [`PresentationDispatcher`] was addressed to — stamped onto this
    /// task's queue entry at enqueue time (`dispatch_platform_ui_runtime`), since
    /// a UI runtime's queue is shared across every presentation it hosts. Font
    /// notifications invalidate the whole UI runtime; surface recovery still
    /// repaints its primary. Only `Self::Event` threads the address through
    /// to [`RuntimeEvent::run`].
    ///
    /// # Panics
    /// Panics if called with `Self::ClosePresentation` — that variant never
    /// reaches this method: [`dispatch_platform_ui_runtime`]'s drain loop matches
    /// it out before calling `run`, since it needs `&mut UiRuntime` instead of
    /// this method's `&UiRuntime` receiver. Not reachable from any other
    /// caller — `run` has exactly one call site.
    fn run(
        self,
        ui_runtime: &crate::app::ui_runtime::UiRuntime,
        presentation_id: flui_foundation::PresentationId,
    ) {
        match self {
            Self::Event(event) => event.run(ui_runtime, presentation_id),
            #[cfg(test)]
            Self::TestCallback(run) => run(ui_runtime),
            #[cfg(any(test, target_os = "ios"))]
            Self::BackgroundPump => unreachable!(
                "BUG: background pumps require exclusive ui_runtime access in the dispatcher"
            ),
            Self::Frame(_) => unreachable!(
                "BUG: RuntimeTask::Frame reached RuntimeTask::run -- dispatch_platform_ui_runtime's drain \
                 loop must match this variant out before calling run, so it can hand the pump \
                 &mut UiRuntime instead"
            ),
            Self::ClosePresentation(id) => unreachable!(
                "BUG: RuntimeTask::ClosePresentation({id:?}) reached RuntimeTask::run -- \
                 dispatch_platform_ui_runtime's drain loop must match this variant out before \
                 calling run, so it can call close_presentation_entered with &mut UiRuntime \
                 instead"
            ),
        }
    }
}

impl RuntimeEvent {
    /// `presentation_id` is the exact presentation this event was stamped
    /// for at enqueue time (see [`RuntimeTask::run`]'s doc). Native input,
    /// metrics and window observations use that presentation. Font changes
    /// invalidate every presentation; surface restoration still invalidates
    /// only the primary, which owns the UI runtime's current sink. Dispatch checks
    /// the stamped address before applying any of these observations.
    fn run(
        self,
        ui_runtime: &crate::app::ui_runtime::UiRuntime,
        presentation_id: flui_foundation::PresentationId,
    ) {
        match self {
            Self::FontsChanged => ui_runtime.fonts_changed(),
            #[cfg(any(test, target_os = "android", target_os = "ios"))]
            Self::PrimarySurfaceRestored => ui_runtime.mark_primary_needs_full_repaint(),
            Self::Input { input, consumed } => {
                let handled = ui_runtime.handle_input_addressed(presentation_id, input);
                if let Some(consumed) = consumed {
                    let _ = consumed.set(handled);
                }
            }
            Self::Resized { size, scale_factor } => {
                // Take the applier out of THIS ui_runtime's slot, release the
                // borrow, call it, then restore it — never call through a
                // live borrow, so a reentrant TLS access from inside the
                // applier (e.g. a nested dispatch enqueuing further work)
                // cannot hit an already-mutably-borrowed `RefCell` panic. If
                // the slot is ever found empty here (no applier installed
                // yet, or already cleared by teardown) this skips with a
                // trace instead of unwrapping/panicking; surface application
                // then coalesces onto the next real applier install.
                //
                // The ui_runtime has one surface applier and one frame sink, both
                // the window's that installed them (`surface_owner`). A
                // resize of any other presentation — a `WindowPolicy::Shared`
                // secondary — must not reach them: the primary's surface
                // would take the secondary's size and its next frame would be
                // laid out at it. That resize stays addressed to its own
                // ratio and media query below; its surface waits for
                // per-presentation sinks (#559).
                let ui_runtime_id = ui_runtime.id();
                let (owns_surface, applier) = APP_RUNTIME.with(|slot| {
                    let mut state = slot.borrow_mut();
                    let Some(ui_runtime_slot) = state.ui_runtimes.get_mut(&ui_runtime_id) else {
                        return (true, None);
                    };
                    if ui_runtime_slot
                        .surface_owner
                        .is_some_and(|owner| owner != presentation_id)
                    {
                        return (false, None);
                    }
                    (true, ui_runtime_slot.surface_applier.take())
                });
                match applier {
                    None if !owns_surface => {
                        tracing::trace!(
                            ?presentation_id,
                            "ui_runtime resize: presentation does not own the ui_runtime's surface; \
                             surface left alone"
                        );
                    }
                    Some(applier) => {
                        // The guard restores the applier on drop
                        // unconditionally — including if `call` below
                        // panics and the drop runs during unwind — so a
                        // caught panic in the applier never permanently
                        // strands resizing.
                        let mut guard = SurfaceApplierRestoreGuard {
                            ui_runtime_id,
                            applier: Some(applier),
                        };
                        guard.call(size, scale_factor);
                    }
                    None => {
                        tracing::debug!(
                            "ui_runtime resize: surface applier slot is empty; surface application \
                             coalesces onto the next real applier install"
                        );
                    }
                }
                // Addressed writes: the ratio and the media query belong to
                // the window that reported them (windows on monitors with
                // different scales keep their own), and both are dropped when
                // the presentation this resize was stamped for is gone by
                // delivery time — see `UiRuntime::media_query_for`. The redraw
                // below is ui_runtime-wide and runs
                // either way.
                ui_runtime.set_device_pixel_ratio_for(presentation_id, scale_factor);
                if let Some(source) = ui_runtime.media_query_for(presentation_id) {
                    source.update(|data| {
                        data.size = size;
                        data.device_pixel_ratio = scale_factor;
                    });
                } else {
                    tracing::debug!(
                        ?presentation_id,
                        "ui_runtime resize: addressed presentation is gone; no media query to resize"
                    );
                }
                ui_runtime.request_redraw();
                tracing::trace!(?size, scale_factor, "ui_runtime resize committed");
            }
            Self::SafeAreaChanged(insets) => {
                if let Some(source) = ui_runtime.media_query_for(presentation_id) {
                    source.update(|data| data.padding = insets);
                } else {
                    tracing::debug!(
                        ?presentation_id,
                        "safe-area report: addressed presentation is gone; no media query to pad"
                    );
                }
                ui_runtime.request_redraw();
            }
            Self::WindowFocus(focused) => ui_runtime.update_window_focus(presentation_id, focused),
            Self::WindowSnapshot {
                execution,
                focused,
                visible,
            } => {
                ui_runtime.synchronize_window_snapshot(
                    presentation_id,
                    execution,
                    focused,
                    visible,
                );
            }
            Self::WindowExecution(state) => {
                ui_runtime.update_window_execution(presentation_id, state);
            }
            Self::WindowHover(inside) => {
                ui_runtime.handle_window_hover_addressed(presentation_id, inside);
            }
            Self::AppearanceChanged(appearance) => {
                use flui_platform::WindowAppearance;
                let brightness = match appearance {
                    WindowAppearance::Dark | WindowAppearance::VibrantDark => {
                        flui_platform_api::Brightness::Dark
                    }
                    WindowAppearance::Light | WindowAppearance::VibrantLight => {
                        flui_platform_api::Brightness::Light
                    }
                };
                if let Some(source) = ui_runtime.media_query_for(presentation_id) {
                    source.update(|data| {
                        data.platform_brightness = brightness;
                    });
                } else {
                    tracing::debug!(
                        ?presentation_id,
                        "appearance change: addressed presentation is gone; no media query to \
                         brighten"
                    );
                }
                ui_runtime.request_redraw();
            }
            Self::WindowVisibility(visible) => {
                ui_runtime.update_window_visibility(presentation_id, visible);
            }
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            Self::SynchronizeLifecycle => ui_runtime.synchronize_window_lifecycle(),
            Self::Lifecycle(new) => {
                #[cfg(all(
                    not(target_os = "android"),
                    not(target_os = "ios"),
                    not(target_arch = "wasm32")
                ))]
                APP_RUNTIME.with(|slot| slot.borrow_mut().main_host_lifecycle = new);
                ui_runtime.update_host_lifecycle(new);
            }
        }
    }
}

/// Installs `applier` as `ui_runtime_id`'s registration-lifetime renderer-surface
/// applier, replacing (never stacking) any previously-installed one for that
/// SAME UI runtime — a sibling UI runtime's own applier is untouched. Call once per
/// UI runtime install, alongside `install_platform_ui_runtime` (Android/web; the desktop
/// and iOS bootstraps use [`install_ui_runtime_alongside`]), from each backend's
/// bootstrap — never from inside a frame/event dispatch.
///
/// `ui_runtime_id` not being resident here is always a caller bug, never a
/// legitimate race: this is called synchronously, immediately alongside the
/// UI runtime's own install, so the slot must already exist. Loud rather than a
/// silent no-op, because the failure mode otherwise is silent forever — that
/// UI runtime's `Resized` events would coalesce onto a `None` applier for its
/// entire lifetime with nothing ever pointing at why.
pub(super) fn install_surface_applier(
    ui_runtime_id: UiRuntimeId,
    applier: impl FnMut(flui_foundation::geometry::Size<f64>, f64) + 'static,
) {
    APP_RUNTIME.with(|slot| {
        if let Some(ui_runtime_slot) = slot.borrow_mut().ui_runtimes.get_mut(&ui_runtime_id) {
            ui_runtime_slot.surface_applier = Some(Box::new(applier));
            ui_runtime_slot.surface_owner = Some(ui_runtime_slot.address.presentation_id);
        } else {
            debug_assert!(
                false,
                "BUG: install_surface_applier called for ui_runtime {ui_runtime_id:?}, which is not \
                 resident in the registry -- call this once, synchronously, immediately \
                 alongside install_platform_ui_runtime/install_ui_runtime_alongside, never after"
            );
            tracing::error!(
                ?ui_runtime_id,
                "install_surface_applier: ui_runtime not found in the registry -- this ui_runtime's \
                 resize handling is silently lost for its whole lifetime"
            );
        }
    });
}

/// Installs `ui_runtime` as the SOLE hosted UI runtime on this thread, minting its
/// dispatcher's address by registering `window` in the single
/// [`crate::app::window_registry::WindowRegistry`] authority — the registry is
/// the sole mint path for a routable [`flui_foundation::PresentationAddress`];
/// no caller of this function ever names the platform-internal native-handle
/// key type itself.
///
/// This is the legacy single-primary-UI runtime entry point every backend's
/// bootstrap still calls exactly once: it clears the ENTIRE UI runtime registry
/// (every hosted UI runtime, not only one) before inserting the fresh one —
/// exactly the behavior the single `Option<UiRuntime>` slot this registry
/// replaces used to have, since that slot could only ever hold one UI runtime at
/// all. A second, non-displacing UI runtime (a genuinely independent window
/// alongside this one) is installed through
/// [`install_ui_runtime_alongside`] instead.
#[cfg(any(test, target_os = "android", target_arch = "wasm32"))]
pub(super) fn install_platform_ui_runtime(
    ui_runtime: crate::app::ui_runtime::UiRuntime,
    window: &std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) -> PresentationDispatcher {
    let owner_thread = std::thread::current().id();
    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: ui_runtime.id(),
        presentation_id: ui_runtime.presentation_id(),
    };
    let (displaced, stale_owner_turns) = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.frame_drivers.retire_all();
        // Every ui_runtime hosted here may already be installed — a reinstall
        // without an intervening `teardown_platform_ui_runtime` (the
        // panic-recovery path: a mid-`on_ready` failure leaves the old
        // registry/queue/applier/window-registry mappings in place, and
        // bootstrap tries again on the same thread). Remove every registry
        // mapping addressed to EACH displaced ui_runtime — not just the window
        // being installed now — in this same borrow, before registering the
        // new window: otherwise a displaced ui_runtime's own window(s) survive as
        // dead entries no later teardown ever reaches (this legacy entry
        // point is the only one that clears the whole registry at once).
        let mut removed_window_mappings = 0;
        let displaced = state.ui_runtimes.clear();
        // A panicking old-incarnation task can leave later owner turns in
        // the global FIFO after its drain guard releases. They are addressed
        // to the registry being replaced here, so retain neither their stale
        // work nor captures into the fresh loop. Return them beside the
        // displaced slots so arbitrary capture destructors run only after
        // this TLS borrow has ended.
        let stale_owner_turns = std::mem::take(&mut state.owner_turn_queue);
        state.owner_turn_continuation = None;
        state.owner_turn_continuation_failed = false;
        state.owner_turn_callback_budget = None;
        state.owner_turn_callback_active = false;
        state.owner_turn_draining = false;
        state.closing_presentations.clear();
        for (displaced_id, _) in &displaced {
            removed_window_mappings += state.registry.remove_ui_runtime(*displaced_id).len();
        }
        state.registry.register_window(window, address);

        if !displaced.is_empty() {
            tracing::warn!(
                displaced_ui_runtimes = displaced.len(),
                new_address = ?address,
                removed_window_mappings,
                "install_platform_ui_runtime: replacing ui_runtime(s) that were never torn down"
            );
        }
        state.ui_runtimes.insert(
            address.ui_runtime_id,
            RuntimeSlot {
                ui_runtime: Some(ui_runtime),
                queue: VecDeque::new(),
                draining: false,
                address,
                surface_applier: None,
                surface_owner: None,
            },
        );
        state.owner_thread = Some(owner_thread);
        // Defensive: a reinstall-without-teardown only reaches this path
        // when the displaced incarnation's own dispatch never restored its
        // slot's `ui_runtime` (the panic-recovery scenario this function's doc
        // already documents) — `dispatched_scheduler`/`dispatched_ui_runtime_id`,
        // if the displaced incarnation left either stashed, belong to that
        // dead incarnation and must not leak into the fresh one's fence-(c)
        // reads.
        state.dispatched_scheduler = None;
        state.dispatched_ui_runtime_id = None;
        // Explicit, known-point resolution: a ui_runtime is actually being
        // installed, so this thread genuinely needs `SharedEngineServices`
        // -- unlike `install_owner_platform`, which every backend calls
        // (including `run_direct`, which never installs a ui_runtime and never
        // needs these services). Idempotent (`ensure_services` caches), so
        // it does not matter whether a prior ui_runtime on this thread already
        // triggered it.
        let _ = state.ensure_services();
        // Same known-point discipline for the loop-scoped execution
        // services (issue #557): resolved here (host-injected if the
        // bootstrap stashed `AppConfig::executors`, default pools
        // otherwise), never ambiently. Cheap — default pools start worker
        // threads on first background spawn, not here.
        let _ = state.ensure_execution();
        // And for the service registry (issue #558): a PRIOR loop's
        // teardown closed its admission; this loop hosting a ui_runtime reopens
        // it so config-declared services can start. Running services are
        // untouched — mid-loop reinstalls (hot-restart, panic recovery)
        // find admission already open and their services still owned.
        #[cfg(not(target_arch = "wasm32"))]
        state.reopen_lifecycles();
        (displaced, stale_owner_turns)
    });
    // Destructors may re-enter platform/framework code (the same invariant
    // `teardown_platform_ui_runtime` honors) — drop only after the TLS borrow
    // above has released.
    let mut first_panic = None;
    drop_removed_ui_runtimes(
        displaced.into_iter().map(|(_, slot)| slot).collect(),
        &mut first_panic,
    );
    drop_queued_turns(stale_owner_turns, &mut first_panic);
    let retirement = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.frame_drivers.sweep_retired();
        state.native_retirement.clone()
    });
    retirement.drain(&mut first_panic);
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
    PresentationDispatcher {
        owner_thread,
        address,
    }
}

/// Installs `ui_runtime` ALONGSIDE whatever is already hosted, never displacing a
/// sibling — the multi-UI runtime counterpart to `install_platform_ui_runtime`'s
/// (Android/web-only, hence not linked)
/// legacy single-primary-UI runtime replace semantics. Requests window
/// registration and the registry insertion TOGETHER, through
/// [`crate::app::runtime::AppRuntime::request_ui_runtime_install`] (never registers
/// the window separately/eagerly — see that method's own doc for the gap a
/// two-step sequence would leave open), so an install requested while
/// another UI runtime is checked out for dispatch (a frame callback opening a
/// second window) defers to loop idle instead of installing mid-dispatch.
///
/// `Err(RegistryError::WindowAlreadyMapped)` only when applied immediately
/// (no dispatch/visit in flight) and `window`'s id already maps to a live
/// entry — refused, never silently re-routed onto whichever sibling UI runtime
/// already owns that id (`WindowRegistry::register_window`'s replace
/// semantics is exactly the wrong tool for two UI runtimes meant to coexist). A
/// deferred install that later collides is traced and dropped instead (see
/// `AppRuntime::drain_pending_ui_runtime_mutations`), since the caller has
/// already returned by the time a deferred mutation applies.
///
/// Production caller: [`open_secondary_window`](super::secondary_window::open_secondary_window) under
/// [`crate::app::runtime::WindowPolicy::Isolated`] — the embedder-facing
/// seam issue #555 adds. Also exercised directly by this module's own
/// tests.
///
#[cfg_attr(
    not(any(test, all(not(target_os = "android"), not(target_arch = "wasm32")))),
    expect(
        dead_code,
        reason = "open_secondary_window (its production caller) is desktop-only -- android/wasm32 \
                  have no caller outside this module's own tests"
    )
)]
pub(super) fn install_ui_runtime_alongside(
    ui_runtime: crate::app::ui_runtime::UiRuntime,
    window: &std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
) -> Result<PresentationDispatcher, crate::app::window_registry::RegistryError> {
    let owner_thread = std::thread::current().id();
    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: ui_runtime.id(),
        presentation_id: ui_runtime.presentation_id(),
    };
    let window = std::sync::Arc::clone(window);
    let result = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.owner_thread.get_or_insert(owner_thread);
        let _ = state.ensure_services();
        state.request_ui_runtime_install(
            address.ui_runtime_id,
            RuntimeSlot {
                ui_runtime: Some(ui_runtime),
                queue: VecDeque::new(),
                draining: false,
                address,
                surface_applier: None,
                surface_owner: None,
            },
            window,
        )
    });
    // On a refused collision, `result` carries the rejected `RuntimeSlot` (and
    // the `UiRuntime` it owns) back out of the TLS borrow above -- dropped
    // only here, after that borrow has released, never inside it (see
    // `AppRuntime::apply_install`'s own doc for why).
    match result {
        Ok(()) => Ok(PresentationDispatcher {
            owner_thread,
            address,
        }),
        Err((error, rejected_slot)) => {
            drop(rejected_slot);
            Err(error)
        }
    }
}

/// Errors from [`install_presentation_alongside`]. A dedicated type rather
/// than folding into [`DispatchError`]: none of that enum's variants
/// mean "the UI runtime is fine, but this specific window id collided" —
/// mislabeling that as `RuntimeUnavailable` (the pre-fix shape) told a caller
/// the wrong thing about what actually went wrong.
#[cfg_attr(
    not(any(
        test,
        all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        )
    )),
    expect(
        dead_code,
        reason = "install_presentation_alongside (its only producer) is desktop-only -- \
                  android/wasm32 have no caller outside this module's own tests"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum InstallPresentationError {
    /// `dispatcher`'s UI runtime no longer exists (a newer UI runtime replaced it, or
    /// it was already torn down).
    #[error("the ui_runtime this dispatcher was minted for no longer exists")]
    RuntimeUnavailable,
    /// A dispatch or hot-restart visit is currently in flight on this
    /// thread; see this function's own doc for why that is a named,
    /// stated gap rather than a defer-to-idle path.
    #[error("a dispatch or hot-restart visit is in flight on this thread")]
    DispatchInFlight,
    /// `dispatcher`'s UI runtime is live, but `dispatcher.address` itself is no
    /// longer registered in `WindowRegistry` — the presentation it was
    /// minted for closed since, even though a DIFFERENT presentation kept
    /// the UI runtime alive. The same authorization check
    /// `dispatch_platform_ui_runtime` runs (`registry.contains_address`), applied
    /// here too: a caller must hold a dispatcher whose exact address is
    /// CURRENTLY live to authorize installing another presentation
    /// alongside it, not merely one whose UI runtime happens to still exist.
    #[error("the presentation this dispatcher was minted for is no longer registered")]
    StalePresentation,
    /// A terminal close for `dispatcher.address` was already admitted but
    /// has not necessarily reached the bounded owner FIFO yet. The address
    /// is still registered during that interval, but it no longer
    /// authorizes expanding the presentation forest: doing so could turn a
    /// sole-presentation close into a partial close after admission.
    #[error("the presentation this dispatcher was minted for is closing")]
    PresentationClosing,
    /// `window`'s id was already registered to a (possibly different)
    /// address — practically unreachable for a freshly opened window, but
    /// a real, distinct failure mode from `RuntimeUnavailable`: the UI runtime
    /// itself is perfectly fine.
    #[error("window is already registered: {0}")]
    WindowAlreadyMapped(#[from] crate::app::window_registry::RegistryError),
}

/// Installs another presentation into `dispatcher`'s UI runtime, alongside
/// whatever it already hosts — the addressed-routing counterpart to
/// [`install_ui_runtime_alongside`] (which installs a second UI runtime instead of a
/// second presentation of the SAME UI runtime). This is the production entry
/// point `flui_runtime`'s `PresentationForest` doc
/// points to: the forest's former `len()<=1` ratchet lifted (issue #555)
/// specifically so this function has somewhere real to install into.
///
/// `window` becomes the fresh presentation's own native window. Ordering is
/// load-bearing: the presentation is
/// [`assembled`](crate::app::ui_runtime::UiRuntime::assemble_presentation) but NOT
/// yet installed into the forest, then its `WindowRegistry` mapping is
/// minted, and ONLY on success is it
/// [`installed`](crate::app::ui_runtime::UiRuntime::install_presentation) — so a
/// registration failure leaves nothing forest-resident to roll back (the
/// assembled-but-uninstalled `PresentationState` is simply dropped), never
/// a presentation the forest holds with no registry entry of its own. This
/// is the single-native-window-map-authority invariant this function
/// exists to uphold: no hosted presentation is ever forest-resident
/// without a mapping.
///
/// # Errors
///
/// [`InstallPresentationError::RuntimeUnavailable`] if `dispatcher`'s UI runtime no
/// longer exists. [`InstallPresentationError::StalePresentation`] if the
/// UI runtime survives but `dispatcher.address` itself is no longer a live
/// registered address (its own presentation closed, even though a sibling
/// kept the UI runtime alive) — the same authorization
/// `dispatch_platform_ui_runtime` requires of every dispatched task, applied to
/// this mutation too. [`InstallPresentationError::PresentationClosing`] if
/// the address is still registered but its terminal close has already been
/// admitted — the close barrier covers forest mutations as well as ordinary
/// tasks. [`InstallPresentationError::WindowAlreadyMapped`] if
/// `window`'s id is somehow already registered (practically unreachable: a
/// freshly opened window has a fresh id by construction) — nothing is
/// installed into the forest in this case.
/// [`InstallPresentationError::DispatchInFlight`] if a dispatch or
/// hot-restart visit is currently in flight on this thread: **named gap,
/// not a silent one** — unlike [`install_ui_runtime_alongside`]/
/// `request_ui_runtime_uninstall`, this path does not yet defer to loop idle
/// through `AppRuntime::pending_ui_runtime_mutations`; [`open_secondary_window`](super::secondary_window::open_secondary_window)
/// (its production caller, under
/// [`crate::app::runtime::WindowPolicy::Shared`]) never calls this from
/// inside a dispatched callback, so there is nothing forcing that
/// generalization today. Extending the deferral queue to
/// presentation-install requests is follow-up work, not silently skipped:
/// this refuses loudly (a `debug_assert!` in debug builds) rather than
/// corrupting `AppRuntime` state.
#[cfg_attr(
    not(any(
        test,
        all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        )
    )),
    expect(
        dead_code,
        reason = "open_secondary_window (its production caller) is desktop-only -- android/wasm32 \
                  have no caller outside this module's own tests"
    )
)]
pub(super) fn install_presentation_alongside(
    dispatcher: PresentationDispatcher,
    window: impl Into<crate::app::presentation::PresentationWindow>,
) -> Result<PresentationDispatcher, InstallPresentationError> {
    let presentation_window = window.into();
    let ui_runtime_id = dispatcher.address.ui_runtime_id;
    let owner_thread = dispatcher.owner_thread;
    APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.dispatched_ui_runtime_id.is_some() || state.iterating_all_ui_runtimes {
            debug_assert!(
                false,
                "BUG: install_presentation_alongside called while a dispatch/hot-restart visit \
                 is in flight -- this path has no defer-to-idle queue yet (a named, stated gap, \
                 not a silent one); call only from outside any dispatched callback"
            );
            tracing::error!(
                ?ui_runtime_id,
                "rejecting install_presentation_alongside while a dispatch is in flight"
            );
            return Err(InstallPresentationError::DispatchInFlight);
        }
        if !state.ui_runtimes.contains_key(&ui_runtime_id) {
            return Err(InstallPresentationError::RuntimeUnavailable);
        }
        // Authorization check (mirrors `dispatch_platform_ui_runtime`'s own):
        // the ui_runtime existing is not enough -- `dispatcher.address` itself
        // must still be a LIVE registered address. A ui_runtime survives its
        // sole non-primary presentation closing (a sibling keeps it
        // alive), so a caller could otherwise still hold a dispatcher for
        // that now-closed presentation and use it to authorize installing
        // yet another one alongside -- checked BEFORE borrowing
        // `state.ui_runtimes` mutably below, exactly like `dispatch_platform_
        // ui_runtime` checks `state.registry` before `state.ui_runtimes.get_mut`.
        if !state.registry.contains_address(dispatcher.address) {
            tracing::debug!(
                ?dispatcher,
                "rejecting install_presentation_alongside: the dispatcher's own presentation is \
                 no longer registered, even though its ui_runtime survives"
            );
            return Err(InstallPresentationError::StalePresentation);
        }
        // A close becomes terminal when admitted, not when the bounded
        // owner FIFO eventually executes it. Registration alone therefore
        // cannot authorize this mutation: installing a sibling in that
        // interval would change the admitted close from a whole-ui_runtime close
        // into a partial close and leave the new presentation alive.
        if state.closing_presentations.contains(&dispatcher.address) {
            tracing::debug!(
                ?dispatcher,
                "rejecting install_presentation_alongside: the dispatcher's presentation has a \
                 terminal close pending"
            );
            return Err(InstallPresentationError::PresentationClosing);
        }
        let ui_runtime_slot = state
            .ui_runtimes
            .get_mut(&ui_runtime_id)
            .expect("BUG: presence just checked above via contains_key");
        let Some(ui_runtime) = ui_runtime_slot.ui_runtime.as_mut() else {
            return Err(InstallPresentationError::RuntimeUnavailable);
        };
        // Assemble WITHOUT installing yet -- see this function's own doc
        // for why the ordering matters. `presentation` is dropped (no
        // forest membership, so nothing to roll back) if registration
        // below fails.
        let window = std::sync::Arc::clone(presentation_window.window());
        let presentation = ui_runtime.assemble_presentation(presentation_window);
        let address = flui_foundation::PresentationAddress {
            ui_runtime_id,
            presentation_id: presentation.id(),
        };
        state.registry.try_register_window(&window, address)?;
        let ui_runtime_slot = state
            .ui_runtimes
            .get_mut(&ui_runtime_id)
            .expect("BUG: presence checked above, and nothing between here and there removed it");
        let ui_runtime = ui_runtime_slot
            .ui_runtime
            .as_mut()
            .expect("BUG: presence checked above, and nothing between here and there took it");
        ui_runtime.install_presentation(presentation);
        Ok(PresentationDispatcher {
            owner_thread,
            address,
        })
    })
}

/// Requests that one presentation be closed and removed from `dispatcher`'s
/// UI runtime — a single window closing out of a UI runtime that hosts more than one,
/// without tearing down the UI runtime itself (contrast `request_ui_runtime_uninstall`,
/// which removes a whole UI runtime). Request-shaped, like every other UI runtime-map
/// mutation in this module: this function only enqueues
/// [`RuntimeTask::ClosePresentation`] and (if the UI runtime is currently idle)
/// drives the drain loop that runs it — it never runs the six teardown
/// steps itself, and never returns anything about their outcome beyond
/// whether the request was accepted for this dispatcher (see
/// [`dispatch_platform_ui_runtime`]'s own `Err` variants).
///
/// The six steps run at Idle inside [`crate::app::ui_runtime::UiRuntime::close_presentation_entered`],
/// which only [`dispatch_platform_ui_runtime`]'s own drain loop calls, and only
/// with the UI runtime checked out as an owned local — so a dispose callback the
/// closed presentation runs mid-teardown sees the exact same
/// `dispatched_ui_runtime_id` TLS state (and therefore the exact same
/// install/uninstall deferral discipline) any other dispatched frame or
/// event callback sees. Production contract: async-at-Idle only — a caller
/// needing the six steps to have actually run before proceeding must drive
/// the dispatch loop to idle itself (a synchronous bypass would defeat the
/// whole point of routing this through the dispatch seam).
///
/// Production caller: [`close_this_window`] — the single `on_close` wiring
/// point every window this crate opens uses (`run_desktop`'s primary,
/// [`open_secondary_window`](super::secondary_window::open_secondary_window)'s new window under either [`WindowPolicy`](crate::app::runtime::WindowPolicy)).
/// This is the SAME entry point regardless of whether `id` turns out to be
/// its UI runtime's sole presentation (routes to a full UI runtime uninstall above) or
/// one of several (removes just this one, siblings and UI runtime survive) —
/// #555 closes with this slice; there is no further slice deferring this.
/// Also exercised directly by this module's own tests.
fn close_presentation(
    dispatcher: PresentationDispatcher,
    id: flui_foundation::PresentationId,
) -> Result<(), DispatchError> {
    dispatch_platform_ui_runtime(dispatcher, RuntimeTask::ClosePresentation(id))
}

/// Closes exactly the window `dispatcher` addresses — the single production
/// `on_close` wiring point for every window this crate opens (`run_desktop`'s
/// primary, both [`open_secondary_window`](super::secondary_window::open_secondary_window) policies). Routes through
/// [`close_presentation`], which correctly reduces to a full UI runtime uninstall
/// when `dispatcher`'s presentation is its UI runtime's ONLY one (a
/// [`WindowPolicy::Isolated`](crate::app::runtime::WindowPolicy::Isolated) window, or the last surviving
/// presentation of a [`WindowPolicy::Shared`](crate::app::runtime::WindowPolicy::Shared) group), or removes just
/// that one presentation while its UI runtime and any sibling presentation
/// survive otherwise — never `request_ui_runtime_uninstall` directly, which
/// would tear down an ENTIRE `WindowPolicy::Shared` group out from under a still-open
/// sibling window.
pub(super) fn close_this_window(dispatcher: PresentationDispatcher) {
    if let Err(error) = close_presentation(dispatcher, dispatcher.address.presentation_id) {
        tracing::warn!(?dispatcher, ?error, "close_this_window: dispatch refused");
    }
}

/// The single-window backends end the binding on either native close or quit.
/// Admission revokes async publication before deferred lifecycle observers run.
#[cfg(any(test, target_os = "android", target_arch = "wasm32"))]
pub(super) fn install_single_window_terminal_wiring(
    window: &std::sync::Arc<dyn flui_platform::traits::PlatformWindow>,
    platform: &flui_platform::SharedPlatform,
    dispatcher: PresentationDispatcher,
) {
    window.on_close(Box::new(move || close_this_window(dispatcher)));
    platform.on_quit(Box::new(move || close_this_window(dispatcher)));
}

/// Route every input event of `window` to its UI runtime: the one input wiring
/// each runner installs.
pub(super) fn install_input_wiring(
    dispatcher: PresentationDispatcher,
    window: &(impl flui_platform::traits::PlatformWindow + ?Sized),
) {
    window.on_input(Box::new(move |input| {
        dispatch_platform_input(dispatcher, input)
    }));
}

/// Deliver a window's platform input to its UI runtime and answer the platform
/// with the UI runtime's decision. A key no handler took keeps the platform's own
/// default (Alt+F4 closes, Alt+Space opens the system menu); a consumed one
/// prevents it. So does a key whose outcome is not known when the callback
/// returns (queued behind the current owner turn, or refused), so a shortcut
/// that will consume it later never races the default. Every other input
/// (pointer, IME, drag and drop) is always reported handled: the UI runtime owns
/// it, and a backend that redraws only for handled input (Android) must keep
/// doing so.
fn dispatch_platform_input(
    dispatcher: PresentationDispatcher,
    input: flui_platform::traits::PlatformInput,
) -> flui_platform::DispatchEventResult {
    // Only a key's answer depends on the outcome, so only a key pays for a
    // result channel: pointer move streams allocate nothing here.
    let consumed = matches!(input, flui_platform::traits::PlatformInput::Keyboard(_))
        .then(|| std::sync::Arc::new(std::sync::OnceLock::new()));
    let _ = dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::Event(RuntimeEvent::Input {
            input,
            consumed: consumed.clone(),
        }),
    );
    let default_prevented = consumed
        .as_ref()
        .is_none_or(|consumed| consumed.get().copied().unwrap_or(true));
    flui_platform::DispatchEventResult::resolved(false, default_prevented)
}

pub(super) fn dispatch_platform_ui_runtime(
    dispatcher: PresentationDispatcher,
    event: RuntimeTask,
) -> Result<(), DispatchError> {
    if std::thread::current().id() != dispatcher.owner_thread {
        tracing::error!(
            ?dispatcher,
            "rejecting ui_runtime callback on non-owner thread"
        );
        return Err(DispatchError::WrongThread);
    }

    let mut event = Some(event);
    let (starts_drain, queued_fallback, carried_callback) = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        validate_dispatch_admission(&state, dispatcher)?;
        if let Some(RuntimeTask::Frame(binding)) = event.as_ref()
            && binding.address() != dispatcher.address
        {
            return Err(DispatchError::StalePresentation);
        }
        if let Some(RuntimeTask::ClosePresentation(presentation_id)) = event.as_ref() {
            let closing_address = flui_foundation::PresentationAddress {
                ui_runtime_id: dispatcher.address.ui_runtime_id,
                presentation_id: *presentation_id,
            };
            if !state.registry.contains_address(closing_address) {
                return Err(DispatchError::StalePresentation);
            }
            if !state.closing_presentations.insert(closing_address) {
                return Err(DispatchError::PresentationClosing);
            }
            state.frame_drivers.fence_presentation(closing_address);
        } else if state.closing_presentations.contains(&dispatcher.address) {
            return Err(DispatchError::PresentationClosing);
        }
        if state.owner_turn_draining || state.iterating_all_ui_runtimes {
            state.owner_turn_queue.push_back((
                dispatcher,
                event.take().expect("BUG: admitted owner event is present"),
            ));
            return Ok((false, false, false));
        }
        if let Some(remaining) = state.owner_turn_callback_budget.as_mut() {
            if *remaining == 0 {
                state.owner_turn_queue.push_back((
                    dispatcher,
                    event.take().expect("BUG: admitted owner event is present"),
                ));
                return Ok((false, false, true));
            }
            *remaining -= 1;
            state.owner_turn_draining = true;
            return Ok((true, false, true));
        }
        let carried_continuation = state.owner_turn_continuation.is_some();
        if !state.owner_turn_queue.is_empty()
            && !carried_continuation
            && !state.owner_turn_continuation_failed
        {
            state.owner_turn_queue.push_back((
                dispatcher,
                event.take().expect("BUG: admitted owner event is present"),
            ));
            state.owner_turn_draining = true;
            return Ok((true, true, false));
        }
        state.owner_turn_draining = true;
        Ok((true, false, carried_continuation))
    })?;
    if !starts_drain {
        return Ok(());
    }

    let _guard = OwnerTurnDrainGuard;
    // A top-level turn: tell every ui_runtime if the app's fonts changed since
    // the last notice (the host feed landing, whose wake brings this turn).
    // The notices queue behind this turn's event and run in this drain.
    super::fonts::announce_font_change();
    if queued_fallback {
        drain_owner_turn_queue(OWNER_TURN_BUDGET);
        return Ok(());
    }
    let result = dispatch_platform_ui_runtime_now(
        dispatcher,
        event.take().expect("BUG: fresh owner event was not queued"),
    );
    if result.is_ok() && !carried_callback {
        drain_owner_turn_queue(OWNER_TURN_BUDGET.saturating_sub(1));
    }
    result
}

/// Drains the owner-local turn queue after the caller has atomically claimed
/// it by setting `owner_turn_draining`.
///
/// Keeping the drain separate from admission lets a whole-UI runtime visitor start
/// work that was accepted while `iterating_all_ui_runtimes` was true, once every
/// checked-out UI runtime has been restored. The guard is deliberately local to
/// this function so an unwinding task always releases the claim while leaving
/// later queued turns available to the next top-level owner turn.
struct OwnerTurnDrainGuard;

impl Drop for OwnerTurnDrainGuard {
    fn drop(&mut self) {
        APP_RUNTIME.with(|slot| slot.borrow_mut().owner_turn_draining = false);
        request_owner_turn_continuation();
    }
}

fn drain_owner_turn_queue(budget: usize) {
    for _ in 0..budget {
        let next = APP_RUNTIME.with(|slot| slot.borrow_mut().owner_turn_queue.pop_front());
        let Some((next_dispatcher, next_event)) = next else {
            return;
        };
        // The enqueueing call already performed admission so stale callers
        // still receive a synchronous error. Re-check here because an older
        // owner turn may have closed or replaced this target before its
        // queued turn reached the front.
        if let Err(error) = dispatch_platform_ui_runtime_now(next_dispatcher, next_event) {
            tracing::debug!(
                ?next_dispatcher,
                ?error,
                "dropping queued owner turn whose target became stale"
            );
        }
    }
}

/// Reserves and posts exactly one continuation opportunity for carried work.
/// The actuator runs after the TLS borrow and drain claim are released. A
/// failed or panicking actuator clears only its own sequence reservation, so
/// a synchronously reentrant replacement request cannot be erased.
fn request_owner_turn_continuation() {
    let request = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.owner_turn_callback_budget.is_some()
            || state.owner_turn_queue.is_empty()
            || state.owner_turn_continuation.is_some()
        {
            return None;
        }
        let wake = state.owner_turn_wake.clone()?;
        state.owner_turn_next_sequence = state
            .owner_turn_next_sequence
            .checked_add(1)
            .expect("BUG: owner-turn continuation sequence exhausted");
        let sequence = state.owner_turn_next_sequence;
        state.owner_turn_continuation = Some(sequence);
        Some((sequence, wake))
    });
    let Some((sequence, wake)) = request else {
        return;
    };
    let posted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wake())).unwrap_or(false);
    if posted {
        APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            if state.owner_turn_continuation == Some(sequence) {
                state.owner_turn_continuation_failed = false;
            }
        });
    } else {
        APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            if state.owner_turn_continuation == Some(sequence) {
                state.owner_turn_continuation = None;
                state.owner_turn_continuation_failed = true;
            }
        });
        tracing::warn!(
            "owner-turn continuation could not be posted; a later owner entry will retry"
        );
    }
}

/// Starts a physical native callback. If it consumes a posted continuation
/// (or retries after a failed post), every fresh root in this callback shares
/// one finite budget with the deferred FIFO.
#[must_use = "the guard finishes the physical owner callback, including during unwind"]
pub(super) struct OwnerCallbackGuard {
    #[cfg_attr(
        not(target_os = "ios"),
        expect(
            dead_code,
            reason = "only the iOS owner wake synthesizes one root per retained presentation"
        )
    )]
    resumes_carried_work: bool,
    outermost: bool,
}

impl OwnerCallbackGuard {
    /// Whether this callback consumed the continuation reserved for carried
    /// owner-local work.
    ///
    /// A backend that would otherwise synthesize a root for every retained
    /// presentation can use this to avoid replenishing the deferred FIFO
    /// faster than its finite continuation batch can drain it.
    #[cfg_attr(
        not(target_os = "ios"),
        expect(
            dead_code,
            reason = "only the iOS owner wake synthesizes one root per retained presentation"
        )
    )]
    pub(super) fn resumes_carried_work(&self) -> bool {
        self.resumes_carried_work
    }
}

impl Drop for OwnerCallbackGuard {
    fn drop(&mut self) {
        if !self.outermost {
            return;
        }

        // Keep nested synchronous platform callbacks attached to this
        // physical callback until its tail drain has completed. The clear
        // guard also restores the flag if a carried operation panics while
        // that tail is draining.
        struct ActiveCallbackClearGuard;
        impl Drop for ActiveCallbackClearGuard {
            fn drop(&mut self) {
                APP_RUNTIME.with(|slot| slot.borrow_mut().owner_turn_callback_active = false);
            }
        }

        let _active_callback = ActiveCallbackClearGuard;
        finish_owner_callback();
    }
}

pub(super) fn begin_owner_callback() -> OwnerCallbackGuard {
    let (resumes_carried_work, outermost) = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.owner_turn_callback_active {
            return (false, false);
        }
        state.owner_turn_callback_active = true;
        debug_assert!(state.owner_turn_callback_budget.is_none());
        let resumes_carried_work = state.owner_turn_continuation.take().is_some()
            || std::mem::take(&mut state.owner_turn_continuation_failed);
        if resumes_carried_work {
            state.owner_turn_callback_budget = Some(OWNER_TURN_BUDGET);
        }
        (resumes_carried_work, true)
    });
    OwnerCallbackGuard {
        resumes_carried_work,
        outermost,
    }
}

/// Runs one backend owner wake as a single physical callback.
///
/// Backends that fan one native wake out into multiple presentation roots
/// must not generate that fan-out while the wake is a continuation reserved
/// for the carried FIFO. `fresh_roots` is therefore called only for an
/// ordinary wake; `rearm_fresh_roots` records one later ordinary opportunity
/// when a coalescing native signal may have combined both causes.
#[cfg_attr(
    not(target_os = "ios"),
    expect(
        dead_code,
        reason = "only the iOS owner wake fans one callback out across presentations"
    )
)]
pub(super) fn drive_fanout_owner_callback(
    fresh_roots: impl FnOnce(),
    rearm_fresh_roots: impl FnOnce(),
) {
    let owner_callback = begin_owner_callback();
    if owner_callback.resumes_carried_work() {
        rearm_fresh_roots();
    } else {
        fresh_roots();
    }
}

/// Finishes a native callback and spends its unused continuation budget on
/// the deferred FIFO. The drain guard posts exactly one later opportunity if
/// work remains.
fn finish_owner_callback() {
    let unwinding = std::thread::panicking();
    let remaining = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        let remaining = state.owner_turn_callback_budget.take()?;
        if unwinding {
            return None;
        }
        if state.owner_turn_draining
            || state.iterating_all_ui_runtimes
            || state.owner_turn_queue.is_empty()
        {
            return None;
        }
        state.owner_turn_draining = true;
        Some(remaining)
    });
    if unwinding {
        request_owner_turn_continuation();
        return;
    }
    if let Some(remaining) = remaining {
        let _guard = OwnerTurnDrainGuard;
        drain_owner_turn_queue(remaining);
    }
}

/// Drains one finite batch when queued work becomes runnable outside a native
/// callback, notably after a whole-UI runtime visitor restores its checkouts.
#[cfg(any(
    test,
    all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    )
))]
fn continue_owner_turns() {
    let claimed = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.owner_turn_callback_budget.is_some() {
            return false;
        }
        state.owner_turn_continuation = None;
        state.owner_turn_continuation_failed = false;
        if state.owner_turn_draining
            || state.iterating_all_ui_runtimes
            || state.owner_turn_queue.is_empty()
        {
            return false;
        }
        state.owner_turn_draining = true;
        true
    });
    if claimed {
        let _guard = OwnerTurnDrainGuard;
        drain_owner_turn_queue(OWNER_TURN_BUDGET);
    }
}

fn validate_dispatch_admission(
    state: &crate::app::runtime::AppRuntime,
    dispatcher: PresentationDispatcher,
) -> Result<(), DispatchError> {
    let ui_runtime_id = dispatcher.address.ui_runtime_id;
    if state.ui_runtimes.is_empty() {
        tracing::debug!(
            ?dispatcher,
            "dropping ui_runtime callback: no ui_runtime installed (not yet ready, or already torn down)"
        );
        return Err(DispatchError::RuntimeUnavailable);
    }
    if !state.ui_runtimes.contains_key(&ui_runtime_id) {
        tracing::debug!(
            ?dispatcher,
            "dropping ui_runtime callback: a newer ui_runtime replaced the one it was dispatched for"
        );
        return Err(DispatchError::StaleRuntime);
    }
    if !state.registry.contains_address(dispatcher.address) {
        tracing::debug!(
            ?dispatcher,
            "dropping ui_runtime callback: presentation incarnation mismatch within the live ui_runtime"
        );
        return Err(DispatchError::StalePresentation);
    }
    Ok(())
}

fn dispatch_platform_ui_runtime_now(
    dispatcher: PresentationDispatcher,
    event: RuntimeTask,
) -> Result<(), DispatchError> {
    let ui_runtime_id = dispatcher.address.ui_runtime_id;
    let checked_out = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // Normative compare order (ADR-0037): ui_runtime first, then
        // presentation. `ui_runtime_id`/`presentation_id` mint from one shared
        // counter, so teardown+reinstall always changes both and the ui_runtime
        // check fires first on the common path.
        validate_dispatch_admission(&state, dispatcher)?;
        // Presentation check (issue #555's addressed-routing slice): membership in the SAME
        // `WindowRegistry` authority `dispatch_platform_ui_runtime`'s own
        // production window callbacks are resolved through, not equality
        // against `ui_runtime_slot.address` (which tracks only ONE presentation —
        // this ui_runtime's primary). A ui_runtime hosting N presentations has N live
        // addresses at once; a dispatcher naming any one of them, as long as
        // its exact address is still registered, is live -- one closed
        // (unregistered) since the dispatcher was minted, or a forged/mixed
        // address, is `StalePresentation` either way. Checked BEFORE
        // borrowing `ui_runtime_slot` mutably below (the registry is a sibling
        // field on the same `state`, so an immutable read here and a mutable
        // `ui_runtimes` borrow next cannot overlap).
        let ui_runtime_slot = state
            .ui_runtimes
            .get_mut(&ui_runtime_id)
            .expect("BUG: presence just checked above via contains_key");
        // Same-ui_runtime reentrancy: this ui_runtime is already draining (mid its
        // own drain loop below) or checked out (by its own dispatch, or by
        // a `for_each_installed_ui_runtime` visit) -- always safe to enqueue and
        // return early; the ongoing drain loop, or the next legitimate
        // dispatch once the ui_runtime is restored, picks the event up.
        if ui_runtime_slot.draining || ui_runtime_slot.ui_runtime.is_none() {
            ui_runtime_slot
                .queue
                .push_back((dispatcher.address.presentation_id, event));
            return Ok(None);
        }
        let ui_runtime_slot = state
            .ui_runtimes
            .get_mut(&ui_runtime_id)
            .expect("BUG: presence checked above");
        ui_runtime_slot
            .queue
            .push_back((dispatcher.address.presentation_id, event));
        let first = ui_runtime_slot
            .queue
            .pop_front()
            .expect("BUG: event was enqueued before starting ui_runtime dispatch");
        ui_runtime_slot.draining = true;
        let ui_runtime = ui_runtime_slot.ui_runtime.take();
        // Stash a clone of the checked-out ui_runtime's scheduler (and its
        // identity) BEFORE it leaves this slot: `installed_ui_runtime_phase`
        // (with_owner_platform's fence (c)) reads `dispatched_scheduler` as
        // its fallback whenever no OTHER ui_runtime's dispatch is in flight, which
        // is exactly the state this call is about to create for the entire
        // duration of the dispatched task below — otherwise the fence goes
        // blind for every real production frame, not merely when no ui_runtime is
        // installed at all. `UpdateScheduler::clone` is one `Arc::clone` (see
        // `flui-scheduler`'s single-`Arc` handle shape), not a second
        // scheduler.
        state.dispatched_scheduler = ui_runtime
            .as_ref()
            .map(|ui_runtime| ui_runtime.scheduler().clone());
        state.dispatched_ui_runtime_id = Some(ui_runtime_id);
        Ok(ui_runtime.map(|ui_runtime| (ui_runtime, first)))
    })?;
    let Some((mut ui_runtime, first)) = checked_out else {
        return Ok(());
    };

    // Never hold the TLS RefCell borrow across user/platform callbacks. Catch
    // only to restore the host invariants; the original panic is resumed.
    let native_retirement = APP_RUNTIME.with(|slot| slot.borrow().native_retirement.clone());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut next = Some(first);
        while let Some((task_presentation_id, event)) = next {
            // `ClosePresentation` is matched out here, before it would
            // otherwise reach `RuntimeTask::run`'s `&UiRuntime` receiver: closing
            // a presentation removes it from the forest (`&mut UiRuntime`),
            // which is only available in this exact window -- `ui_runtime` sits
            // here as an owned local, checked out of `APP_RUNTIME` for the
            // whole dispatched task, so `&mut` falls out naturally instead of
            // needing interior mutability on the forest itself.
            match event {
                RuntimeTask::ClosePresentation(id) => {
                    APP_RUNTIME.with(|slot| {
                        slot.borrow_mut().frame_drivers.retire_presentation(
                            flui_foundation::PresentationAddress {
                                ui_runtime_id,
                                presentation_id: id,
                            },
                        );
                    });
                    if ui_runtime.is_sole_presentation(id) {
                        // Reentrant events must fail admission before terminal observers run.
                        APP_RUNTIME.with(|slot| {
                            let mut state = slot.borrow_mut();
                            state.registry.remove_ui_runtime(ui_runtime_id);
                            state
                                .closing_presentations
                                .retain(|address| address.ui_runtime_id != ui_runtime_id);
                        });
                        let handlers = APP_RUNTIME
                            .with(|slot| slot.borrow().close_requests())
                            .take_ui_runtime(ui_runtime_id);
                        native_retirement.close_handlers(handlers);
                        // Closing the ui_runtime's ONLY presentation IS closing
                        // the ui_runtime. Dispatch Detached FIRST, through this
                        // exact ui_runtime, before requesting the uninstall --
                        // shutdown must cancel any in-flight pointer
                        // sequence whose platform Up/Cancel will never
                        // arrive, and must notify lifecycle observers,
                        // before the ui_runtime and its `UpdateScheduler` are gone.
                        // This is the same reason `on_quit`'s own Detached
                        // dispatch exists (`run_desktop`'s bootstrap),
                        // generalized to per-ui_runtime teardown instead of only
                        // process-wide quit: a ui_runtime closing because its one
                        // window closed is exactly as "detached" as one
                        // closing because the whole process quit, and
                        // `on_quit`'s own dispatch now frequently finds this
                        // ui_runtime already gone (a harmless, traced no-op --
                        // see that callback's doc). Uses `RuntimeEvent::
                        // Lifecycle(..).run` directly (the same private
                        // helper an ordinary `RuntimeTask::Event(RuntimeEvent::
                        // Lifecycle(..))` dispatches through below) rather
                        // than re-queuing another task, since `ui_runtime` is
                        // already the exact owned local that method needs.
                        let notification =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                ui_runtime.stop_presentations();
                            }));

                        // Closing the ui_runtime's ONLY presentation IS closing
                        // the ui_runtime -- routing it through
                        // close_presentation_entered would leave an empty
                        // forest behind (every other method on this ui_runtime,
                        // starting with `primary()`, assumes one always
                        // exists). Request a full ui_runtime uninstall through
                        // the SAME deferral machinery every other
                        // ui_runtime-map mutation already uses:
                        // `dispatched_ui_runtime_id` is `Some` for this whole
                        // checkout, so this defers to `dispatch_platform_
                        // ui_runtime`'s own tail (below), which runs
                        // `apply_uninstall` only after `ui_runtime` is restored
                        // to its slot -- `apply_uninstall` already removes
                        // every one of this ui_runtime's `WindowRegistry`
                        // entries before dropping the `RuntimeSlot` (see its
                        // own doc), so step 1 is covered by the SAME path
                        // a whole-ui_runtime close always used.
                        let displaced = APP_RUNTIME.with(|slot| {
                            slot.borrow_mut()
                                .request_ui_runtime_uninstall(ui_runtime_id)
                        });
                        drop(displaced);
                        if let Err(payload) = notification {
                            std::panic::resume_unwind(payload);
                        }
                    } else {
                        // Step 1: unregister exactly THIS presentation's own
                        // window mapping -- never a sibling's, and never
                        // every window this ui_runtime owns (`WindowRegistry::
                        // remove_ui_runtime` would be wrong here). `UiRuntime`
                        // itself has no access to the registry (AppRuntime
                        // is its single authority, ADR-0037 §2), so this
                        // runs here, in the one caller that does, before
                        // handing off to close_presentation_entered's
                        // steps 2-6. Without this, a stale platform event
                        // for the closed presentation's original window
                        // would still resolve to its now-dead
                        // PresentationId instead of being refused.
                        let address = flui_foundation::PresentationAddress {
                            ui_runtime_id,
                            presentation_id: id,
                        };
                        let unregistered = APP_RUNTIME.with(|slot| {
                            let mut state = slot.borrow_mut();
                            state.closing_presentations.remove(&address);
                            state.registry.remove_presentation(address)
                        });
                        drop(unregistered);
                        // Same step for the close-request router (issue
                        // #558): this presentation can no longer be asked
                        // about, nor closed programmatically, once its
                        // routable address is gone.
                        let handler = APP_RUNTIME
                            .with(|slot| slot.borrow().close_requests())
                            .take(address);
                        native_retirement.close_handlers(handler);

                        // Re-stamp this ui_runtime's tracked routable address
                        // (`RuntimeSlot::address`) to the surviving primary
                        // BEFORE running `id`'s own teardown/dispose hooks
                        // -- not after. `close_presentation_entered`'s step
                        // 2-3 (below) can run a dispose hook that re-enters
                        // this exact function with a dispatcher still
                        // bearing `id`; ordering the re-stamp first means
                        // that reentrant dispatch's `StalePresentation`
                        // check (at the top of this function) already
                        // compares against the NEW primary and correctly
                        // refuses it right there. Re-stamping AFTER
                        // disposal instead would leave a window where that
                        // same reentrant dispatch still compares equal
                        // (both sides still `id`), gets ACCEPTED by the
                        // stale check, and is merely enqueued behind the
                        // same-ui_runtime reentrancy guard -- only to run a
                        // moment later, in this very drain loop, against
                        // whatever survives the close: silently
                        // misaddressed rather than refused. See
                        // `UiRuntime::primary_id_excluding`'s own doc for why
                        // this is computable before the removal happens.
                        // Kept as an `if let` rather than an unwrap: this
                        // branch runs only when `is_sole_presentation(id)` was
                        // `false` above, which rules out a forest holding just
                        // `id` but not an EMPTY one — `is_sole_presentation`
                        // is `false` for a forest of none as well. A `None`
                        // here is therefore not reachable for a ui_runtime that
                        // still hosts something, and the arm simply skips the
                        // re-stamp for one that does not.
                        if let Some(surviving_primary_id) = ui_runtime.primary_id_excluding(id) {
                            APP_RUNTIME.with(|slot| {
                                if let Some(ui_runtime_slot) =
                                    slot.borrow_mut().ui_runtimes.get_mut(&ui_runtime_id)
                                {
                                    ui_runtime_slot.address.presentation_id = surviving_primary_id;
                                }
                            });
                        }

                        ui_runtime.close_presentation_entered(id);
                    }
                }
                // Not entered here: the pump enters the ui_runtime itself, and a
                // installed driver enters it explicitly for its gate.
                RuntimeTask::Frame(binding) => {
                    let driver =
                        APP_RUNTIME.with(|slot| slot.borrow().frame_drivers.checkout(binding));
                    if let Some(mut driver) = driver {
                        driver.wake(&mut ui_runtime);
                    }
                }
                #[cfg(any(test, target_os = "ios"))]
                RuntimeTask::BackgroundPump => {
                    // There is no frame gate to consume the redraw report. Async
                    // work may enqueue new commands for the next owner opportunity.
                    let _ = ui_runtime.enter(crate::app::ui_runtime::UiRuntime::drain_owner_inbox);
                    ui_runtime.pump_background();
                }
                other => ui_runtime.enter(|ui_runtime| other.run(ui_runtime, task_presentation_id)),
            }
            next = APP_RUNTIME.with(|slot| {
                slot.borrow_mut()
                    .ui_runtimes
                    .get_mut(&ui_runtime_id)
                    .and_then(|ui_runtime_slot| ui_runtime_slot.queue.pop_front())
            });
        }
    }));
    let removed = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        // The slot may be gone entirely if a NESTED, non-dispatch teardown
        // (e.g. `teardown_platform_ui_runtime`'s full-registry clear, called
        // reentrantly from inside the just-run task) already removed it —
        // that dropped this ui_runtime's queue/applier already, so there is
        // nothing left to restore; just let `ui_runtime` fall out of scope below.
        if let Some(ui_runtime_slot) = state.ui_runtimes.get_mut(&ui_runtime_id) {
            ui_runtime_slot.ui_runtime = Some(ui_runtime);
            ui_runtime_slot.draining = false;
        }
        // Cleared unconditionally in this same restore block, which runs
        // whether or not the dispatched task above panicked (the panic, if
        // any, is only resumed after this restore completes below) — the
        // fence-(c) fallback must not survive past the dispatch it was
        // stashed for.
        state.dispatched_scheduler = None;
        state.dispatched_ui_runtime_id = None;
        // Applies any ui_runtime-map mutation this ui_runtime's own task requested
        // (e.g. a dispose callback uninstalling itself, or a frame callback
        // installing a second ui_runtime) — deferred above precisely because
        // `dispatched_ui_runtime_id` was `Some` for the whole task, and safe to
        // apply now that it is cleared.
        let removed = state.drain_pending_ui_runtime_mutations();
        // A ui_runtime that leaves the map HERE left it after the exit-policy hook
        // had already been consulted and answered "don't exit".
        //
        // That is reachable, and was a tracked gap: a window closed
        // REENTRANTLY from inside a dispatched callback requests its own
        // ui_runtime's uninstall, which — being a same-ui_runtime dispatch — only
        // enqueues. `window.close()`'s `notify_closed` then consults the hook
        // while the registry is still non-empty, gets a veto, and returns;
        // the uninstall applies moments later, right above, with nothing left
        // to re-ask. The exit was missed, not merely delayed.
        //
        // So re-ask, through the platform's own coalesced owner-thread
        // request rather than any new machinery.
        //
        // Armed whenever the drain yielded a slot, which is NOT the same as
        // "every mutation": a successful `Install` yields none and arms
        // nothing, while `removed` carries both an applied `Uninstall`'s slot
        // AND a REJECTED install's — `drain_pending_ui_runtime_mutations` routes a
        // window-id collision through the same bucket. Only the first
        // actually shrinks the ui_runtime map; the second is a spurious arm on an
        // already-logged error path. Left that way deliberately: the seam's
        // own contract makes a spurious request a no-op — windows still open,
        // or a hook still vetoing, decide nothing — so paying for it is
        // cheaper than teaching the drain to report which kind it applied.
        //
        // Cloned out here and fired below, outside this borrow: the hook
        // borrows `APP_RUNTIME` itself.
        #[cfg(not(target_arch = "wasm32"))]
        let reevaluate_exit = (!removed.is_empty())
            .then(|| state.exit_policy_reevaluation_notifier())
            .flatten();
        #[cfg(target_arch = "wasm32")]
        let reevaluate_exit: Option<std::sync::Arc<dyn Fn() + Send + Sync>> = None;
        state.frame_drivers.sweep_retired();
        (removed, reevaluate_exit, state.native_retirement.clone())
    });
    let (removed, reevaluate_exit, current_retirement) = removed;
    // Destructors may re-enter platform/framework code — drop only after the
    // TLS borrow above has released.
    let mut first_panic = result.err();
    current_retirement.drain(&mut first_panic);
    native_retirement.drain(&mut first_panic);
    drop_removed_ui_runtimes(removed, &mut first_panic);
    // Fired after the borrow AND after those destructors: the hook this wakes
    // borrows `APP_RUNTIME`, and a ui_runtime dropped by `removed` must be gone
    // before the policy is asked whether anything is left.
    if let Some(reevaluate) = reevaluate_exit {
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reevaluate())).err();
        preserve_first_lifecycle_panic(&mut first_panic, failure, "exit policy reevaluation");
    }
    // Applies any `open_secondary_window` Pending-arm completion this
    // ui_runtime's own task resolved (see `drain_pending_secondary_window_
    // completions`'s own doc for why this must run only now, after the
    // checkout state above is cleared) — same "drain regardless of panic"
    // discipline as `drain_pending_ui_runtime_mutations` above, for the same
    // reason: a resolved secondary window should not sit unwired forever
    // just because this dispatch's own task panicked for an unrelated
    // reason. This function itself (`dispatch_platform_ui_runtime`) compiles on
    // every backend (only `not(target_os = "ios")`), but the drain helper
    // and everything it touches exist only on the desktop-only
    // `open_secondary_window` family's own cfg -- gate the call site to
    // match, not the function.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    {
        let notification =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(drain_quit_notification)).err();
        preserve_first_lifecycle_panic(
            &mut first_panic,
            notification,
            "deferred quit notification",
        );
        let completions = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            drain_pending_secondary_window_completions,
        ))
        .err();
        preserve_first_lifecycle_panic(&mut first_panic, completions, "secondary completion drain");
        let main = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            super::main_window::drive_main_window,
        ))
        .err();
        preserve_first_lifecycle_panic(&mut first_panic, main, "main window drain");
    }
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
    Ok(())
}

/// Release each removed UI runtime independently: one user destructor must not
/// unwind through another removed UI runtime or skip the deferred quit notification.
fn drop_removed_ui_runtimes(
    removed: Vec<RuntimeSlot>,
    first_panic: &mut Option<Box<dyn std::any::Any + Send>>,
) {
    for slot in removed {
        let RuntimeSlot {
            ui_runtime,
            queue,
            surface_applier,
            ..
        } = slot;
        let failure =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(ui_runtime))).err();
        preserve_first_lifecycle_panic(first_panic, failure, "removed UI runtime");
        for (_, task) in queue {
            let failure =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(task))).err();
            preserve_first_lifecycle_panic(first_panic, failure, "removed runtime operation");
        }
        let failure =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(surface_applier))).err();
        preserve_first_lifecycle_panic(first_panic, failure, "removed surface applier");
    }
}

/// Complete registry-owned retirement after the caller has released app TLS.
pub(super) fn complete_registry_retirement(removed: Vec<RuntimeSlot>) {
    let retirement = APP_RUNTIME.with(|slot| slot.borrow().native_retirement.clone());
    let mut first_panic = None;
    drop_removed_ui_runtimes(removed, &mut first_panic);
    retirement.drain(&mut first_panic);
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
}

fn drop_queued_turns(
    turns: VecDeque<(PresentationDispatcher, RuntimeTask)>,
    first_panic: &mut Option<Box<dyn std::any::Any + Send>>,
) {
    for (_, task) in turns {
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(task))).err();
        preserve_first_lifecycle_panic(first_panic, failure, "retired owner operation");
    }
}

/// Hot-restart's own iteration primitive (issue #555): visits every
/// currently-installed UI runtime, in mount order, running `f` against each one
/// OUTSIDE the `APP_RUNTIME` borrow — the same checkout/restore discipline
/// [`dispatch_platform_ui_runtime`] uses for a single addressed UI runtime.
///
/// What `f` may safely do, precisely (not "any `APP_RUNTIME`-touching
/// function" — the visited UI runtime counts as dispatched, see below, so the
/// same restrictions a dispatched task's own closure has apply here too):
/// dispatch back to the SAME UI runtime being visited (enqueues via the ordinary
/// same-UI runtime reentrant path, never recurses); request a UI runtime-map
/// install/uninstall (defers rather than applies immediately — see
/// [`crate::app::runtime::AppRuntime::request_ui_runtime_install`]'s own doc — applied
/// only once every UI runtime has been visited, so the set of UI runtimes visited
/// never shifts mid-iteration); or call [`crate::app::runtime::AppRuntime::should_exit`]
/// (also defers its own drain while this visit is in flight, for the same
/// reason). Dispatch to a different sibling UI runtime is admitted into the
/// host-wide owner queue and runs only after every visited UI runtime has been
/// restored and the deferred UI runtime-map mutations have been applied.
///
/// Each visited UI runtime is ALSO stashed into `dispatched_scheduler`/
/// `dispatched_ui_runtime_id` for the duration of its own call to `f` — the same
/// fields `dispatch_platform_ui_runtime` stashes for a dispatched task — so
/// `with_owner_platform`'s fence (c) stays able to see the visited UI runtime's
/// own scheduler phase for the whole time it sits checked out of `ui_runtimes`,
/// exactly as it does for a real dispatch. `iterating_all_ui_runtimes` prevents
/// the queued sibling operation from starting while any UI runtime is checked
/// out; the visit tail claims and drains that queue after restoration.
///
/// A panic inside `f` is caught, not propagated past this function's own
/// cleanup: the visited UI runtime is restored to its slot, `iterating_all_ui_runtimes`
/// is cleared, and any mutation deferred during the visit (including one
/// requested by the very call that panicked) is drained, all BEFORE the
/// panic resumes. Skipping any of that on a panicking visit would strand the
/// visited UI runtime's slot at `ui_runtime: None` forever and wedge
/// `iterating_all_ui_runtimes` at `true` forever — which, after
/// `drain_pending_ui_runtime_mutations`'s own guard against draining while a
/// visit is nominally still in flight, would silently defer every future
/// UI runtime-map mutation for the rest of the process.
///
/// Quit notification reuses this checkout discipline. Its visitor catches each
/// UI runtime's lifecycle panic so all siblings still receive the notification.
#[cfg(any(
    test,
    all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    )
))]
fn for_each_installed_ui_runtime(mut f: impl FnMut(&crate::app::ui_runtime::UiRuntime)) {
    let native_retirement = APP_RUNTIME.with(|slot| slot.borrow().native_retirement.clone());
    let ids = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        debug_assert!(
            !state.iterating_all_ui_runtimes,
            "BUG: reentrant for_each_installed_ui_runtime"
        );
        state.iterating_all_ui_runtimes = true;
        state.ui_runtimes.keys()
    });

    let mut panic_payload = None;
    for id in ids {
        let ui_runtime = APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            let ui_runtime = state
                .ui_runtimes
                .get_mut(&id)
                .and_then(|ui_runtime_slot| ui_runtime_slot.ui_runtime.take())?;
            // Mirrors `dispatch_platform_ui_runtime`'s own checkout stash: while
            // `ui_runtime` sits outside `ui_runtimes` for the extent of `f` below,
            // fence (c) must still be able to read ITS phase, not go blind.
            state.dispatched_scheduler = Some(ui_runtime.scheduler().clone());
            state.dispatched_ui_runtime_id = Some(id);
            Some(ui_runtime)
        });
        let Some(ui_runtime) = ui_runtime else {
            // Removed by an earlier ui_runtime's own visit before this iteration
            // reached it -- unreachable under the defer-to-idle discipline
            // (a mutation requested mid-visit only applies AFTER the whole
            // visit completes), kept as a defensive skip rather than an
            // `expect`.
            continue;
        };

        // Never hold the TLS RefCell borrow across `f` -- the same
        // discipline `dispatch_platform_ui_runtime` follows for a dispatched
        // task. Catch only to restore the checked-out ui_runtime and this
        // visit's own bookkeeping; the original panic resumes once every
        // ui_runtime has been handled (visited, or left untouched after a
        // panic stops the walk).
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&ui_runtime)));
        APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            if let Some(ui_runtime_slot) = state.ui_runtimes.get_mut(&id) {
                ui_runtime_slot.ui_runtime = Some(ui_runtime);
            }
            state.dispatched_scheduler = None;
            state.dispatched_ui_runtime_id = None;
        });
        if let Err(payload) = result {
            panic_payload = Some(payload);
            break;
        }
    }

    let removed = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.iterating_all_ui_runtimes = false;
        state.drain_pending_ui_runtime_mutations()
    });
    drop_removed_ui_runtimes(removed, &mut panic_payload);
    native_retirement.drain(&mut panic_payload);
    let owner_turns =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(continue_owner_turns)).err();
    preserve_first_lifecycle_panic(
        &mut panic_payload,
        owner_turns,
        "owner turns queued during ui_runtime visit",
    );
    // Same rationale as `dispatch_platform_ui_runtime`'s own tail: a visited
    // ui_runtime's frame callback may have resolved an `open_secondary_window`
    // Pending completion via the ui_runtime's owner-task poll, which cannot
    // complete mid-visit for the same reason it cannot complete
    // mid-dispatch (`iterating_all_ui_runtimes` holds this thread's checkout
    // state just as `dispatched_ui_runtime_id` does). The visitor is available
    // in desktop builds and tests; secondary completion is desktop-only.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    {
        let notification =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(drain_quit_notification)).err();
        preserve_first_lifecycle_panic(
            &mut panic_payload,
            notification,
            "visited quit notification",
        );
        let completions = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            drain_pending_secondary_window_completions,
        ))
        .err();
        preserve_first_lifecycle_panic(
            &mut panic_payload,
            completions,
            "visited secondary completion drain",
        );
    }

    if let Some(payload) = panic_payload {
        std::panic::resume_unwind(payload);
    }
}

/// Close admission now; notify once all currently checked-out UI runtime state returns.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn request_quit_notification() {
    APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.quit_notification == crate::app::runtime::QuitNotification::Active {
            state.quit_notification = crate::app::runtime::QuitNotification::Requested;
        }
    });
    drain_quit_notification();
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn drain_quit_notification() {
    use crate::app::runtime::QuitNotification;
    let ready = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.quit_notification != QuitNotification::Requested
            || state.dispatched_ui_runtime_id.is_some()
            || state.iterating_all_ui_runtimes
        {
            return false;
        }
        state.quit_notification = QuitNotification::Notifying;
        true
    });
    if !ready {
        return;
    }
    let mut first_panic = None;
    let cancel = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        super::secondary_window::cancel_pending_secondary_windows,
    ))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, cancel, "pending window cancellation");
    let visit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for_each_installed_ui_runtime(|ui_runtime| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ui_runtime.enter(|ui_runtime| {
                    ui_runtime.stop_presentations();
                });
            }))
            .err();
            preserve_first_lifecycle_panic(
                &mut first_panic,
                result,
                "ui_runtime quit notification",
            );
        });
    }))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, visit, "quit visitor cleanup");
    APP_RUNTIME.with(|slot| slot.borrow_mut().quit_notification = QuitNotification::Notified);
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
}

/// Per-pool grace deadline for joining running background work at full
/// loop-exit teardown. Bounds a hung compute job's ability to wedge process
/// exit; running work that finishes sooner ends shutdown sooner (the
/// deadline is a cap, not a sleep).
#[cfg(not(target_arch = "wasm32"))]
const EXECUTION_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Deadline for the staged SERVICE shutdown (issue #558) that runs just
/// before the pools close: every service is cancelled first, then joined
/// against this one shared deadline — the bounded flush window in which a
/// service writes its final state. A service that ignores cancellation is
/// reported (`DeadlineExceeded`) and force-abandoned by the pool shutdown
/// that follows; it cannot wedge process exit past this deadline plus the
/// per-pool grace above.
#[cfg(not(target_arch = "wasm32"))]
const SERVICE_SHUTDOWN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

/// Full loop-exit teardown: drop every hosted UI runtime, close-request
/// registration, service and execution pool, and the platform clipboard.
///
/// Reached from each backend's loop exit — `run_desktop`/`run_android` after
/// `Platform::run` returns, and iOS from `applicationWillTerminate:`, which is
/// the only pre-exit signal a `UIApplicationMain` loop that never returns can
/// offer.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn teardown_platform_ui_runtime() {
    let (ui_runtimes, queued_turns) = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.frame_drivers.retire_all();
        // Registry removal first (ADR-0037 §2): stop new routing before the
        // queued old-generation events below are dropped, and before the
        // registry is cleared. The teardown real read: assert the removed
        // entries include the address EACH ui_runtime installed — `remove_ui_runtime`
        // removes every window mapped to that ui_runtime, not just the first, so
        // a one-ui_runtime-many-windows install still leaves nothing behind.
        //
        // Every hosted ui_runtime tears down here, not just one: this runs from
        // `run_desktop`/`run_android` after their respective
        // `platform.run(...)` returns, i.e. the WHOLE loop is exiting, so
        // every ui_runtime this thread ever hosted goes with it.
        let ui_runtimes = state.ui_runtimes.clear();
        for (id, ui_runtime_slot) in &ui_runtimes {
            let removed = state.registry.remove_ui_runtime(*id);
            debug_assert!(
                removed
                    .iter()
                    .any(|(_, removed_address)| *removed_address == ui_runtime_slot.address),
                "BUG: window_registry teardown read did not include the installed address"
            );
        }
        // Close-request registrations go with the loop, not with any one
        // ui_runtime (issue #558). Per-ui_runtime teardown already drops a ui_runtime's
        // own entries, but an explicit platform quit, or a bootstrap that
        // wired a window and then failed before installing its ui_runtime,
        // leaves entries no ui_runtime removal ever names — and this same
        // `AppRuntime` serves a SECOND `Platform::run` on this thread, so a
        // survivor would be consulted by the next loop's windows.
        state
            .native_retirement
            .close_handlers(state.close_requests().take_all());
        state.owner_thread = None;
        state.dispatched_scheduler = None;
        state.dispatched_ui_runtime_id = None;
        state.iterating_all_ui_runtimes = false;
        let queued_turns = std::mem::take(&mut state.owner_turn_queue);
        state.owner_turn_continuation = None;
        state.owner_turn_continuation_failed = false;
        state.owner_turn_callback_budget = None;
        state.owner_turn_callback_active = false;
        state.owner_turn_draining = false;
        state.closing_presentations.clear();
        debug_assert!(
            !state.has_pending_ui_runtime_mutations(),
            "BUG: ui_runtime-map mutations still pending at full loop-exit teardown -- \
             dispatch_platform_ui_runtime and for_each_installed_ui_runtime must drain \
             unconditionally in their own tails"
        );
        (ui_runtimes, queued_turns)
    });
    // Runtime and queued-task destructors may re-enter platform/framework
    // code. Drop both only after the TLS borrow and incarnation identity have
    // been released.
    let mut first_panic = None;
    drop_removed_ui_runtimes(
        ui_runtimes.into_iter().map(|(_, slot)| slot).collect(),
        &mut first_panic,
    );
    drop_queued_turns(queued_turns, &mut first_panic);
    let retirement = APP_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.frame_drivers.sweep_retired();
        state.native_retirement.clone()
    });
    retirement.drain(&mut first_panic);

    // Service-lifecycle shutdown (issue #558) BEFORE the pools close: the
    // registry cancels every application service cooperatively and joins
    // each against one shared deadline — the flush window in which a
    // service persists its final state. Ordering is load-bearing: the
    // execution shutdown below cancels the pools' root token and
    // hard-drops any future still running at its next await point, so a
    // service joined AFTER that would lose its flush window every time.
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let report = APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .shutdown_lifecycles(SERVICE_SHUTDOWN_DEADLINE)
        });
        let incomplete: Vec<&'static str> = report
            .entries
            .iter()
            .filter(|entry| {
                entry.outcome != crate::app::lifecycle::ServiceShutdownOutcome::Completed
            })
            .map(|entry| entry.name)
            .collect();
        if incomplete.is_empty() {
            tracing::debug!(
                services = report.entries.len(),
                "application services shut down cleanly"
            );
        } else {
            tracing::warn!(
                services = report.entries.len(),
                ?incomplete,
                "some application services did not complete by the shutdown deadline"
            );
        }
    }))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, failure, "service shutdown");

    // Execution-services shutdown (issue #557): the whole loop is exiting,
    // so stop background admission, cancel outstanding work, join running
    // work bounded by a per-pool grace deadline, and CLEAR the slot — a
    // second platform loop hosted on this same thread later (an embedder
    // running `run_app` twice in one process) must re-resolve fresh
    // services at its own ui_runtime install, not inherit an instance whose
    // admission is permanently closed. Loop-scoped like the clipboard
    // below — hot-restart never reaches this function, so a reinstalled
    // ui_runtime keeps its pools. A teardown path that skips this (panic
    // mid-teardown) still tears the pools down non-blockingly via
    // `ExecutionServices`' own `Drop`.
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .shutdown_execution(EXECUTION_SHUTDOWN_GRACE);
        });
    }))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, failure, "execution shutdown");

    // ADR-0038 §9's install/teardown symmetry: the event loop has exited (this
    // runs from both `run_desktop` and `run_android`, after their respective
    // `platform.run(...)` returns), so drop the platform clipboard now rather
    // than let a live platform resource (arboard on X11 owns a live X11
    // connection) sit pinned behind `AppRuntime` for the rest of the
    // process's life. `Drop for AppRuntime` is the last-resort third clear
    // if this explicit path is ever skipped (a panic mid-teardown, for
    // instance) — see that impl's doc.
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let released = APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            state.clear_platform_clipboard();
            state.clear_redraw_window()
        });
        // Ordinarily `None` already: the window-close path released this pin
        // (`release_redraw_window_for`) while the event loop was still alive,
        // which is the order the platform teardown contract wants. Dropped here
        // outside the TLS borrow for the paths that never closed a window (an
        // OS-level quit with the window still open).
        drop(released);
    }))
    .err();
    preserve_first_lifecycle_panic(&mut first_panic, failure, "native shutdown");
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
}

#[path = "owner_dispatch/tests.rs"]
#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
mod owner_dispatch_tests;
