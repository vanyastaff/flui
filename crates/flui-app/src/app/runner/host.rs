use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::terminal::contained;
use crate::app::runtime::{AppRuntime, ExitPolicy};

/// A fresh clone of the loop-scoped platform wake capability — see
/// `AppRuntime::frame_wake_callback`'s doc. `APP_RUNTIME` must not be
/// currently mutably borrowed when this is called (it takes a shared
/// borrow); every call site here is either before a UI runtime is installed or
/// after one has been taken out of the slot for dispatch.
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
pub(super) fn runtime_wake_callback() -> Arc<dyn Fn() + Send + Sync> {
    APP_RUNTIME.with(|slot| slot.borrow().frame_wake_callback())
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn start_service(
    definition: &crate::app::lifecycle::ServiceDefinition,
) -> Result<(), crate::app::lifecycle::ServiceStartError> {
    let (generation, admission) =
        APP_RUNTIME.with(|slot| slot.borrow_mut().prepare_service(definition))?;
    let prepared = admission.construct(definition)?;
    let result = APP_RUNTIME.with(|slot| slot.borrow_mut().commit_service(generation, prepared));
    match result {
        Ok(launch) => launch.spawn(),
        Err(prepared) => {
            prepared.refuse();
            Err(crate::app::lifecycle::ServiceStartError::Spawn(
                flui_runtime::execution::SpawnError::ShuttingDown,
            ))
        }
    }
}

/// The providers a UI runtime this host builds acquires platform capabilities
/// from (ADR-0152). Same borrow rule as [`runtime_wake_callback`].
pub(super) fn runtime_capabilities() -> flui_runtime::capability::CapabilityRegistry {
    APP_RUNTIME.with(|slot| slot.borrow().capability_registry())
}

/// The app's font collection for [`crate::app::ui_runtime::UiRuntime::new`]'s
/// `fonts` parameter (ADR-0092 §2). Same borrow rule as
/// [`runtime_wake_callback`]. The first call on a thread resolves the shared
/// engine services.
pub(super) fn runtime_font_collection() -> flui_painting::FontCollection {
    APP_RUNTIME.with(|slot| slot.borrow().font_collection())
}

/// The host's byte storage, resolved once from the run's configuration when
/// the host started (`AppRuntime::install_host_storage`). Same borrow rule
/// as [`runtime_wake_callback`].
pub(super) fn runtime_storage() -> Option<Arc<dyn flui_platform::Storage>> {
    APP_RUNTIME.with(|slot| slot.borrow().host_storage())
}

/// Stash the host executors `config` injects, before anything resolves the
/// loop's execution services (the host storage does, on install).
#[cfg(any(target_os = "android", target_arch = "wasm32"))]
pub(super) fn install_configured_executors(config: &crate::app::AppConfig) {
    if let Some(executors) = config.executors.clone() {
        APP_RUNTIME.with(|slot| slot.borrow_mut().install_host_executors(executors));
    }
}

/// Give the host the storage `config` asks for, under the installed owner
/// platform's data roots. Call once the owner platform and the host's
/// executors are installed; without a configured directory, or where the
/// platform has no data root, widgets get no storage.
pub(super) fn install_host_storage(config: &crate::app::AppConfig) {
    let dirs = config.storage_dir.and_then(|name| {
        let dirs = with_owner_platform(|owner| owner.shared().data_dirs(&name)).flatten();
        if dirs.is_none() {
            tracing::warn!(
                storage_dir = name.as_str(),
                "this platform has no data directory; widgets get no storage"
            );
        }
        dirs
    });
    // Always installed, `None` included: a run without storage must not
    // inherit the previous run's on this thread.
    APP_RUNTIME.with(|slot| slot.borrow_mut().install_host_storage(dirs));
    // The run's receivers replace the last run's, for the same reason.
    let memory = APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().host_memory));
    memory.replace_receivers(config.memory_reclaimers.0.clone());
}

/// Have the host trim the GPU memory behind a new raster lane on a
/// memory-pressure notice, for as long as the lane exists.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn track_raster_lane(lane: flui_engine::RasterHandle) {
    let memory = APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().host_memory));
    memory.track_raster_lane(lane);
}

/// Shared host services cannot depend on a desktop window-request error.
#[derive(Debug, thiserror::Error)]
pub(super) enum HostServicesError {
    #[error(transparent)]
    Preferences(#[from] flui_native::PlatformError),
    #[error("display sampling failed")]
    Displays(#[source] flui_native::PlatformError),
    #[error(transparent)]
    Owner(#[from] flui_runtime::owner::DispatchError),
    #[error(transparent)]
    Runtime(#[from] crate::app::ui_runtime::UiRuntimeError),
}

pub(super) fn refresh_preferences_with(
    read: impl FnOnce(
        &flui_native::OwnerPlatform,
    ) -> Result<flui_platform::SystemPreferences, flui_native::PlatformError>,
) -> Result<(), HostServicesError> {
    let (owner, host) = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        (state.owner_platform.clone(), state.installed_host.clone())
    });
    let Some(owner) = owner else {
        return Ok(());
    };
    let observation = read(&owner);
    // Native getters may pump messages, replacing either side of this pair.
    // Discard obsolete errors as well as values before touching the new host.
    let current = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        state
            .owner_platform
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, &owner))
            && state.installed_host.same_host(&host)
    });
    if !current {
        return Ok(());
    }
    if matches!(
        observation,
        Err(flui_native::PlatformError::PreferencesDeferred)
    ) {
        // A deferred read publishes nothing. The source retains its retry;
        // unrelated owner wakes must not flood diagnostics during backoff.
        return Ok(());
    }
    let values = observation?;
    host.logical().update_preferences(values, host.effects())?;
    Ok(())
}

/// The colour profile of the display `window` is on, by the installed host's
/// accepted snapshot: the display report a renderer for `window` is set up
/// with.
pub(super) fn display_color_of(
    window: &dyn flui_platform::PlatformWindow,
) -> Option<flui_platform::DisplayColor> {
    let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
    host.displays().color_of(window)
}

/// Sample the platform's displays, accept the snapshot, then tell every
/// followed window which display it is on (ADR-0236).
///
/// Follows the preference rules of ADR-0172: the sample runs outside any host
/// borrow, an obsolete result (the host or the platform was replaced during the
/// read) is discarded, the snapshot is committed before any window is notified,
/// and a failed read keeps the last accepted snapshot. A window that fails to
/// apply its update neither stops the others nor loses the update: the first
/// failure is rethrown once every window has been offered its value, and the
/// next sample offers the same value again.
pub(super) fn refresh_displays_with(
    read: impl FnOnce(
        &flui_native::OwnerPlatform,
    ) -> Result<flui_platform::DisplaySnapshot, flui_native::PlatformError>,
) -> Result<(), HostServicesError> {
    let (owner, host) = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        (state.owner_platform.clone(), state.installed_host.clone())
    });
    let Some(owner) = owner else {
        return Ok(());
    };
    let observation = read(&owner);
    // A native sample may pump messages, replacing either side of this pair.
    let current = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        state
            .owner_platform
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, &owner))
            && state.installed_host.same_host(&host)
    });
    if !current {
        return Ok(());
    }
    let failure = match observation {
        Ok(snapshot) => {
            host.displays().accept(snapshot);
            None
        }
        // The accepted snapshot stands. Windows still get their value
        // re-derived below, so a window that moved is not left behind.
        Err(error) => host
            .displays()
            .note_failure(error.to_string())
            .then_some(error),
    };
    deliver_displays(&host);
    failure.map_or(Ok(()), |error| Err(HostServicesError::Displays(error)))
}

