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

/// The app's font collection for [`crate::app::ui_runtime::UiRuntime::try_new`]'s
/// `fonts` parameter (ADR-0092 §2). Same borrow rule as
/// [`runtime_wake_callback`]. The first call on a thread resolves the shared
/// engine services.
pub(super) fn runtime_font_collection() -> flui_painting::FontCollection {
    APP_RUNTIME.with(|slot| slot.borrow().font_collection())
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
/// executors are installed. Without a directory to name
/// ([`storage_dir`]) widgets are refused storage as not registered. A native
/// desktop host always answers a store, whose calls are refused when its
/// platform names no data folder (found at the store's first call, on its IO
/// thread); only an Android Activity that reports no internal data path
/// answers none, and widgets are then refused storage as not on this
/// platform.
pub(super) fn install_host_storage(config: &crate::app::AppConfig) {
    let refused =
        |reason| flui_platform::Unsupported::of::<flui_view::persist::StorageCapability>(reason);
    let store = storage_dir(config)
        .ok_or_else(|| refused(flui_platform::UnsupportedReason::NotRegistered))
        .and_then(|dir| {
            with_owner_platform(|owner| owner.data_store(&dir))
                .flatten()
                .ok_or_else(|| {
                    tracing::warn!(
                        storage_dir = dir.as_str(),
                        "the Activity reports no internal data path; widgets get no storage"
                    );
                    refused(flui_platform::UnsupportedReason::NotOnThisPlatform)
                })
        });
    // Always installed, a refusal included: a run without storage must not
    // inherit the previous run's on this thread.
    APP_RUNTIME.with(|slot| slot.borrow_mut().install_host_storage(store));
    // The run's receivers replace the last run's, for the same reason.
    let memory = APP_RUNTIME.with(|slot| Rc::clone(&slot.borrow().host_memory));
    memory.replace_receivers(config.memory_reclaimers.0.clone());
}

/// The host's own byte storage ([`install_host_storage`]), whatever storage
/// the run's capability registry gives widgets; `None` without one.
/// `APP_RUNTIME` must not be mutably borrowed when this is called.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn host_storage() -> Option<Arc<dyn flui_platform::Storage>> {
    APP_RUNTIME.with(|slot| slot.borrow().application.host_storage.current())
}

/// The directory the run's storage lives in under the data roots: the one
/// [`AppConfig::storage_dir`](crate::app::AppConfig::storage_dir)
/// names, else the application's identifier, else, where the roots belong to
/// this application alone, [`SANDBOXED_STORAGE_DIR`]. `None` on a desktop
/// application with neither, whose roots every application shares.
pub(crate) fn storage_dir(config: &crate::app::AppConfig) -> Option<flui_platform::AppDataDir> {
    if let Some(dir) = &config.storage_dir {
        return Some(dir.clone());
    }
    if let Some(app_id) = config.application_identity.app_id() {
        let dir = flui_platform::AppDataDir::try_new(app_id.as_str())
            .expect("BUG: an application identifier is a valid directory name");
        return Some(dir);
    }
    SANDBOXED.then(|| {
        flui_platform::AppDataDir::try_new(SANDBOXED_STORAGE_DIR)
            .expect("BUG: the sandboxed storage directory name is valid")
    })
}

/// Whether the platform's data roots belong to this application alone: the
/// Android app's private directories, the iOS app container, the page's
/// origin on the Web.
const SANDBOXED: bool = cfg!(any(
    target_os = "android",
    target_os = "ios",
    target_arch = "wasm32"
));

/// The storage directory of an application without an identifier, where the
/// data roots are its own.
pub(crate) const SANDBOXED_STORAGE_DIR: &str = "app";

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
    // The fallback order follows the first locale the host prefers
    // (ADR-0271 §4), once the snapshot carrying it is accepted. Delivery may
    // re-enter the owner and accept a newer snapshot, or replace the host;
    // the read started here then sends nothing.
    let locale = values
        .locales()
        .and_then(<[flui_platform::Locale]>::first)
        .map(flui_platform::Locale::to_language_tag);
    let preference_read = APP_RUNTIME.with(|slot| slot.borrow().begin_preference_read());
    // The host learns of the delivery only once it is accepted: a read whose
    // delivery is dropped above or fails here is read again in the next
    // owner turn (ADR-0378).
    let delivered = values.clone();
    host.logical().update_preferences(values, host.effects())?;
    owner.preferences_delivered(&delivered);
    if let Some(tag) = locale {
        let wake = APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            if state.installed_host.same_host(&host) {
                state.follow_locale(preference_read, tag)
            } else {
                None
            }
        });
        // Outside the borrow: the wake reaches the app's redraw requester.
        if let Some(wake) = wake {
            wake();
        }
    }
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
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    let accepted = observation.is_ok();
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
    // A window's placement is kept relative to its display's usable area,
    // which a new snapshot may have moved under a window that did not move.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    if accepted {
        super::placement::displays_changed();
    }
    failure.map_or(Ok(()), |error| Err(HostServicesError::Displays(error)))
}

/// Offer every followed window the display it is on according to the accepted
/// snapshot, each in its own containment so one failing window cannot cancel
/// the rest.
fn deliver_displays(host: &super::InstalledHost) {
    use super::owner_dispatch::{RuntimeEvent, RuntimeTask, dispatch_platform_ui_runtime};

    let mut first = flui_foundation::panic::FirstPanic::new();
    for (dispatcher, update) in host.displays().updates() {
        first.contain(|| {
            // A refused delivery means the window is gone; nothing is owed.
            let _ = dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::Event(RuntimeEvent::DisplayChanged(update)),
            );
        });
    }
    first.resume();
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub(super) fn drive_native_text_sizing() {
    let host = APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
    super::owner_dispatch::with_owner_callback(|_| {
        let current = APP_RUNTIME.with(|slot| slot.borrow().installed_host.same_host(&host));
        if current {
            let mut first = flui_foundation::panic::FirstPanic::new();
            first.contain(|| {
                if let Err(error) =
                    refresh_preferences_with(flui_native::OwnerPlatform::preferences)
                {
                    tracing::warn!(%error, "mobile system preference refresh failed");
                }
            });
            first.contain(|| {
                if let Err(error) =
                    refresh_displays_with(flui_native::OwnerPlatform::display_snapshot)
                {
                    tracing::warn!(%error, "mobile display refresh failed");
                }
            });
            if APP_RUNTIME.with(|slot| slot.borrow().installed_host.same_host(&host)) {
                first.contain(|| {
                    host.native()
                        .refresh_text_sizing(host.logical(), host.effects(), true);
                });
            }
            first.resume();
        }
    });
}

/// Every close the installed platform lets the application refuse
/// ([`close_vetoes`](flui_runtime::owner_turn::close_vetoes)), in the
/// runner's [`window_system`](super::window_system).
fn close_vetoes() -> Vec<flui_view::CloseReason> {
    flui_runtime::owner_turn::close_vetoes(
        super::window_system(),
        with_owner_platform(flui_native::OwnerPlatform::session_end_support),
    )
}

/// Builds a runner's UI runtime with its own window wake and redraw debt,
/// the platform capabilities and the app's font
/// collection and the host's byte storage. Every runner site and secondary
/// window builds its UI runtime through this one call, so a UI runtime cannot be
/// handed a stand-in for any of them.
///
/// It also opens the URLs held for the window's presentation in the runtime,
/// on loan to the installation: the caller hands the guard to the
/// installation it prepares (`WindowInstall::lend_opened`), and a return
/// before that drops it, which gives the URLs back.
pub(super) fn build_lending_ui_runtime(
    window: impl Into<crate::app::presentation::PresentationWindow>,
    scale_factor: f64,
    is_main_window: bool,
) -> Result<
    (
        crate::app::ui_runtime::UiRuntime,
        Option<super::opened::OpenedLoan>,
    ),
    HostServicesError,
> {
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
    if let Some(registry) = APP_RUNTIME.with(|slot| slot.borrow().flush_registry()) {
        host = host.with_flush_registry(registry);
    }
    host = host.with_close_vetoes(&close_vetoes());
    // The loop's rule for the in-flight floors it owes, which its wake reads
    // (`AppRuntime::next_wake`): the runtime's half of what keeps the
    // application from being idle reads the same one.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let raster_host = APP_RUNTIME.with(|slot| slot.borrow().raster_host.clone());
        host = host.with_floor_owed(move |address| raster_host.floor_owed(address));
    }
    if let Some(transfer) = APP_RUNTIME.with(|slot| slot.borrow().data_transfer()) {
        host = host
            .with_data_transfer(transfer.source)
            .with_process_nonce(transfer.nonce);
    }
    if let Some(preferences) = preferences {
        host = host.with_preferences(preferences);
    }
    let native = Rc::clone(window.window());
    let ui_runtime = crate::app::ui_runtime::UiRuntime::try_new(window, scale_factor, host)?;
    attach_window_control(&ui_runtime, &native);
    #[cfg(not(target_arch = "wasm32"))]
    feed_service_lifecycle(&ui_runtime);
    // Before its first presentation can hear an opened URL, and before its
    // root mounts, so a link that launched the application opens its first
    // Router on the link. The links are on loan to the installation, which
    // the caller hands the guard to (`WindowInstall::lend_opened`).
    let loan = current_open_routing().and_then(|routing| {
        routing.admit_in(&ui_runtime);
        routing.open_held_in(&ui_runtime, native.as_ref(), is_main_window)
    });
    Ok((ui_runtime, loan))
}

/// Feed the lifecycle the application's services watch from `ui_runtime`'s
/// scheduler: its state now, each change it commits, and its retirement,
/// when the scheduler drops the listener with the runtime. The services'
/// lifecycle holds the subscription until then.
#[cfg(not(target_arch = "wasm32"))]
fn feed_service_lifecycle(ui_runtime: &crate::app::ui_runtime::UiRuntime) {
    /// Takes the runtime out of the services' lifecycle as the listener
    /// retires: with the runtime's scheduler, or when the loop ends.
    struct Fed {
        lifecycle: Rc<crate::app::lifecycle::ServiceLifecycle>,
        runtime: flui_foundation::UiRuntimeId,
    }

    impl Drop for Fed {
        fn drop(&mut self) {
            self.lifecycle.forget(self.runtime);
        }
    }

    let lifecycle = APP_RUNTIME.with(|slot| slot.borrow().service_lifecycle());
    let scheduler = ui_runtime.scheduler();
    lifecycle.note(ui_runtime.id(), scheduler.lifecycle_state());
    let fed = Fed {
        lifecycle: Rc::clone(&lifecycle),
        runtime: ui_runtime.id(),
    };
    let subscription = scheduler.listen_lifecycle_state(move |state| {
        fed.lifecycle.note(fed.runtime, state);
    });
    lifecycle.hold(ui_runtime.id(), subscription);
}

/// [`build_lending_ui_runtime`] for a test that installs no window: a loan it
/// opened is given back as it drops.
#[cfg(test)]
pub(super) fn build_ui_runtime(
    window: impl Into<crate::app::presentation::PresentationWindow>,
    scale_factor: f64,
) -> Result<crate::app::ui_runtime::UiRuntime, HostServicesError> {
    build_lending_ui_runtime(window, scale_factor, false).map(|(ui_runtime, _)| ui_runtime)
}