/// Offer every followed window the display it is on according to the accepted
/// snapshot, each in its own containment so one failing window cannot cancel
/// the rest.
fn deliver_displays(host: &super::InstalledHost) {
    use super::owner_dispatch::{RuntimeEvent, RuntimeTask, dispatch_platform_ui_runtime};

    let mut first = None;
    for (dispatcher, update) in host.displays().updates() {
        super::installed_host::contain(&mut first, || {
            // A refused delivery means the window is gone; nothing is owed.
            let _ = dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::Event(RuntimeEvent::DisplayChanged(update)),
            );
        });
    }
    super::installed_host::finish(first);
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub(super) fn drive_native_text_sizing() {
    let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
    super::owner_dispatch::with_owner_callback(|_| {
        let current = APP_RUNTIME.with(|slot| slot.borrow().installed_host.same_host(&host));
        if current {
            let mut first = None;
            super::installed_host::contain(&mut first, || {
                if let Err(error) =
                    refresh_preferences_with(flui_native::OwnerPlatform::preferences)
                {
                    tracing::warn!(%error, "mobile system preference refresh failed");
                }
            });
            super::installed_host::contain(&mut first, || {
                if let Err(error) =
                    refresh_displays_with(flui_native::OwnerPlatform::display_snapshot)
                {
                    tracing::warn!(%error, "mobile display refresh failed");
                }
            });
            if APP_RUNTIME.with(|slot| slot.borrow().installed_host.same_host(&host)) {
                super::installed_host::contain(&mut first, || {
                    host.native()
                        .refresh_text_sizing(host.logical(), host.effects(), true);
                });
            }
            super::installed_host::finish(first);
        }
    });
}

/// The window closes this platform lets the application refuse (ADR-0260): a
/// desktop asks before a window closes; Android, iOS and the web never ask.
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
const WINDOW_CLOSE_VETOES: &[flui_view::CloseReason] = &[
    flui_view::CloseReason::User,
    flui_view::CloseReason::Program,
];
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
const WINDOW_CLOSE_VETOES: &[flui_view::CloseReason] = &[];

/// Every close the installed platform lets the application refuse: the window
/// closes, and the session's end where the platform holds the session for a
/// block (Windows, macOS). A platform that only delays it (logind) or only
/// tells cannot be refused, so a guard's holds never refuse it there.
fn close_vetoes() -> Vec<flui_view::CloseReason> {
    let mut vetoes = WINDOW_CLOSE_VETOES.to_vec();
    if with_owner_platform(flui_native::OwnerPlatform::session_end_support)
        == Some(flui_platform::SessionEndSupport::Hold)
    {
        vetoes.push(flui_view::CloseReason::SessionEnd);
    }
    vetoes
}

/// Builds a runner's UI runtime with its own window wake and redraw debt,
/// the platform capabilities and the app's font
/// collection and the host's byte storage. Every runner site and secondary
/// window builds its UI runtime through this one call, so a UI runtime cannot be
/// handed a stand-in for any of them.
pub(super) fn build_ui_runtime(
    window: impl Into<crate::app::presentation::PresentationWindow>,
    scale_factor: f64,
) -> Result<crate::app::ui_runtime::UiRuntime, HostServicesError> {
    let window = window.into();
    let redraw = Arc::new(AtomicBool::new(false));
    let pending = Arc::clone(&redraw);
    // The wake fires on any thread (an async task's waker, a signal written
    // on a worker): it asks the window through its requester (ADR-0254 §5).
    let target = window.window().redraw_requester();
    #[cfg(target_os = "ios")]
    let proxy = with_owner_platform(flui_native::OwnerPlatform::proxy);
    let wake = Arc::new(move || {
        pending.store(true, std::sync::atomic::Ordering::Release);
        target.request_redraw();
        #[cfg(target_os = "ios")]
        if let Some(proxy) = &proxy {
            let _ = proxy.wake();
        }
    });
    let preferences =
        APP_RUNTIME.with(|slot| slot.borrow().installed_host.logical().preferences())?;
    let fonts = runtime_font_collection();
    let mut host = crate::app::ui_runtime::RuntimeHostServices::new(
        wake,
        redraw,
        &fonts,
        flui_scheduler::ClockSource::Platform,
    )
    .with_capabilities(runtime_capabilities());
    if let Some(storage) = runtime_storage() {
        host = host.with_storage(storage);
    }
    if let Some(registry) = APP_RUNTIME.with(|slot| slot.borrow().flush_registry()) {
        host = host.with_flush_registry(registry);
    }
    host = host.with_close_vetoes(&close_vetoes());
    if let Some(transfer) = APP_RUNTIME.with(|slot| slot.borrow().data_transfer()) {
        host = host
            .with_data_transfer(transfer.source)
            .with_process_nonce(transfer.nonce);
    }
    if let Some(preferences) = preferences {
        host = host.with_preferences(preferences);
    }
    let native = Rc::clone(window.window());
    let ui_runtime = crate::app::ui_runtime::UiRuntime::new(window, scale_factor, host)?;
    attach_window_control(&ui_runtime, &native);
    Ok(ui_runtime)
}