/// Answer one phase of the user's session ending (ADR-0237, ADR-0260), as
/// [`SessionEnd::answer`](flui_runtime::owner_turn::SessionEnd::answer) runs
/// it over this loop's state ([`RunnerSessionEnd`]). A UI runtime that is
/// checked out is not asked and counts as `Proceed`, since its published bytes
/// are written when the session ends.
fn answer_session_end(phase: flui_platform::SessionEndPhase) -> flui_platform::SessionEndAnswer {
    // A loop state that is busy (a session end reported from a nested loop
    // inside a frame) keeps no book: the refusals such a query takes are
    // released when it returns.
    let book = APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .map_or_default(|state| Rc::clone(&state.session_end))
    });
    book.answer(phase, &mut RunnerSessionEnd)
}

/// The window at `address` carried out the session-end close it refused: its
/// refusal no longer holds the session. Once nothing else holds it, the
/// platform is released.
pub(super) fn release_session_end_refusal(address: flui_foundation::PresentationAddress) {
    let book = APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .ok()
            .map(|state| Rc::clone(&state.session_end))
    });
    if let Some(book) = book {
        book.release_refusal(address);
    }
}

/// A runner's side of the session end's steps: this thread's loop state.
struct RunnerSessionEnd;

impl flui_runtime::owner_turn::SessionEndHost for RunnerSessionEnd {
    fn support(&self) -> Option<flui_platform::SessionEndSupport> {
        session_end_platform(flui_native::OwnerPlatform::session_end_support)
    }

    fn window_system(&self) -> flui_runtime::window_system::WindowSystem {
        super::window_system()
    }

    /// The operating system's: the platform ends the session on it.
    fn clock(&self) -> Rc<dyn flui_foundation::MonotonicClock> {
        Rc::new(flui_foundation::SystemClock)
    }

    fn router(&self) -> Option<Rc<flui_runtime::owner_turn::CloseRequestRouter>> {
        APP_RUNTIME.with(|slot| slot.try_borrow().ok().map(|state| state.close_requests()))
    }

    fn askable_presentations(&self) -> Vec<flui_foundation::PresentationAddress> {
        askable_presentations()
    }

    fn deliver_notice(
        &mut self,
        notice: &flui_platform::HostNotice,
        may_start: &dyn Fn() -> bool,
    ) -> flui_runtime::owner_turn::NoticeDelivery {
        super::owner_dispatch::deliver_host_notice_while(notice, may_start)
    }

    fn release_hook(&self) -> Box<dyn FnOnce()> {
        // The platform is told the application is done with the session end
        // it asked to wait for: every window that refused carried its close
        // out, and every subscriber let go.
        Box::new(|| {
            let _ = session_end_platform(flui_native::OwnerPlatform::release_session_end);
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn close_for_session_end(&mut self, may_start: &dyn Fn() -> bool) {
        super::owner_dispatch::deliver_session_end_close(may_start);
    }

    #[cfg(target_arch = "wasm32")]
    fn close_for_session_end(&mut self, _may_start: &dyn Fn() -> bool) {}

    #[cfg(not(target_arch = "wasm32"))]
    fn flush(
        &mut self,
        deadline: web_time::Instant,
    ) -> Option<flui_view::__runtime::WritesInFlight> {
        flush_for_session_end(deadline)
    }

    #[cfg(target_arch = "wasm32")]
    fn flush(
        &mut self,
        _deadline: web_time::Instant,
    ) -> Option<flui_view::__runtime::WritesInFlight> {
        None
    }

    /// The window placements come after the application's own work and the
    /// documents: they take what the budget has left, never more than their
    /// own small bound, and what is lost is reported by name.
    fn flush_host_state(&mut self, deadline: web_time::Instant) {
        #[cfg(all(
            not(target_os = "android"),
            not(target_os = "ios"),
            not(target_arch = "wasm32")
        ))]
        super::placement::flush_all_by(
            deadline.min(std::time::Instant::now() + super::placement::SESSION_END_PLACEMENT_BOUND),
        );
        #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
        let _ = deadline;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn is_runtime_busy(&self) -> bool {
        super::owner_dispatch::any_runtime_checked_out()
    }

    #[cfg(target_arch = "wasm32")]
    fn is_runtime_busy(&self) -> bool {
        true
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn end_application(&mut self, tear_down: bool) {
        super::owner_dispatch::end_session_teardown(tear_down);
        let _ = session_end_platform(flui_native::OwnerPlatform::quit);
    }

    #[cfg(target_arch = "wasm32")]
    fn end_application(&mut self, _tear_down: bool) {}

    #[cfg(not(target_arch = "wasm32"))]
    fn hold_background(
        &mut self,
        writes: flui_view::__runtime::WritesInFlight,
        until: web_time::Instant,
    ) -> flui_platform::SessionEndAnswer {
        hold_for_background_writes(writes, until)
    }

    #[cfg(target_arch = "wasm32")]
    fn hold_background(
        &mut self,
        _writes: flui_view::__runtime::WritesInFlight,
        _until: web_time::Instant,
    ) -> flui_platform::SessionEndAnswer {
        flui_platform::SessionEndAnswer::Proceed
    }

    fn drop_background_hold(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        drop(take_background_writes());
    }

    /// Logged, its subscriber contained ([`SessionEndStep::log`]), and
    /// dropped: the caller answers a native callback.
    ///
    /// [`SessionEndStep::log`]: flui_runtime::owner_turn::SessionEndStep::log
    fn contained(
        &mut self,
        step: flui_runtime::owner_turn::SessionEndStep,
        payload: Box<dyn std::any::Any + Send>,
    ) {
        step.log(payload);
    }
}

/// Hold a phone's process in the background for `writes`, the flush's writes
/// still in flight: a thread waits for them until `until`, reports by name
/// what is still not stored then, and asks the owner for the turn that
/// releases the platform ([`release_finished_background_writes`]). `Block`
/// while the hold is taken, `Proceed` when there is no owner to release it
/// or the thread cannot start.
#[cfg(not(target_arch = "wasm32"))]
fn hold_for_background_writes(
    writes: flui_view::__runtime::WritesInFlight,
    until: std::time::Instant,
) -> flui_platform::SessionEndAnswer {
    use std::sync::atomic::{AtomicBool, Ordering};

    let Some(proxy) = session_end_platform(flui_native::OwnerPlatform::proxy) else {
        return flui_platform::SessionEndAnswer::Proceed;
    };
    let done = Arc::new(AtomicBool::new(false));
    let finished = Arc::clone(&done);
    let spawned = std::thread::Builder::new()
        .name("flui-background-writes".into())
        .spawn(move || {
            let unstored = writes.wait_until(until);
            if unstored.is_empty() {
                tracing::debug!("the writes left in flight at the session end are stored");
            } else {
                tracing::warn!(
                    ?unstored,
                    "the background window ended before these values were stored"
                );
            }
            finished.store(true, Ordering::Release);
            // An owner that is gone has nothing left to release.
            let _ = proxy.wake();
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "no thread waits for the writes left in flight; the process is not held");
        return flui_platform::SessionEndAnswer::Proceed;
    }
    let displaced = APP_RUNTIME.with(|slot| {
        slot.try_borrow_mut()
            .map(|mut state| state.background_writes.replace(done))
    });
    match displaced {
        Ok(displaced) => {
            drop(displaced);
            flui_platform::SessionEndAnswer::Block
        }
        // The loop state is busy: nobody could release the hold.
        Err(_) => flui_platform::SessionEndAnswer::Proceed,
    }
}

/// Take the hold a phone's session end left for its writes, whatever its
/// state.
#[cfg(not(target_arch = "wasm32"))]
fn take_background_writes() -> Option<Arc<std::sync::atomic::AtomicBool>> {
    APP_RUNTIME.with(|slot| {
        slot.try_borrow_mut()
            .ok()
            .and_then(|mut state| state.background_writes.take())
    })
}

/// Release the platform from the background hold a phone's session end took
/// once its writes ended or their window passed
/// ([`hold_for_background_writes`]). Called on every owner turn; does
/// nothing while no hold is taken or its writes still run.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn release_finished_background_writes() {
    let finished = APP_RUNTIME.with(|slot| {
        let Ok(mut state) = slot.try_borrow_mut() else {
            return false;
        };
        let finished = state
            .background_writes
            .as_ref()
            .is_some_and(|done| done.load(std::sync::atomic::Ordering::Acquire));
        if finished {
            state.background_writes = None;
        }
        finished
    });
    if finished {
        let _ = session_end_platform(flui_native::OwnerPlatform::release_session_end);
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
fn askable_presentations() -> Vec<flui_foundation::PresentationAddress> {
    APP_RUNTIME.with(|slot| {
        let Ok(state) = slot.try_borrow() else {
            return Vec::new();
        };
        let logical = state.installed_host.logical();
        state
            .close_requests()
            .addresses()
            .into_iter()
            .filter(|address| {
                logical
                    .runtime_status(address.ui_runtime_id)
                    .is_ok_and(|status| !status.checked_out)
            })
            .collect()
    })
}

/// Write what the documents published, inline, by `deadline`: the process may
/// be ended at any moment once the session-end handler returns. Runs no
/// application code; the session end contains a panic in the writer. The
/// writes still in flight at `deadline`, when there are any, are handed back
/// for a phone's host to hold the process for.
#[cfg(not(target_arch = "wasm32"))]
fn flush_for_session_end(
    deadline: std::time::Instant,
) -> Option<flui_view::__runtime::WritesInFlight> {
    let flush = APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .ok()
            .and_then(|state| state.application.flush_host.clone())
    })?;
    let report = flush.flush_within(deadline);
    if report.is_complete() {
        return None;
    }
    tracing::warn!(
        failed = ?report.failed,
        unfinished = ?report.unfinished,
        "the session ended before the flush registry was written"
    );
    flush.writes_in_flight()
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
    /// carries both invariants. A window's handler, the window itself, the
    /// application handler (ADR-0254 §4, §5) and the owner turn the runner
    /// installs (`OwnerPlatform::on_wake`, ADR-0327 §5) are owner-local; the
    /// `!Send` UI runtime this holds sits in owner TLS, reached only through
    /// the stamped FIFO dispatcher below and the fenced `with_owner_platform`
    /// accessor.
    ///
    /// `AppRuntime::new()` is cheap and side-effect-free (no singleton
    /// resolution) precisely so merely *touching* this thread-local -- for
    /// any reason, including `OwnerHostClearGuard::drop` firing during an
    /// unwind on a thread that never reached platform init -- can never
    /// itself trigger singleton construction or full system-font
    /// enumeration. Real service resolution happens only when a UI runtime is
    /// built or installed: `build_lending_ui_runtime`, which every runner calls
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
/// Deliberately does NOT resolve `SharedEngineServices`: the owner platform
/// is installed before any `UiRuntime` exists (no widget tree, no
/// painting/semantics/scheduler singleton reach), so resolving here would
/// pay for singleton construction and full system-font enumeration before
/// anything consumes either. UI runtime construction and prepared
/// installation resolve these services; merely installing the platform
/// owner does not.
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
                let mut commands = flui_foundation::panic::FirstPanic::new();
                commands.contain(|| {
                    super::owner_dispatch::apply_queued_window_commands(
                        std::thread::current().id(),
                    );
                });
                // An expiring clipboard entry's clear is no window's work: it
                // is served whether or not a UI runtime is installed.
                commands.contain(serve_clipboard_expiries);
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
                        commands.resume();
                        return;
                    }
                    let mut first = flui_foundation::panic::FirstPanic::new();
                    first.contain(|| {
                        if let Err(error) =
                            refresh_preferences_with(flui_native::OwnerPlatform::preferences)
                        {
                            tracing::warn!(%error, "system preference refresh failed");
                        }
                    });
                    first.contain(|| {
                        if let Err(error) =
                            refresh_displays_with(flui_native::OwnerPlatform::display_snapshot)
                        {
                            tracing::warn!(%error, "display refresh failed");
                        }
                    });
                    #[cfg(not(target_arch = "wasm32"))]
                    if current() {
                        let recovery = if first.is_held() {
                            flui_runtime::owner::RecoveryState::PreservingFailure
                        } else {
                            flui_runtime::owner::RecoveryState::Healthy
                        };
                        first.contain(|| {
                            super::secondary_window::drain_pending_secondary_window_completions(
                                recovery,
                            );
                        });
                        if !first.is_held() && current() {
                            first.contain(super::main_window::drive_main_window);
                            first.contain(super::main_window::drive_program_quit);
                            // A restart a frame of this turn found due runs
                            // once the frames returned (ADR-0393 §4).
                            if !first.is_held() && current() {
                                first.contain(super::restart::restart_if_due);
                            }
                        }
                    }
                    // What an opened URL's loan returned is routed here,
                    // outside the destructor that returned it.
                    if current() {
                        route_owed_opened_urls();
                        // A phone's background hold is released once its
                        // writes ended; the headless host stands in for one.
                        #[cfg(not(target_arch = "wasm32"))]
                        release_finished_background_writes();
                    }
                    commands.absorb(first);
                    commands.resume();
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
    let primary_selection = owner.primary_selection();
    let data_transfer = owner.data_transfer();
    // The outgoing loop's raster host, whose GPU context retires below,
    // outside the composition-root borrow.
    let mut previous_raster_host = None;
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
                state.set_platform_primary_selection(primary_selection),
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
                state.pending_window_reservations =
                    Arc::new(std::sync::atomic::AtomicUsize::new(0));
            }
            // A loop that wants other renderers says so after this install.
            {
                previous_raster_host = Some(std::mem::take(&mut state.raster_host));
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
    let mut first = flui_foundation::panic::FirstPanic::new();
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    first.contain(install_wake_deadline_hook);
    first.contain(install_turn_deadline_route);
    first.contain(|| {
        if let Err(error) = refresh_preferences_with(flui_native::OwnerPlatform::preferences) {
            tracing::warn!(%error, "initial system preference observation failed");
        }
    });
    first.contain(|| {
        if let Err(error) = refresh_displays_with(flui_native::OwnerPlatform::display_snapshot) {
            tracing::warn!(%error, "initial display observation failed");
        }
    });
    first.contain(|| previous_host.shutdown());
    first.contain(|| drop(previous_host));
    first.contain(|| drop(previous_raster_host));
    first.contain(|| drop(previous_owner_turn_wake));
    first.contain(|| drop(previous_clipboard));
    first.contain(|| drop(previous));
    first.resume();
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
    /// Answers a quit the user or the system asked for.
    quit_asked: Option<Rc<dyn Fn() -> flui_native::QuitAnswer>>,
    /// Answers whether the main window's controller took the reopen.
    reopen: Option<Rc<dyn Fn() -> bool>>,
    exit_policy: Option<Rc<dyn Fn() -> bool>>,
    wake_deadline: Option<Rc<dyn Fn() -> Option<web_time::Instant>>>,
    /// The instant the owner needs a turn at for work that is no frame.
    turn_deadline: Option<Rc<dyn Fn() -> Option<web_time::Instant>>>,
    /// Where what the operating system opens goes; set when the handler is
    /// made, so it is there before `ready` and the first window.
    opened: Option<Rc<super::opened::OpenRouting>>,
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

/// The routing of what the operating system opens, of the innermost run on
/// this thread whose handler is alive.
pub(super) fn current_open_routing() -> Option<Rc<super::opened::OpenRouting>> {
    let routes = APP_RUNTIME.with(|slot| {
        let state = slot.try_borrow().ok()?;
        state
            .run_routes
            .iter()
            .rev()
            .find_map(std::rc::Weak::upgrade)
    })?;
    routes
        .try_borrow()
        .ok()
        .and_then(|routes| routes.opened.clone())
}

/// Send the platform's quit to `route`.
pub(crate) fn route_quit(route: impl Fn() + 'static) {
    set_route(|routes| &mut routes.quit, Rc::new(route) as Rc<dyn Fn()>);
}

/// Answer a quit the user or the system asked for (AppKit's Quit) with
/// `route`.
#[cfg_attr(
    any(target_arch = "wasm32", target_os = "android", target_os = "ios"),
    expect(
        dead_code,
        reason = "only the desktop main window asks its presentations"
    )
)]
pub(super) fn route_quit_asked(route: impl Fn() -> flui_native::QuitAnswer + 'static) {
    set_route(
        |routes| &mut routes.quit_asked,
        Rc::new(route) as Rc<dyn Fn() -> flui_native::QuitAnswer>,
    );
}