/// Answer one phase of the user's session ending (ADR-0237, ADR-0260).
///
/// A query asks the close of every presentation whose UI runtime is in its
/// slot with [`CloseReason::SessionEnd`](flui_view::CloseReason::SessionEnd):
/// its close handler, then its guard. A UI runtime that is checked out is not
/// asked and counts as `Proceed`, since its published bytes are written when
/// the session ends. The query also becomes one
/// [`flui_platform::HostNotice::SessionEnding`] for every runtime, all sharing
/// one request kept until the session end is cancelled or happens, so its
/// blocks outlive the query. The answer is `Block` when a presentation
/// refused or a subscriber holds a block, `Proceed` otherwise. Subscribers of
/// a runtime whose turn is busy hear the notice later, after the request's
/// book has closed with the answer, so a block taken then is refused instead
/// of silently too late.
///
/// Cancelling withdraws only the session-end closes the query recorded.
/// Ending delivers the notice, writes the flush registry inline within
/// [`SESSION_END_BUDGET`](flui_view::__runtime::SESSION_END_BUDGET) and asks
/// the loop to quit. Both release what subscribers still hold, and when the
/// last block is released the platform is told
/// (`Platform::release_session_end`) so it can let the session go on.
fn answer_session_end(phase: flui_platform::SessionEndPhase) -> flui_platform::SessionEndAnswer {
    use flui_platform::{HostNotice, SessionEndAnswer, SessionEndPhase, SessionEndRequest};

    match phase {
        SessionEndPhase::Query => {
            let request = SessionEndRequest::new().on_release(|| {
                // The platform is told the application is done with the
                // session end it asked to wait for: every window that refused
                // carried its close out, and every subscriber let go.
                let _ = session_end_platform(flui_native::OwnerPlatform::release_session_end);
            });
            let refusals = refuse_session_end(&request);
            let displaced = APP_RUNTIME.with(|slot| {
                slot.try_borrow_mut().ok().map(|mut state| {
                    let request = state.session_end.replace(request.clone());
                    let refusals = std::mem::replace(&mut state.session_end_refusals, refusals);
                    (request, refusals)
                })
            });
            drop(displaced);
            // A subscriber that panics still gets the platform its answer, and
            // the book still closes: the panic is contained here, a native
            // callback boundary, and reported. A panicking subscriber holds
            // no block (R25).
            let delivery = deliver_contained(&HostNotice::SessionEnding(request.clone()));
            let held = request.answer();
            if let Some(delivery) = delivery
                && !delivery.is_complete()
            {
                tracing::warn!(
                    queued = delivery.queued,
                    refused = delivery.refused,
                    "a session end arrived while a UI runtime was busy; its subscribers hear it \
                     after the answer and cannot hold the session"
                );
            }
            if delivery.is_some() || has_session_end_refusals() {
                held
            } else {
                SessionEndAnswer::Proceed
            }
        }
        SessionEndPhase::Cancelled => {
            withdraw_session_end();
            drop(take_session_end());
            deliver_contained(&HostNotice::SessionEndCancelled);
            SessionEndAnswer::Proceed
        }
        SessionEndPhase::Ending => {
            let deadline = std::time::Instant::now() + session_end_budget();
            // A platform that only tells (Android's stop, the web's pagehide,
            // iOS) reports that the process may be killed, not that the
            // application is over: it may come back to the same UI. Only a
            // platform that holds or delays the session ends the
            // application here.
            let ends_application =
                session_end_platform(flui_native::OwnerPlatform::session_end_support)
                    .is_some_and(|support| support != flui_platform::SessionEndSupport::NotifyOnly);
            drop(take_session_end());
            // What the host keeps itself is written first, within the same
            // deadline as the documents.
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            super::placement::flush_all_by(deadline);
            // Application code runs while the budget can absorb it: a
            // subscriber or a Detached observer may still publish what the
            // flush then writes. A UI runtime that is checked out queues the
            // notice and runs none of its code before the process ends.
            if deadline.saturating_duration_since(std::time::Instant::now())
                >= SESSION_END_USER_CODE_THRESHOLD
            {
                deliver_contained(&HostNotice::SessionEnded);
                if ends_application {
                    deliver_session_end_close();
                }
            }
            flush_for_session_end(deadline);
            // The loop is asked to quit whatever ends the session, an end
            // that closes only this application included.
            if ends_application {
                let _ = session_end_platform(flui_native::OwnerPlatform::quit);
            }
            SessionEndAnswer::Proceed
        }
        // A phase this host does not know asks for nothing.
        _ => SessionEndAnswer::Proceed,
    }
}

/// The least of the session end's budget that must be left for a step that
/// runs application code to start, so the flush keeps time to write
/// (ADR-0260, "the 1 s threshold").
const SESSION_END_USER_CODE_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(1);

/// Whether a window still refuses the session end the host is answering.
fn has_session_end_refusals() -> bool {
    APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .is_ok_and(|state| !state.session_end_refusals.is_empty())
    })
}

/// The session end's request and the windows' refusals, taken from the loop
/// state; dropping them releases what they held.
fn take_session_end() -> Option<(
    Option<flui_platform::SessionEndRequest>,
    crate::app::runtime::SessionEndRefusals,
)> {
    APP_RUNTIME.with(|slot| {
        slot.try_borrow_mut().ok().map(|mut state| {
            (
                state.session_end.take(),
                std::mem::take(&mut state.session_end_refusals),
            )
        })
    })
}

/// The window at `address` carried out the session-end close it refused: its
/// refusal no longer holds the session. Once nothing else holds it, the
/// platform is released.
pub(super) fn release_session_end_refusal(address: flui_foundation::PresentationAddress) {
    let released = APP_RUNTIME.with(|slot| {
        slot.try_borrow_mut().ok().map(|mut state| {
            let (released, kept) = std::mem::take(&mut state.session_end_refusals)
                .into_iter()
                .partition::<Vec<_>, _>(|(refused, _)| *refused == address);
            state.session_end_refusals = kept;
            released
        })
    });
    // Outside the borrow: the last release tells the platform.
    drop(released);
}

/// Deliver the session's end to every UI runtime in its slot, containing and
/// reporting a panic: the caller answers a native callback.
fn deliver_session_end_close() {
    let mut failure = flui_foundation::panic::FirstPanic::new();
    failure.contain(super::owner_dispatch::deliver_session_end_close);
    if let Some(payload) = failure.into_payload() {
        tracing::error!(
            panic = flui_foundation::panic::payload_text(&*payload),
            "a Detached observer panicked at the session's end"
        );
        flui_foundation::panic::retain_opaque_payload(payload);
    }
}

/// The owner platform, for a session end. The operating system may report one
/// from a nested loop inside a frame (a modal dialog in a callback), where
/// [`with_owner_platform`] refuses: what is asked here (the platform's support
/// and its quit) touches no frame state. `None` while the loop state is busy
/// or no platform is installed.
fn session_end_platform<R>(f: impl FnOnce(&flui_native::OwnerPlatform) -> R) -> Option<R> {
    let owner = APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .ok()
            .and_then(|state| state.owner_platform.clone())
    });
    owner.as_deref().map(f)
}

/// The presentations a session-end query may ask: every registered one whose
/// UI runtime is in its slot. A checked-out runtime's code cannot run now, and
/// none is asked while the loop state itself is busy.
fn askable_presentations() -> Option<(
    Rc<crate::app::close_request::CloseRequestRouter>,
    Vec<flui_foundation::PresentationAddress>,
)> {
    APP_RUNTIME.with(|slot| {
        let state = slot.try_borrow().ok()?;
        let router = state.close_requests();
        let logical = state.installed_host.logical();
        let addresses = router
            .addresses()
            .into_iter()
            .filter(|address| {
                logical
                    .runtime_status(address.ui_runtime_id)
                    .is_ok_and(|status| !status.checked_out)
            })
            .collect();
        Some((router, addresses))
    })
}