/// Send the user's reopen (AppKit's Dock click) to `route`, which answers
/// whether it took it: `false` once the application opens no window.
#[cfg_attr(
    any(
        target_arch = "wasm32",
        all(not(test), any(target_os = "android", target_os = "ios"))
    ),
    expect(dead_code, reason = "only the desktop main window reopens")
)]
pub(super) fn route_reopen(route: impl Fn() -> bool + 'static) {
    set_route(
        |routes| &mut routes.reopen,
        Rc::new(route) as Rc<dyn Fn() -> bool>,
    );
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

/// The application handler a test passes to `Platform::run`, with
/// `bootstrap` as its `ready`, for a run that declares nothing it opens
/// ([`opening_app_handler`] with the default settings).
#[cfg(test)]
pub(crate) fn app_handler(
    bootstrap: impl FnOnce(flui_native::OwnerPlatform) -> Result<(), flui_native::BootstrapError>
    + 'static,
) -> Rc<dyn flui_native::AppHandler> {
    opening_app_handler(super::opened::OpenSettings::default(), bootstrap)
}

/// The application handler of an application's run, which routes what the
/// operating system opens it with by `opening`.
pub(crate) fn opening_app_handler(
    opening: super::opened::OpenSettings,
    bootstrap: impl FnOnce(flui_native::OwnerPlatform) -> Result<(), flui_native::BootstrapError>
    + 'static,
) -> Rc<dyn flui_native::AppHandler> {
    let routes = AppRoutes {
        opened: Some(Rc::new(super::opened::OpenRouting::new(opening))),
        ..AppRoutes::default()
    };
    Rc::new(RunnerHandler {
        bootstrap: std::cell::RefCell::new(Some(Box::new(bootstrap))),
        routes: Rc::new(std::cell::RefCell::new(routes)),
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

/// Take the reopen route of the innermost run on this thread whose handler
/// is alive, as a Dock click does: the main window opens, and the URLs held
/// for the application open in its first Router
/// ([`OpenRouting::route_held`](super::opened::OpenRouting::route_held)). A
/// run with no main window (iOS, Android, the web) routes no reopen: there
/// the host opens the window they wait for. Asked again while the window is
/// coming, the route only keeps the intent it set.
pub(super) fn reopen_current_run() -> ReopenAnswer {
    let routes = APP_RUNTIME
        .try_with(|slot| {
            let state = slot.try_borrow().ok()?;
            state
                .run_routes
                .iter()
                .rev()
                .find_map(std::rc::Weak::upgrade)
        })
        .ok()
        .flatten();
    let reopen = routes.and_then(|routes| {
        routes
            .try_borrow()
            .ok()
            .and_then(|routes| routes.reopen.clone())
    });
    match reopen.map(|reopen| reopen()) {
        Some(true) => ReopenAnswer::Taken,
        Some(false) => ReopenAnswer::NotTaken,
        None => ReopenAnswer::NoRoute,
    }
}

/// What the run answered a URL's reopen of its main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReopenAnswer {
    /// The main window's controller took it: an opening starts, or waits for
    /// a pending quit to be abandoned.
    Taken,
    /// The controller did not take it: the application opens no window now
    /// (it quits, or its owner is gone).
    NotTaken,
    /// The run routes no reopen (iOS, Android, the web): its host opens the
    /// window the URL waits for.
    NoRoute,
}

/// The run's main window, the one window an opened URL for the application
/// goes to (ADR-0339 §2). On the desktop it is the window the main-window
/// controller opens (the `run` or `run_resident` factory's); on iOS, Android
/// and the web, the runner's one window (the first scene, the Activity's
/// window, the page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MainWindow {
    /// Installed and open at this address.
    Open(flui_foundation::PresentationAddress),
    /// Being opened: a URL that arrives now is bound to this opening,
    /// delivered when it commits and refused when it fails.
    Opening,
    /// Not open and not being opened: the desktop reopens it for a URL; the
    /// other runners wait for the window their host opens.
    Closed,
    /// Not known now: the loop's state is borrowed. A URL waits for a later
    /// turn.
    Busy,
}

/// The run's main window on this thread. With the loop's state gone (the
/// thread ends) there is no window: `Closed`.
pub(super) fn main_window() -> MainWindow {
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    {
        let ingress = APP_RUNTIME.try_with(|slot| {
            slot.try_borrow()
                .ok()
                .map(|state| state.application.main_ingress.clone())
        });
        match ingress {
            Ok(Some(Some(ingress))) => {
                return match ingress.open_address() {
                    Some(address) => MainWindow::Open(address),
                    None if ingress.is_opening() => MainWindow::Opening,
                    None => MainWindow::Closed,
                };
            }
            Ok(Some(None)) => {}
            Ok(None) => return MainWindow::Busy,
            Err(_) => return MainWindow::Closed,
        }
    }
    let host = APP_RUNTIME.try_with(|slot| {
        slot.try_borrow()
            .ok()
            .map(|state| state.installed_host.clone())
    });
    match host {
        Ok(Some(host)) => match host.native().first_address() {
            Some(address) => MainWindow::Open(address),
            None if host.has_pending_installs() => MainWindow::Opening,
            None => MainWindow::Closed,
        },
        Ok(None) => MainWindow::Busy,
        Err(_) => MainWindow::Closed,
    }
}

/// How many openings of the main window have failed in this run: a URL bound
/// to an opening is refused once this count grows past what it was when the
/// URL was bound. Only the desktop reopens a main window for a URL; on iOS,
/// Android and the web no URL is bound, and the count stays 0.
pub(super) fn main_window_failures() -> u64 {
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    {
        APP_RUNTIME
            .try_with(|slot| {
                slot.try_borrow()
                    .ok()?
                    .application
                    .main_ingress
                    .as_ref()
                    .map(|ingress| ingress.failed_openings())
            })
            .ok()
            .flatten()
            .unwrap_or(0)
    }
    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    {
        0
    }
}

/// Route what opened URLs' loans returned since the last turn
/// ([`OpenRouting::route_owed`](super::opened::OpenRouting::route_owed)), from
/// the owner's turn, outside every destructor and borrow. A panic in a URL's
/// navigation ends the run, as one in the handler's other methods does.
pub(super) fn route_owed_opened_urls() {
    if let Some(routing) = current_open_routing() {
        contained((), "opened URLs", || routing.route_owed());
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
            // What launched the application is taken before the bootstrap
            // opens its first window, so that window's first Router opens on
            // the link (Flutter's initial route) instead of navigating to it
            // after its initial page was built.
            let routing = self.route(|routes| routes.opened.clone());
            if let Some(routing) = &routing {
                routing.bind_owner(owner.proxy());
            }
            if let (Some(routing), Some(origin)) = (&routing, owner.page_origin()) {
                routing.set_page_origin(origin);
            }
            // Refuses what launched the application, in the log, when the
            // bootstrap fails or panics.
            let launching = match (&routing, owner.take_launch_opened()) {
                (Some(routing), Some(launch)) => Some(routing.hold_launch(launch)),
                _ => None,
            };
            bootstrap(owner)?;
            if let Some(launching) = launching {
                launching.started();
            }
            // A link that launched an application which opened no window (a
            // resident one) opens its main window.
            if let Some(routing) = routing {
                routing.route_held();
            }
            Ok(())
        })
    }

    fn quit_requested(&self, _: &flui_native::OwnerPlatform) {
        contained((), "application quit", || {
            if let Some(quit) = self.route(|routes| routes.quit.clone()) {
                quit();
            }
        });
    }

    /// The quit the user or the system asked for is the quit the application
    /// asks for itself ([`AppHandle::request_quit`](crate::app::AppHandle::request_quit)):
    /// every presentation is asked on the owner's next turn, and the
    /// application quits once none refuses. A run with no main window to ask
    /// through quits at once; a failure keeps it running.
    fn quit_asked(&self, _: &flui_native::OwnerPlatform) -> flui_native::QuitAnswer {
        contained(
            flui_native::QuitAnswer::KeepRunning,
            "application quit question",
            || {
                self.route(|routes| routes.quit_asked.clone())
                    .map_or(flui_native::QuitAnswer::Quit, |asked| asked())
            },
        )
    }

    fn reopen(&self, _: &flui_native::OwnerPlatform) {
        contained((), "application reopen", || {
            // The user's reopen asks for nothing back: a reopen the
            // controller does not take shows nothing, as on a quitting app.
            if let Some(reopen) = self.route(|routes| routes.reopen.clone()) {
                let _taken: bool = reopen();
            }
        });
    }

    /// Files to the application's callback, URLs to the presentation they
    /// address ([`OpenRouting::deliver`](super::opened::OpenRouting::deliver)).
    fn opened(&self, _: &flui_native::OwnerPlatform, opened: flui_platform::Opened) {
        contained((), "application open", || {
            if let Some(routing) = self.route(|routes| routes.opened.clone()) {
                routing.deliver(opened);
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
        // The clipboard clears before the application hears the lock, each
        // contained on its own: a host's panic in the clear costs no
        // subscriber the notice, and a subscriber's costs no clear.
        contained((), "host notice", || {
            let mut first = flui_foundation::panic::FirstPanic::new();
            first.contain(|| clear_clipboard_expiries_on_lock(notice));
            first.contain(|| super::owner_dispatch::deliver_host_notice(&notice.into()));
            first.resume();
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

    fn turn_deadline(&self, _: &flui_native::OwnerPlatform) -> Option<web_time::Instant> {
        contained(None, "turn deadline", || {
            self.route(|routes| routes.turn_deadline.clone())
                .and_then(|deadline| deadline())
        })
    }
}

/// Routes the platform's exit question to this thread's `AppRuntime`, instead
/// of letting a platform backend decide, alone, whether "every window this
/// backend tracks just closed" means "exit": the application's `ExitPolicy`
/// decides, through `AppRuntime::should_exit`. Call once,
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
            let ingress = APP_RUNTIME.with(|slot| slot.borrow().application.main_ingress.clone());
            if let Some(ingress) = ingress {
                return ingress.try_auto_quit();
            }
        }
        should_exit
    });
    let proxy = with_owner_platform(flui_native::OwnerPlatform::proxy);

    // The exit policy is otherwise only asked when a window closes, so a
    // veto owed to a running keep-alive service would be
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
    if let Some(proxy) = proxy {
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .set_lifecycle_exit_notifier(std::sync::Arc::new(move || {
                    reevaluate_exit_policy(&proxy);
                }));
        });
    }
    #[cfg(target_arch = "wasm32")]
    drop(proxy);
}

/// Ask the owner, from any thread, to ask the exit policy again. An owner that
/// stopped, or one with no policy to ask for other threads, has nothing left
/// to decide; a turn that could not be posted is logged.
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
        reason = "its callers are the desktop exit policy and windows; android and iOS ask none"
    )
)]
#[cfg_attr(
    all(test, target_os = "android"),
    expect(
        dead_code,
        reason = "desktop exit-policy tests are excluded on Android"
    )
)]
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn reevaluate_exit_policy(proxy: &flui_native::PlatformProxy) {
    match proxy.request_exit_policy_reevaluation() {
        Ok(())
        | Err(
            flui_native::ProxySendError::OwnerGone { .. }
            | flui_native::ProxySendError::Unsupported { .. },
        ) => {}
        Err(error) => {
            super::terminal::contain_diagnostic(
                || tracing::error!(%error, "an exit-policy request could not reach the owner"),
            );
        }
    }
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
/// (its idle wait) — never synchronously from this function — and only
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

/// Route the loop's turn deadline to this owner installation's
/// `AppRuntime::turn_deadline`, on every host: an expiring clipboard entry's
/// clear (ADR-0363), and on the desktop the owner turns window placements
/// need, which no frame serves and the main window waiting for its placement
/// read has none for. A later installation's route answers for itself.
fn install_turn_deadline_route() {
    let installed = APP_RUNTIME.with(|slot| slot.borrow().owner_install_generation);
    set_route(
        |routes| &mut routes.turn_deadline,
        Rc::new(move || {
            APP_RUNTIME.with(|slot| {
                let state = slot.borrow();
                (state.owner_install_generation == installed)
                    .then(|| state.turn_deadline())
                    .flatten()
            })
        }) as Rc<dyn Fn() -> Option<web_time::Instant>>,
    );
}

/// The owner's turn serves the expiring clipboard entries' clears due by now
/// (ADR-0363); the host's answers settle at a later turn.
pub(super) fn serve_clipboard_expiries() {
    let expiries = APP_RUNTIME.with(|slot| slot.borrow().clipboard_expiries());
    if let Some(expiries) = expiries {
        expiries.serve();
    }
}

/// The application is in front again: the expiring clipboard entries the
/// host could not see the clipboard for are asked again (ADR-0363).
pub(super) fn clipboard_expiries_in_front() {
    let expiries = APP_RUNTIME.with(|slot| slot.borrow().clipboard_expiries());
    if let Some(expiries) = expiries {
        expiries.in_front();
    }
}

/// The user's session locked, or the machine is going to sleep: the host is
/// asked to clear whatever expiring entry it still holds (ADR-0363).
fn clear_clipboard_expiries_on_lock(notice: flui_platform::SystemNotice) {
    if !matches!(
        notice,
        flui_platform::SystemNotice::SessionLocked | flui_platform::SystemNotice::Sleep
    ) {
        return;
    }
    let expiries = APP_RUNTIME.with(|slot| slot.borrow().clipboard_expiries());
    if let Some(expiries) = expiries {
        expiries.session_locked();
    }
}

/// The instant the running loop is asked to give the owner a turn at: the
/// answer of the turn-deadline route [`install_wake_deadline_hook`] set.
#[cfg(all(
    feature = "testing",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(crate) fn loop_turn_deadline() -> Option<web_time::Instant> {
    let routes = APP_RUNTIME.with(|slot| {
        let runs = &slot.borrow().run_routes;
        runs.last().and_then(std::rc::Weak::upgrade)
    })?;
    let route = routes.borrow().turn_deadline.clone()?;
    route()
}

/// The earliest armed source; an absent source contributes no deadline.
#[cfg_attr(
    target_arch = "wasm32",
    expect(
        dead_code,
        reason = "the browser's animation frames wake the page; no deadline does"
    )
)]
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
/// `WakeAction::PumpBackground` arm returns before the raster host's turn
/// (`RasterSlot::pump`) ever runs, so nothing on that path would consume a reported
/// deadline — reporting it anyway hands the idle wait the SAME past
/// instant on every idle iteration once it comes due, which is a
/// `WaitUntil(past)` busy-spin, forced by this hook instead of a stale UI
/// runtime deadline. The deadline is not lost by staying unreported while
/// disabled: presentation lifecycle reconciliation
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
///     `Frame` is forbidden; `Idle` and `PostFrame` are allowed (legitimate
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
            phase != Some(flui_scheduler::SchedulerPhase::Frame),
            "BUG: with_owner_platform called while the installed ui_runtime's scheduler is inside \
             the frame transaction (phase {phase:?}) -- owner_platform must \
             not be acquired from build/layout/paint (ADR-0039 §6)"
        );
    }
    let owner = APP_RUNTIME.with(|slot| slot.borrow().owner_platform.clone());
    owner.as_deref().map(f)
}