/// Ask every askable presentation whether the session may end (ADR-0260 §1),
/// its handler first, then its guard, and take a block on `request` for each
/// one that refuses: the session stays held until that window carries its
/// close out, whatever the notice's subscribers do. Each refusal is also
/// recorded as a `PendingClose(SessionEnd)`. Where the platform does not
/// hold a session end for a block, nothing can be refused and nobody is
/// asked.
fn refuse_session_end(
    request: &flui_platform::SessionEndRequest,
) -> crate::app::runtime::SessionEndRefusals {
    if session_end_platform(flui_native::OwnerPlatform::session_end_support)
        != Some(flui_platform::SessionEndSupport::Hold)
    {
        return Vec::new();
    }
    let Some((router, addresses)) = askable_presentations() else {
        return Vec::new();
    };
    let mut refusals = Vec::new();
    for address in addresses {
        if router.consult_session_end(address) != crate::app::close_request::CloseResponse::KeepOpen
        {
            continue;
        }
        match request.block("a window waits for the user's decision") {
            Ok(block) => refusals.push((address, block)),
            Err(error) => tracing::warn!(?address, %error, "a window's refusal holds no block"),
        }
    }
    refusals
}

/// Withdraw the session-end closes a cancelled session end recorded, leaving
/// every other recorded close in place.
fn withdraw_session_end() {
    let Some(router) =
        APP_RUNTIME.with(|slot| slot.try_borrow().ok().map(|state| state.close_requests()))
    else {
        return;
    };
    for address in router.addresses() {
        router.withdraw(address, flui_view::CloseReason::SessionEnd);
    }
}

/// How long the framework's part of a session end may take: the budget, or
/// less where the platform waits a shorter bounded time.
fn session_end_budget() -> std::time::Duration {
    use flui_view::__runtime::SESSION_END_BUDGET;
    match session_end_platform(flui_native::OwnerPlatform::session_end_support) {
        Some(flui_platform::SessionEndSupport::Delay { max }) => max.min(SESSION_END_BUDGET),
        _ => SESSION_END_BUDGET,
    }
}

/// Write what the documents published, inline, by `deadline`: the process may
/// be ended at any moment once the session-end handler returns. Runs no
/// application code; a panic in the writer is contained and reported.
fn flush_for_session_end(deadline: std::time::Instant) {
    let flush = APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .ok()
            .and_then(|state| state.flush_host.clone())
    });
    let Some(flush) = flush else {
        return;
    };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        flush.flush_within(deadline)
    })) {
        Ok(report) if !report.is_complete() => tracing::warn!(
            failed = report.failed.len(),
            unfinished = report.unfinished.len(),
            "the session ended before the flush registry was written"
        ),
        Ok(_) => {}
        Err(payload) => {
            tracing::error!(
                panic = flui_foundation::panic::payload_text(&*payload),
                "the session-end flush panicked"
            );
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
}

/// Deliver a session-end notice to every runtime, containing a subscriber's
/// panic: the callers answer a native callback (`WM_QUERYENDSESSION`), where an
/// unwind would abort the process or skip the answer. The first panic is
/// reported and `None` is returned; otherwise how the notice was delivered.
fn deliver_contained(
    notice: &flui_platform::HostNotice,
) -> Option<super::owner_dispatch::NoticeDelivery> {
    let mut failure = flui_foundation::panic::FirstPanic::new();
    let delivery = failure.contain(|| super::owner_dispatch::deliver_host_notice(notice));
    if let Some(payload) = failure.into_payload() {
        tracing::error!(
            panic = flui_foundation::panic::payload_text(&*payload),
            "a session-end subscriber panicked; the session end is answered regardless"
        );
        flui_foundation::panic::retain_opaque_payload(payload);
    }
    delivery
}

/// Give the UI runtime's primary presentation the control of its window, so
/// `LifecycleContext::window` answers from the first `init_state` on: the root
/// is attached after this returns.
fn attach_window_control(
    ui_runtime: &crate::app::ui_runtime::UiRuntime,
    window: &Rc<dyn flui_platform::PlatformWindow>,
) {
    let (queues, wake, router) = APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        (
            Rc::clone(&state.window_commands),
            state.owner_turn_wake.clone(),
            state.close_requests(),
        )
    });
    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: ui_runtime.id(),
        presentation_id: ui_runtime.presentation_id(),
    };
    let wake = wake.unwrap_or_else(|| Rc::new(|| false));
    let source = crate::app::window_control::window_source(window, address, &queues, wake, router);
    let attached = ui_runtime.install_window(address.presentation_id, source);
    debug_assert!(attached, "a fresh UI runtime has its primary presentation");
}

// ============================================================================
// Loop-scoped composition root (ADR-0027, ADR-0039 §6)
// ============================================================================

thread_local! {
    /// The one loop-scoped composition root, shared by desktop, Android, and
    /// wasm. Absorbs what were, before the `AppRuntime` skeleton existed, two
    /// separate thread-locals: the transitional UI runtime host (UI runtime slot, queue,
    /// draining, owner thread, address cache, window registry, surface
    /// applier) and the loop-scoped `OwnerPlatform` host —
    /// see [`AppRuntime`]'s own module doc for why one struct correctly
    /// carries both invariants. A window's handler, the window itself and the
    /// application handler are owner-local (ADR-0254 §4, §5), but the owner
    /// turn the runner installs (`OwnerPlatform::on_wake`) is still a `Send`
    /// closure, so the `!Send`
    /// UI runtime this holds stays in owner TLS for them; access is only
    /// through the stamped FIFO dispatcher below and the fenced
    /// `with_owner_platform` accessor.
    ///
    /// `AppRuntime::new()` is cheap and side-effect-free (no singleton
    /// resolution) precisely so merely *touching* this thread-local -- for
    /// any reason, including `OwnerHostClearGuard::drop` firing during an
    /// unwind on a thread that never reached platform init -- can never
    /// itself trigger singleton construction or full system-font
    /// enumeration. Real service resolution happens only when a UI runtime is
    /// built or installed: `build_ui_runtime`, which every runner calls
    /// to build its UI runtime, or the explicit `ensure_services` call in
    /// the prepared installation entry points.
    pub(super) static APP_RUNTIME: std::cell::RefCell<AppRuntime> =
        std::cell::RefCell::new(AppRuntime::new());
}

/// Installs `owner` in the loop-scoped host. Call once, at the top of each
/// backend's `on_ready` callback (ADR-0039 §6) — every `run_*` entry point
/// in this module does so immediately after minting/receiving its
/// `OwnerPlatform`.
///
/// Deliberately does NOT resolve `SharedEngineServices`: `run_direct`
/// installs an owner platform and opens a window but never installs a
/// `UiRuntime` (no widget tree, no painting/semantics/scheduler singleton
/// reach at all), so resolving here would pay for singleton construction
/// and full system-font enumeration on a path that can never consume
/// either. UI runtime construction and prepared installation resolve these
/// services; merely installing the platform owner does not.
#[cfg_attr(
    any(target_os = "android", target_os = "ios"),
    expect(
        clippy::unnecessary_wraps,
        reason = "portable bootstrap keeps one fallible contract; desktop and web wake registration can fail, mobile only installs the owner"
    )
)]
pub(crate) fn install_owner_platform(
    owner: flui_native::OwnerPlatform,
) -> Result<(), flui_native::WakeRegistrationError> {
    #[cfg(not(target_os = "android"))]
    let owner_turn_wake: Rc<dyn Fn() -> bool> = {
        let proxy = owner.proxy();
        Rc::new(move || proxy.wake().is_ok())
    };
    #[cfg(target_os = "android")]
    let owner_turn_wake: Rc<dyn Fn() -> bool> = {
        let poke = APP_RUNTIME.with(|slot| slot.borrow().owner_turn_window_poke());
        Rc::new(move || poke())
    };
    #[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
    let identity = {
        let identity = Arc::new(());
        let installed_identity = Arc::clone(&identity);
        owner.on_wake(Box::new(move || {
            if APP_RUNTIME
                .with(|slot| Arc::ptr_eq(&slot.borrow().loop_identity, &installed_identity))
            {
                // The owner's turn starts with the commands application code
                // queued for its windows, before anything of this turn runs. A
                // command that panics does not cost the turn the rest of its
                // work: its panic is held apart, so the frame still runs, and
                // is raised when the turn ends.
                let mut commands = None;
                super::installed_host::contain(&mut commands, || {
                    super::owner_dispatch::apply_queued_window_commands(
                        std::thread::current().id(),
                    );
                });
                let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
                super::owner_dispatch::with_owner_callback(|_| {
                    let current = || {
                        APP_RUNTIME.with(|slot| {
                            let state = slot.borrow();
                            Arc::ptr_eq(&state.loop_identity, &installed_identity)
                                && state.installed_host.same_host(&host)
                        })
                    };
                    if !current() {
                        super::installed_host::finish(commands);
                        return;
                    }
                    let mut first = None;
                    super::installed_host::contain(&mut first, || {
                        if let Err(error) =
                            refresh_preferences_with(flui_native::OwnerPlatform::preferences)
                        {
                            tracing::warn!(%error, "system preference refresh failed");
                        }
                    });
                    super::installed_host::contain(&mut first, || {
                        if let Err(error) =
                            refresh_displays_with(flui_native::OwnerPlatform::display_snapshot)
                        {
                            tracing::warn!(%error, "display refresh failed");
                        }
                    });
                    #[cfg(not(target_arch = "wasm32"))]
                    if current() {
                        let recovery = if first.is_some() {
                            flui_runtime::owner::RecoveryState::PreservingFailure
                        } else {
                            flui_runtime::owner::RecoveryState::Healthy
                        };
                        super::installed_host::contain(&mut first, || {
                            super::secondary_window::drain_pending_secondary_window_completions(
                                recovery,
                            );
                        });
                        if first.is_none() && current() {
                            super::installed_host::contain(
                                &mut first,
                                super::main_window::drive_main_window,
                            );
                            super::installed_host::contain(
                                &mut first,
                                super::main_window::drive_program_quit,
                            );
                        }
                    }
                    flui_foundation::panic::FirstPanic::fold(&mut commands, first);
                    super::installed_host::finish(commands);
                });
            }
        }))?;
        identity
    };
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    super::main_window::shutdown_main_window();
    // The platform clipboard is installed with the owner, so every ui_runtime
    // a runner builds afterwards acquires it (`runtime_capabilities`).
    let clipboard = owner.clipboard();
    let data_transfer = owner.shared().data_transfer();
    let (previous, previous_owner_turn_wake, previous_host, previous_clipboard) =
        APP_RUNTIME.with(|slot| {
            let mut state = slot.borrow_mut();
            state.owner_install_generation = state
                .owner_install_generation
                .checked_add(1)
                .expect("BUG: owner install generation exhausted");
            // The displaced source retires with the clipboard, outside this borrow.
            let previous_clipboard = (
                state.set_platform_clipboard(clipboard),
                state.set_platform_data_transfer(data_transfer),
            );
            let previous_host =
                std::mem::replace(&mut state.installed_host, super::InstalledHost::new());
            let previous_owner_turn_wake = state.owner_turn_wake.replace(owner_turn_wake);
            let previous = state.owner_platform.replace(std::rc::Rc::new(owner));
            #[cfg(all(not(target_os = "android"), not(target_os = "ios")))]
            {
                state.loop_identity = identity;
            }
            #[cfg(all(
                not(target_os = "android"),
                not(target_os = "ios"),
                not(target_arch = "wasm32")
            ))]
            {
                state.quit_notification = crate::app::runtime::QuitNotification::Active;
                // A loop that wants other renderers says so after this install.
                state.window_renderers = super::window_renderer::WindowRenderers::default();
                state.pending_window_reservations =
                    Arc::new(std::sync::atomic::AtomicUsize::new(0));
            }
            (
                previous,
                previous_owner_turn_wake,
                previous_host,
                previous_clipboard,
            )
        });
    // Publish the new incarnation before retiring old callbacks and native
    // resources. No outgoing destructor runs under the composition-root borrow.
    let mut first = None;
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    super::installed_host::contain(&mut first, install_wake_deadline_hook);
    super::installed_host::contain(&mut first, || {
        if let Err(error) = refresh_preferences_with(flui_native::OwnerPlatform::preferences) {
            tracing::warn!(%error, "initial system preference observation failed");
        }
    });
    super::installed_host::contain(&mut first, || {
        if let Err(error) = refresh_displays_with(flui_native::OwnerPlatform::display_snapshot) {
            tracing::warn!(%error, "initial display observation failed");
        }
    });
    super::installed_host::contain(&mut first, || previous_host.shutdown());
    super::installed_host::contain(&mut first, || drop(previous_host));
    super::installed_host::contain(&mut first, || drop(previous_owner_turn_wake));
    super::installed_host::contain(&mut first, || drop(previous_clipboard));
    super::installed_host::contain(&mut first, || drop(previous));
    super::installed_host::finish(first);
    Ok(())
}

// ============================================================================
// The application handler of a run (ADR-0254 §4)
// ============================================================================

/// What a runner's bootstrap runs with: the owner capability, by value.
type Bootstrap =
    Box<dyn FnOnce(flui_native::OwnerPlatform) -> Result<(), flui_native::BootstrapError>>;

/// Where the application handler of a run sends what the platform tells and
/// asks it beyond its bootstrap, set by the runner as its bootstrap installs
/// what each one reaches (the main window, the UI runtimes' wake deadlines, a
/// mobile runner's own drivers). The handler owns its routes, so a run never
/// answers with what another set: a platform run from inside a handler, as a
/// test does, routes to its own handler and leaves the outer run's routes as
/// they were.
///
/// A route the runner never sets answers as a run that cares about it not: a
/// quit or a reopen reaches nothing, the exit is left to the host, as with no
/// handler bound ([`ExitAnswer::HostDecides`](flui_native::ExitAnswer)), and
/// no deadline is asked for.
#[derive(Default)]
pub(crate) struct AppRoutes {
    quit: Option<Rc<dyn Fn()>>,
    reopen: Option<Rc<dyn Fn()>>,
    exit_policy: Option<Rc<dyn Fn() -> bool>>,
    wake_deadline: Option<Rc<dyn Fn() -> Option<web_time::Instant>>>,
}