/// Unwind-safe TLS clearing. Arm this guard *before* calling
/// `Platform::run(...)` on any backend whose `run` returns (headless,
/// Android, AppKit) — not inside `on_ready` — so a panic anywhere inside
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
#[cfg_attr(
    all(target_arch = "wasm32", not(test)),
    expect(
        dead_code,
        reason = "the web host clears its owner when its own loop exits (see `web.rs`)"
    )
)]
pub(crate) struct OwnerHostClearGuard {
    expected_generation: u64,
}

impl OwnerHostClearGuard {
    /// Own the first successful owner installation after this point. A later
    /// replacement has another generation and cannot be erased by this guard.
    /// Call immediately before `Platform::run(...)`; no-install failure leaves
    /// a previously installed owner untouched.
    #[cfg_attr(
        all(target_arch = "wasm32", not(test)),
        expect(
            dead_code,
            reason = "the web host clears its owner when its own loop exits (see `web.rs`)"
        )
    )]
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
                    runtime.application.host_storage.take(),
                )
            } else {
                (None, None, None)
            }
        });
        // The loop's GPU context retires with the loop, before the platform
        // whose display it was created on, and never in the thread's
        // teardown, where the GPU driver's own thread-locals are gone.
        if removed.is_some() {
            let raster_host =
                APP_RUNTIME.with(|slot| std::mem::take(&mut slot.borrow_mut().raster_host));
            drop(raster_host);
        }
        drop(removed);
        drop(owner_turn_wake);
        drop(storage);
    }
}

#[cfg(test)]
#[path = "app_handler_tests.rs"]
mod app_handler_tests;