/// Replace one route of the innermost run on this thread whose handler is
/// alive; the one it displaces goes outside every borrow. With no run, there
/// is no handler to answer through, and the route is let go.
fn set_route<T: ?Sized>(project: fn(&mut AppRoutes) -> &mut Option<Rc<T>>, route: Rc<T>) {
    let routes = APP_RUNTIME.with(|slot| {
        let runs = &mut slot.borrow_mut().run_routes;
        runs.retain(|routes| routes.strong_count() > 0);
        runs.last().and_then(std::rc::Weak::upgrade)
    });
    let Some(routes) = routes else {
        tracing::debug!("a route set outside any run has no handler to answer through");
        return;
    };
    let displaced = project(&mut routes.borrow_mut()).replace(route);
    drop(displaced);
}

/// Send the platform's quit to `route`.
pub(crate) fn route_quit(route: impl Fn() + 'static) {
    set_route(|routes| &mut routes.quit, Rc::new(route) as Rc<dyn Fn()>);
}

/// Send the user's reopen (AppKit's Dock click) to `route`.
#[cfg_attr(
    any(
        target_arch = "wasm32",
        all(not(test), any(target_os = "android", target_os = "ios"))
    ),
    expect(dead_code, reason = "only the desktop main window reopens")
)]
pub(super) fn route_reopen(route: impl Fn() + 'static) {
    set_route(|routes| &mut routes.reopen, Rc::new(route) as Rc<dyn Fn()>);
}

/// Answer the platform's exit question with `route`.
pub(crate) fn route_exit_policy(route: impl Fn() -> bool + 'static) {
    set_route(
        |routes| &mut routes.exit_policy,
        Rc::new(route) as Rc<dyn Fn() -> bool>,
    );
}

/// Answer the platform's wake-deadline question with `route`.
#[cfg_attr(
    all(target_arch = "wasm32", not(test)),
    expect(dead_code, reason = "the web host has no idle wait to bound")
)]
pub(super) fn route_wake_deadline(route: impl Fn() -> Option<web_time::Instant> + 'static) {
    set_route(
        |routes| &mut routes.wake_deadline,
        Rc::new(route) as Rc<dyn Fn() -> Option<web_time::Instant>>,
    );
}

/// The instant the running loop is asked to wake at, as a desktop loop asks
/// before it waits: the answer of the route [`route_wake_deadline`] set.
#[cfg(all(
    feature = "testing",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(crate) fn loop_wake_deadline() -> Option<web_time::Instant> {
    let routes = APP_RUNTIME.with(|slot| {
        let runs = &slot.borrow().run_routes;
        runs.last().and_then(std::rc::Weak::upgrade)
    })?;
    let route = routes.borrow().wake_deadline.clone()?;
    route()
}

/// The application handler `flui-app` runs a platform with: `ready` runs the
/// runner's bootstrap, a session end and a host notice reach every UI runtime
/// on the thread, and the rest goes to the routes the bootstrap set
/// ([`AppRoutes`]).
///
/// The handler owns its routes; its bootstrap, and the run's code after it,
/// set them through the `route_*` functions, which reach the innermost run on
/// the thread whose handler is alive. A run from inside a handler (as a test
/// does) gets its own routes and leaves the outer run's as they were; a
/// bootstrap that fails before setting one answers with none of another
/// run's.
struct RunnerHandler {
    bootstrap: std::cell::RefCell<Option<Bootstrap>>,
    routes: Rc<std::cell::RefCell<AppRoutes>>,
}

/// The application handler a runner passes to `Platform::run`, with
/// `bootstrap` as its `ready`.
pub(crate) fn app_handler(
    bootstrap: impl FnOnce(flui_native::OwnerPlatform) -> Result<(), flui_native::BootstrapError>
    + 'static,
) -> Rc<dyn flui_native::AppHandler> {
    Rc::new(RunnerHandler {
        bootstrap: std::cell::RefCell::new(Some(Box::new(bootstrap))),
        routes: Rc::default(),
    })
}

impl RunnerHandler {
    /// The route `pick` names. Setting a route borrows the routes only to
    /// swap it in, and this only to clone it, so neither ever finds them
    /// borrowed.
    fn route<T: ?Sized>(&self, pick: impl FnOnce(&AppRoutes) -> Option<Rc<T>>) -> Option<Rc<T>> {
        pick(&self.routes.borrow())
    }
}

// Every method's whole body runs under one catch, as `WindowRoutes`' do
// (ADR-0258 §4).
impl flui_native::AppHandler for RunnerHandler {
    fn ready(&self, owner: flui_native::OwnerPlatform) -> Result<(), flui_native::BootstrapError> {
        // A bootstrap that panicked quits the loop it started; the exit
        // resumes the panic.
        contained(Ok(()), "application bootstrap", || {
            let bootstrap = self
                .bootstrap
                .borrow_mut()
                .take()
                .expect("BUG: a platform runs its bootstrap once");
            // From here the `route_*` functions reach this run's routes.
            APP_RUNTIME.with(|slot| {
                slot.borrow_mut()
                    .run_routes
                    .push(Rc::downgrade(&self.routes));
            });
            bootstrap(owner)
        })
    }

    fn quit_requested(&self, _: &flui_native::OwnerPlatform) {
        contained((), "application quit", || {
            if let Some(quit) = self.route(|routes| routes.quit.clone()) {
                quit();
            }
        });
    }

    fn reopen(&self, _: &flui_native::OwnerPlatform) {
        contained((), "application reopen", || {
            if let Some(reopen) = self.route(|routes| routes.reopen.clone()) {
                reopen();
            }
        });
    }

    fn session_end(
        &self,
        _: &flui_native::OwnerPlatform,
        phase: flui_platform::SessionEndPhase,
    ) -> flui_platform::SessionEndAnswer {
        contained(
            flui_platform::SessionEndAnswer::Proceed,
            "session end",
            || answer_session_end(phase),
        )
    }

    fn host_notice(&self, _: &flui_native::OwnerPlatform, notice: flui_platform::SystemNotice) {
        // System notices (ADR-0237): every UI runtime's subscribers receive
        // them from the owner thread.
        contained((), "host notice", || {
            super::owner_dispatch::deliver_host_notice(&notice.into());
        });
    }

    fn exit_policy(&self, _: &flui_native::OwnerPlatform) -> flui_native::ExitAnswer {
        // A failure here quits the loop anyway.
        contained(flui_native::ExitAnswer::End, "exit policy", || {
            match self.route(|routes| routes.exit_policy.clone()) {
                Some(exit) if exit() => flui_native::ExitAnswer::End,
                Some(_) => flui_native::ExitAnswer::KeepRunning,
                None => flui_native::ExitAnswer::HostDecides,
            }
        })
    }

    fn wake_deadline(&self, _: &flui_native::OwnerPlatform) -> Option<web_time::Instant> {
        contained(None, "wake deadline", || {
            self.route(|routes| routes.wake_deadline.clone())
                .and_then(|deadline| deadline())
        })
    }
}

/// Routes the platform's exit question to this thread's `AppRuntime`, instead
/// of letting a platform backend decide, alone, whether "every window this
/// backend tracks just closed" means "exit" — the live-loop wiring `issue
/// #555`'s `ExitPolicy`/`AppRuntime::should_exit` names as its own
/// deliberately-deferred follow-up (see `ExitPolicy`'s own doc). Call once,
/// immediately after [`install_owner_platform`], from every backend that
/// wants this: today, that is `run_desktop` only — Android, the web and iOS
/// never ask the exit policy.
///
/// The route re-enters `APP_RUNTIME` only when the platform asks (a window
/// closing) — never synchronously from this function — so this does not
/// violate `with_owner_platform`'s "no host re-entry" rule.
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
        reason = "run_desktop (its one caller) is desktop-only -- android/wasm32 bootstraps \
                  never call this"
    )
)]
#[cfg_attr(
    all(test, target_os = "android"),
    expect(
        dead_code,
        reason = "desktop exit-policy tests are excluded on Android"
    )
)]
pub(super) fn install_exit_policy_hook(policy: ExitPolicy) {
    route_exit_policy(move || {
        // Retirement completes in the installed host before requesting this
        // policy check. Pending publications also veto automatic exit.
        let should_exit = APP_RUNTIME.with(|slot| slot.borrow_mut().should_exit(policy));
        #[cfg(all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        ))]
        if should_exit {
            let ingress = APP_RUNTIME.with(|slot| slot.borrow().main_ingress.clone());
            if let Some(ingress) = ingress {
                return ingress.try_auto_quit();
            }
        }
        should_exit
    });
    let shared = with_owner_platform(flui_native::OwnerPlatform::shared);

    // The exit policy is otherwise only asked when a window closes, so a
    // veto owed to a running keep-alive service (issue #558) would be
    // PERMANENT once the last window is gone — nothing left to close,
    // nothing to re-ask, and the process would linger forever. Close that
    // loop: when a keep-alive service reports its exit (on whatever worker
    // thread it ran on), request the platform's coalesced, owner-thread
    // re-consultation of the same route. Spurious fires are harmless by
    // contract (windows still open, or the route still vetoing, are
    // no-ops), and backends without the mechanism default it to inert.
    // Registered outside the `with_owner_platform` borrow: the notifier
    // installation touches `APP_RUNTIME`, which must never run while the
    // owner-platform host is checked out. `None` (no owner platform on this
    // thread) leaves nothing to wire. Not compiled on wasm32, where the
    // lifecycle layer (and so the notifier seam) does not exist.
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(shared) = shared {
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .set_lifecycle_exit_notifier(std::sync::Arc::new(move || {
                    shared.request_exit_policy_reevaluation();
                }));
        });
    }
    #[cfg(target_arch = "wasm32")]
    drop(shared);
}

/// Route the desktop platform's quit to the main window and the quit
/// notification.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn install_platform_quit_hook() {
    let loop_identity = APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().loop_identity));
    route_quit(move || {
        if !APP_RUNTIME.with(|slot| Arc::ptr_eq(&slot.borrow().loop_identity, &loop_identity)) {
            return;
        }
        super::main_window::shutdown_main_window();
        tracing::info!("Platform quit");
        super::owner_dispatch::request_quit_notification();
    });
}

/// Routes the platform's wake-deadline question, asked once per idle
/// iteration, to this thread's runtimes: `AppRuntime::next_wake` folds every
/// live runtime and presentation's deadlines. Installed once with the desktop
/// owner platform; a window install never replaces it, and closing a window
/// withdraws only its contribution. Android and iOS route their own drivers;
/// the web has no idle wait.
///
/// The route re-enters `APP_RUNTIME` only when the platform asks
/// (`about_to_wait`) — never synchronously from this function — and only
/// reads it (`AppRuntime::next_wake` takes `&self`).
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn install_wake_deadline_hook() {
    let identity = APP_RUNTIME.with(|slot| Arc::clone(&slot.borrow().loop_identity));
    route_wake_deadline(move || {
        APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            if Arc::ptr_eq(&state.loop_identity, &identity) {
                state.next_wake()
            } else {
                None
            }
        })
    });
}

/// The earliest armed source; an absent source contributes no deadline.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn merge_wake_deadlines(
    a: Option<web_time::Instant>,
    b: Option<web_time::Instant>,
) -> Option<web_time::Instant> {
    [a, b].into_iter().flatten().min()
}

/// Gate a presentation's visual deadline on its current frame eligibility.
///
/// `frames_enabled == false` suppresses the deadline unconditionally,
/// regardless of how soon it is due: while frames are disabled
/// (`AppLifecycleState::Hidden`/`Paused`/`Detached`), the frame closure's
/// `WakeAction::PumpBackground` arm returns before `render_frame_with_device_
/// recovery` ever runs, so nothing on that path would consume a reported
/// deadline — reporting it anyway hands `about_to_wait` the SAME past
/// instant on every idle iteration once it comes due, which is
/// `WinitApp::new_events`'s own named `WaitUntil(past)` busy-spin, forced by
/// this hook instead of a stale UI runtime deadline. The deadline is not lost by
/// staying unreported while disabled: presentation lifecycle reconciliation
/// redirties the restored root and wakes the loop through the ordinary
/// `needs_redraw` channel, which
/// lets a real `WakeAction::Render` resume the retry then.
// Desktop-only, like its sole caller `bootstrap_desktop`: the mobile backends
// have no `ControlFlow`/`WaitUntil` to feed and wasm has no loop, so compiling
// it on any of them is dead code the `-D warnings` cross-target gates reject.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn desktop_secondary_wake_deadline(
    next_attempt_at: Option<web_time::Instant>,
    frames_enabled: bool,
) -> Option<web_time::Instant> {
    if frames_enabled {
        next_attempt_at
    } else {
        None
    }
}

/// Borrow-style access to the loop-scoped owner-platform capability.
/// `None` if no `OwnerPlatform` is currently installed on this thread
/// (before `on_ready`, or after the host was cleared).
///
/// Three fences (ADR-0039 §6), all landing in this one accessor:
///
/// (a) **Borrowed public access.** `pub(crate)` to `flui-app`'s `app` module,
///     never re-exported; `OwnerPlatform` isn't `Clone`, so there is no way
///     to escape this closure with a durable owned copy — every access
///     re-crosses the fence.
/// (b) **Scope rule.** Never call it from inside
///     `build`/`perform_layout`/`paint`/composite bodies. This is a free
///     function, not a `LifecycleContext` method, so no type withholds it
///     from a frame phase; (c) is the check.
/// (c) **Runtime backstop.** `debug_assert!`s that the installed UI runtime's own
///     scheduler (`AppRuntime::installed_ui_runtime_phase`) is not inside the
///     frame transaction. "Not inside a frame phase" per the ADR means
///     `TransientCallbacks`/`MidFrameMicrotasks`/`PersistentCallbacks` are
///     forbidden; `Idle` and `PostFrameCallbacks` are allowed (legitimate
///     ADR-0021-style post-frame work); `None` (truly no UI runtime installed,
///     and none currently dispatched — see `OwnerHost::phase`)
///     holds vacuously. `installed_ui_runtime_phase` reads through to the
///     checked-out UI runtime's scheduler for the entire extent of a
///     `dispatch_platform_ui_runtime` call, not only the resident-UI runtime case, so
///     this fence is load-bearing during a real dispatched production frame,
///     not merely when the UI runtime sits untouched in the slot. This fence is
///     still **vacuous on binding-local frame paths**: headless/test
///     bindings drive their own binding-local `UpdateScheduler`, never a UI runtime
///     installed into `APP_RUNTIME`, so fences (a) and (b) are the
///     load-bearing ones there — stated here, not hidden.
///
/// # `owner.shared()`'s method list is a compile-time fence too
///
/// Code reached through this accessor that isn't owner-thread-affine goes
/// through [`OwnerPlatform::shared`](flui_native::OwnerPlatform::shared),
/// which returns `SharedPlatform` — a type whose method list IS the fence
/// (no owner-affine method, e.g. `open_window`, is ever added to it; see
/// its own rustdoc). The failing and passing examples differ only in the
/// window-opening receiver: the shared residual has no `open_window`, while
/// the owner capability does.
///
/// ```compile_fail,E0599
/// use std::rc::Rc;
///
/// use flui_native::OwnerPlatform;
/// use flui_platform::{WindowHandler, WindowOptions};
///
/// fn open(owner: &OwnerPlatform, options: WindowOptions, handler: Rc<dyn WindowHandler>) {
///     let _ = owner.shared().open_window(options, handler);
/// }
/// ```
///
/// ```
/// use std::rc::Rc;
///
/// use flui_native::OwnerPlatform;
/// use flui_platform::{WindowHandler, WindowOptions};
///
/// fn open(owner: &OwnerPlatform, options: WindowOptions, handler: Rc<dyn WindowHandler>) {
///     let _ = owner.open_window(options, handler);
/// }
/// ```
///
/// # Native re-entry
///
/// The runtime owns one `Rc<OwnerPlatform>`. The accessor takes a temporary
/// internal strong reference, releases the TLS borrow, then invokes `f`.
/// Native calls may synchronously dispatch focus/close events into the runtime.
/// Public callers still receive only `&OwnerPlatform`; neither `Clone` nor
/// cross-thread ownership is added to that capability. A native operation's
/// result must separately be checked against its loop identity and admission.
pub(crate) fn with_owner_platform<R>(
    f: impl FnOnce(&flui_native::OwnerPlatform) -> R,
) -> Option<R> {
    #[cfg(debug_assertions)]
    {
        // A sequential, separate `.with()` borrow -- released before the
        // real one below opens -- so this never re-enters the same
        // `RefCell`. `None` (no ui_runtime installed on this thread) is vacuous
        // but truthful: no ui_runtime means no frame transaction can be in
        // flight here, so the asserted property holds trivially.
        let phase = APP_RUNTIME.with(|slot| slot.borrow().installed_ui_runtime_phase());
        debug_assert!(
            !matches!(
                phase,
                Some(
                    flui_scheduler::SchedulerPhase::TransientCallbacks
                        | flui_scheduler::SchedulerPhase::MidFrameMicrotasks
                        | flui_scheduler::SchedulerPhase::PersistentCallbacks
                )
            ),
            "BUG: with_owner_platform called while the installed ui_runtime's scheduler is inside \
             the frame transaction (phase {phase:?}) -- owner_platform must \
             not be acquired from build/layout/paint (ADR-0039 §6)"
        );
    }
    let owner = APP_RUNTIME.with(|slot| slot.borrow().owner_platform.clone());
    owner.as_deref().map(f)
}

/// Unwind-safe TLS clearing. Arm this guard *before* calling
/// `Platform::run(...)` on any backend whose `run` returns (winit,
/// headless, Android, AppKit) — not inside `on_ready` — so a panic anywhere inside
/// `on_ready` or later in `run` unwinds through the guard's `Drop` and
/// cannot leak the host into whatever runs on this thread next (notably,
/// the next test). Clearing an already-empty slot is a no-op.
///
/// Web deliberately arms no guard: the host stays resident for the page's
/// lifetime (see the web runner's own comment on this). Standalone AppKit
/// stops its native loop and returns through this guard like other desktops.
///
/// # Cleanup outside borrows, and no eager resolution
///
/// `Drop` takes the owner reference out of TLS and releases it only after the
/// borrow ends. Captured native resources can therefore re-enter cleanup.
/// Just as importantly, `AppRuntime::new()` is cheap and
/// side-effect-free specifically so that a bare `.borrow_mut()` here — the
/// *first* touch of `APP_RUNTIME` on a thread whose `on_ready` panicked
/// before installing anything — can never trigger `SharedEngineServices`
/// resolution (singleton construction plus full system-font enumeration)
/// while already unwinding. A panic during that resolution, on top of the
/// panic already unwinding, would abort the process instead of propagating
/// the original failure.
#[must_use = "the guard must stay alive across the Platform::run(...) call it \
              guards, or the TLS host clears immediately instead of at loop exit"]
pub(crate) struct OwnerHostClearGuard {
    expected_generation: u64,
}

impl OwnerHostClearGuard {
    /// Own the first successful owner installation after this point. A later
    /// replacement has another generation and cannot be erased by this guard.
    /// Call immediately before `Platform::run(...)`; no-install failure leaves
    /// a previously installed owner untouched.
    pub(crate) fn arm() -> Self {
        let expected_generation = APP_RUNTIME.with(|slot| {
            slot.borrow()
                .owner_install_generation
                .checked_add(1)
                .expect("BUG: owner install generation exhausted")
        });
        Self {
            expected_generation,
        }
    }
}

impl Drop for OwnerHostClearGuard {
    fn drop(&mut self) {
        let (removed, owner_turn_wake, storage) = APP_RUNTIME.with(|slot| {
            let mut runtime = slot.borrow_mut();
            if runtime.owner_install_generation == self.expected_generation {
                (
                    runtime.owner_platform.take(),
                    runtime.owner_turn_wake.take(),
                    runtime.host_storage.take(),
                )
            } else {
                (None, None, None)
            }
        });
        drop(removed);
        drop(owner_turn_wake);
        drop(storage);
    }
}

#[cfg(test)]
#[path = "app_handler_tests.rs"]
mod app_handler_tests;
