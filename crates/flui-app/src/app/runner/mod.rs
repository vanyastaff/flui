//! Application runner - entry points for running FLUI apps.
//!
//! This module provides platform-agnostic entry points that delegate
//! to platform-specific implementations via flui-native.

use std::rc::Rc;

use flui_view::View;

use super::LaunchError;

#[cfg(target_os = "android")]
mod android;
#[cfg(any(windows, test))]
mod associations;
#[cfg(all(
    test,
    feature = "testing",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod clipboard_expiry_tests;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod desktop;
mod dev_agents;
mod display_host;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod first_reveal;
mod fonts;
mod frame_driver;
mod frame_pacing;
#[cfg(any(test, target_arch = "wasm32"))]
mod gpu_loss;
mod host;
mod installed_host;
#[cfg(target_os = "ios")]
pub(super) mod ios;
mod journal_start;
mod native_bindings;
#[cfg(native_journal)]
pub(super) mod native_journal;
mod native_retirement;
mod native_text_sizing;
mod opened;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(in crate::app) mod placement;
mod window_install;
pub(in crate::app) mod window_routes;

mod owner_dispatch;
pub(super) mod raster_host;
mod secondary_window;
#[cfg(any(
    target_os = "ios",
    all(test, not(target_os = "android"), not(target_arch = "wasm32"))
))]
mod session_controller;
#[cfg(all(
    feature = "testing",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod stand_in_renderer;
mod terminal;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_os = "android")]
use android::run_android;
#[cfg(windows)]
pub(in crate::app) use associations::PendingRegistration;
pub use fonts::{FontRegistrationError, register_font};
#[cfg(test)]
pub(crate) use host::app_handler;
pub(crate) use host::{AppRoutes, opening_app_handler};
pub(in crate::app) use installed_host::InstalledHost;
#[cfg(target_os = "ios")]
use ios::run_ios;
pub(crate) use opened::OpenSettings;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[cfg(feature = "unstable-windows")]
pub use secondary_window::open_secondary_window;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[cfg(feature = "unstable-windows")]
pub use secondary_window::open_window;
#[cfg(target_arch = "wasm32")]
use web::run_web;

/// The window system this runner was built for: the browser's page on
/// `wasm32`, the platform's window system elsewhere. The one place a runner
/// takes it from the compile target; its built-in capabilities, its close
/// vetoes, its session end and its root platform convention all read it.
#[cfg(target_arch = "wasm32")]
pub(crate) fn window_system() -> flui_runtime::window_system::WindowSystem {
    flui_runtime::window_system::WindowSystem::Browser
}

/// The window system this runner was built for: its platform's. The one
/// place a runner takes it from the compile target.
#[cfg(not(target_arch = "wasm32"))]
#[expect(
    clippy::disallowed_methods,
    reason = "the runner's windows are those of the platform it was built for, and it is the \
              root default of PlatformConvention"
)]
pub(crate) fn window_system() -> flui_runtime::window_system::WindowSystem {
    flui_runtime::window_system::WindowSystem::Platform(flui_platform::TargetPlatform::current())
}

/// The presentation window for a freshly opened host window: the window
/// itself, upcast to the contract the UI runtime drives, the accessibility
/// bridge its backend fixed when it built it, and its text-store host when
/// the backend's input methods pull from the field (ADR-0135).
///
/// The one place the runner turns an `open_window` result into what a UI runtime
/// constructor takes. Reading the bridge and the host here, once, is sound
/// because every backend sets both at construction and never swaps them.
/// The host is owner-thread state, so it is read through the loop's
/// [`OwnerPlatform`](flui_native::OwnerPlatform); without one installed
/// (no loop on this thread) the window has none.
pub(crate) fn presentation_window(
    host: Rc<dyn flui_native::HostWindow>,
) -> crate::app::presentation::PresentationWindow {
    #[cfg(feature = "native")]
    keep_native_window(&host);
    let accessibility = host.accessibility();
    let text_store_host = text_store_host_of(&host);
    let occluded_surface = host.occluded_surface();
    // The runner names the platform it was built for: the tree follows its
    // conventions, and host observations replace the gesture values the OS
    // publishes (ADR-0212 §4). A browser's page follows the platform-neutral
    // ones.
    let platform = window_system()
        .platform()
        .unwrap_or(flui_platform::TargetPlatform::Unknown);
    crate::app::presentation::PresentationWindow::new(host, accessibility)
        .with_text_store_host(text_store_host)
        .with_occluded_surface(occluded_surface)
        .with_platform(platform)
        .with_gesture_profiles(flui_interaction::gesture::GestureProfiles::preset(platform))
}

/// The window a deferred open built, taken on the owner thread, which is the
/// only one that reaches it (ADR-0254 §4): one that closed before it could be
/// installed is a failure of its open.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(super) fn opened_window(
    opened: flui_native::OpenedWindow,
) -> Result<Rc<dyn flui_native::HostWindow>, flui_native::OpenWindowError> {
    match opened.window() {
        flui_native::OpenedWindowReach::Open(window) => Ok(window),
        flui_native::OpenedWindowReach::Closed => Err(flui_native::OpenWindowError::Backend {
            message: "the window closed before it was installed".to_owned(),
        }),
        flui_native::OpenedWindowReach::OtherThread => {
            Err(flui_native::OpenWindowError::Unavailable {
                message: "a deferred window is installed on the thread that owns it".to_owned(),
            })
        }
    }
}

/// Close a window the runner is discarding: one whose installation failed or
/// was cancelled, or whose open nobody waits for any more. No caller is left
/// to hear a refusal (Android's one window does not close) or a failure, and
/// the presentation behind it is already torn down, so it is logged.
pub(super) fn discard_window(window: &dyn flui_platform::PlatformWindow, why: &'static str) {
    if let Err(error) = flui_platform::WindowCommand::Close.admit_and_apply(window) {
        tracing::warn!(%error, why, "a discarded window did not close");
    }
}

/// Close a window whose close request, held until its presentation could be
/// asked, the presentation granted: the window system's request was answered
/// already, so the close is the application's command.
#[cfg_attr(
    not(all(
        not(target_os = "android"),
        not(target_arch = "wasm32"),
        any(test, not(target_os = "ios"))
    )),
    expect(
        dead_code,
        reason = "only the desktop window installation routes a close request"
    )
)]
pub(crate) fn close_granted_request(window: &dyn flui_platform::PlatformWindow) {
    if let Err(error) = flui_platform::WindowCommand::Close.admit_and_apply(window) {
        tracing::warn!(%error, "a granted close request did not close its window");
    }
}

/// Keep a weak reference to a window the runner is about to present, so the
/// escape hatch finds the host's window behind a presentation's command queue.
/// Windows already dropped leave the list here.
#[cfg(feature = "native")]
fn keep_native_window(host: &Rc<dyn flui_native::HostWindow>) {
    host::APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        let mut windows = state.native_windows.borrow_mut();
        windows.retain(|window| window.strong_count() > 0);
        windows.push(Rc::downgrade(host));
    });
}

/// The host's window presented at `address`, on the thread that runs the
/// application: `None` on any other thread, when nothing is presented at
/// `address` and once its window has closed.
#[cfg(feature = "native")]
pub(crate) fn native_window(
    address: flui_foundation::PresentationAddress,
) -> Option<Rc<dyn flui_native::HostWindow>> {
    presented_host_window(|commands| commands.window_for(address))
}

/// The host's window `id`, while a presentation is drawn in it, on the thread
/// that runs the application: `None` on any other thread, when no
/// presentation is drawn in a window of that id and once it has closed.
#[cfg(feature = "native")]
pub(crate) fn native_window_by_id(
    id: flui_platform::WindowId,
) -> Option<Rc<dyn flui_native::HostWindow>> {
    presented_host_window(|commands| commands.window_with_id(id))
}

/// The host's window behind the presented window `find` picks from the
/// runtime's command queues, on the thread that runs the application. The
/// host's window is the allocation the presentation's command queue holds,
/// seen as the host hands it out.
#[cfg(feature = "native")]
fn presented_host_window(
    find: impl FnOnce(
        &super::window_control::WindowCommandQueues,
    ) -> Option<Rc<dyn flui_platform::PlatformWindow>>,
) -> Option<Rc<dyn flui_native::HostWindow>> {
    host::APP_RUNTIME.with(|slot| {
        let state = slot.try_borrow().ok()?;
        if !state.hosts_loop {
            return None;
        }
        let presented = find(&state.window_commands)?;
        let presented = Rc::as_ptr(&presented).cast::<()>();
        state.native_windows.borrow().iter().find_map(|window| {
            (std::rc::Weak::as_ptr(window).cast::<()>() == presented)
                .then(|| window.upgrade())
                .flatten()
        })
    })
}

fn text_store_host_of(
    window: &Rc<dyn flui_native::HostWindow>,
) -> Option<std::rc::Rc<dyn flui_platform::text::store::TextStoreHost>> {
    host::with_owner_platform(|owner| owner.text_store_host(window)).flatten()
}

/// The control handle of the window presented at `address`, for the thread
/// that runs the application: the address a [`WindowRequest`](crate::WindowRequest)
/// delivers names the new window, and this turns it into the handle its
/// widgets get from `LifecycleContext::window`.
///
/// `None` on any other thread, when no presentation is registered at
/// `address`, and once its window has closed.
#[must_use]
pub fn window_handle(
    address: flui_foundation::PresentationAddress,
) -> Option<flui_view::WindowHandle> {
    host::APP_RUNTIME.with(|slot| {
        let state = slot.try_borrow().ok()?;
        state
            .hosts_loop
            .then(|| state.window_commands.handle_for(address))
            .flatten()
    })
}

/// Close the window at `address` programmatically, bypassing its
/// close-request handler.
///
/// This is how an application finishes a close it kept open: a
/// [`CloseRequestHandler`](crate::CloseRequestHandler) that answered
/// [`CloseResponse::KeepOpen`](crate::CloseResponse) in order to save
/// unsaved work calls this with the address it received in the
/// [`CloseRequest`](crate::CloseRequest) once the work is done, and the
/// window closes on the normal path — `on_close`, presentation teardown,
/// and then the exit-policy question if this was the last window.
///
/// The handler is deliberately not asked again: every backend's own
/// `WindowCommand::Close` bypasses its native close-request path for the
/// same reason (AppKit's `-close` does not send `windowShouldClose:`;
/// Win32's `DestroyWindow` does not send `WM_CLOSE`).
///
/// Owner-thread only, like every other operation on a hosted UI runtime — a
/// call from a thread that runs no FLUI loop is REFUSED with
/// [`NoHostedRuntime`](PresentationCloseError::NoHostedRuntime) rather
/// than silently doing nothing. That matters for the deferral case above:
/// the work an application finishes before calling this often finishes on a
/// worker, and the runtime that hosts the presentations is the loop
/// thread's own, so a worker finds none. Marshal back to the thread that
/// ran the handler.
///
/// The answer is thread-scoped rather than process-scoped because "the
/// owner thread" is not a process-wide fact: a process may host several
/// event loops on several threads, each with its own `AppRuntime`. Merely
/// reading this thread's slot is side-effect-free by construction (see
/// `AppRuntime::new`'s own doc) — it resolves no services and constructs
/// no singletons.
///
/// # Backend behaviour
///
/// This resolves the close fully on the **headless**, **Win32**, **AppKit**,
/// **X11** and **Wayland** backends, whose `WindowCommand::Close` performs a
/// real close; the **headless** double runs `on_close` synchronously
/// inside the command. On **web** and **Android** the veto itself is inert
/// (neither backend consults `dispatch_should_close`), so there is nothing
/// to resolve; an Android window refuses the close
/// ([`CloseRequestError::Refused`](crate::CloseRequestError::Refused)).
///
/// # Errors
///
/// [`PresentationCloseError::NoHostedRuntime`] when the calling thread hosts
/// no FLUI runtime, and otherwise [`PresentationCloseError::Close`] with the
/// window's refusal:
/// [`CloseRequestError::UnknownPresentation`](crate::CloseRequestError::UnknownPresentation)
/// when no presentation is registered at `address` (never installed, or
/// already closed),
/// [`CloseRequestError::WindowGone`](crate::CloseRequestError::WindowGone)
/// when its native window is already destroyed,
/// [`CloseRequestError::Refused`](crate::CloseRequestError::Refused) when
/// its window system closes no window, and
/// [`CloseRequestError::Failed`](crate::CloseRequestError::Failed) when the
/// window took the close and failed it.
#[cfg(not(target_os = "ios"))]
pub fn request_presentation_close(
    address: flui_foundation::PresentationAddress,
) -> Result<(), PresentationCloseError> {
    // Read whether this thread hosts a loop and its router in ONE visit,
    // then leave the thread-local: `WindowCommand::Close` fires the window's
    // own `on_close`, which re-enters `APP_RUNTIME` through
    // `close_this_window`. The slot is this thread's own, so a loop it
    // hosts is the loop of this thread: a thread that hosts none, not an
    // empty router, is what tells "no FLUI loop here" from "that
    // presentation is gone".
    let (hosts_loop, router) = host::APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        (state.hosts_loop, state.close_requests())
    });
    if !hosts_loop {
        return Err(PresentationCloseError::NoHostedRuntime);
    }
    Ok(router.request_close(address)?)
}

#[cfg(not(target_os = "ios"))]
/// Why [`request_presentation_close`] closed no window.
#[derive(Debug, thiserror::Error, PartialEq)]
#[non_exhaustive]
pub enum PresentationCloseError {
    /// The calling thread hosts no FLUI runtime at all, so it has no
    /// presentations to close.
    ///
    /// The common cause is calling from a worker thread after finishing
    /// the work a [`CloseResponse::KeepOpen`](crate::CloseResponse::KeepOpen)
    /// answer deferred. Marshal back to the thread that ran the handler and
    /// call from there.
    ///
    /// Reported instead of
    /// [`CloseRequestError::UnknownPresentation`](crate::CloseRequestError::UnknownPresentation)
    /// because the two are genuinely different failures and a caller can act
    /// on the difference. It is thread-scoped rather than process-scoped on
    /// purpose: a process may host several event loops on several threads
    /// (`AppRuntime` is thread-local by design), so "the owner thread" is not
    /// a process-wide fact this call could consult.
    #[error(
        "this thread hosts no FLUI runtime; request the close from the thread that runs the \
         presentation's event loop"
    )]
    NoHostedRuntime,
    /// This thread's loop asked the window, which refused the close.
    #[error(transparent)]
    Close(#[from] crate::app::CloseRequestError),
}

/// Run `root` in a main window with the default configuration and the
/// built-in providers alone: `Application::new().run(root)`, as Flutter's
/// `runApp` takes the root widget. The root is any view, stateless or
/// stateful.
///
/// The same call starts the application on every platform: on the desktop,
/// iOS and the web from `fn main`, on Android from the function marked
/// `#[flui::main]`, whose `android_main` hands the run the system's
/// `AndroidApp`.
///
/// # Errors
///
/// As [`Application::run`](crate::Application::run).
///
/// # Panics
///
/// As [`Application::run`](crate::Application::run).
pub fn run_app<V>(root: V) -> Result<(), LaunchError>
where
    V: View + Clone + 'static,
{
    crate::app::Application::new().run(root)
}

/// The one check of an application's window options, before any runner
/// starts, on every platform: the numbers a window command would refuse (a
/// size or limit that encloses no area, a minimum above the maximum) refuse
/// the start, naming the option. A configured position is whole pixels and is
/// always a place, so none is refused here. A host checks the options of
/// every window it opens later itself (`WindowOptions::geometry`).
pub(in crate::app) fn check_window_options(
    config: &crate::app::AppConfig,
) -> Result<(), LaunchError> {
    config
        .window_options(None)
        .geometry()
        .map(drop)
        .map_err(LaunchError::InvalidWindowOption)
}

/// [`Application::run`](crate::Application::run) on this platform.
pub(in crate::app) fn run<V>(
    application: crate::app::Application,
    root: V,
) -> Result<(), LaunchError>
where
    V: View + Clone + 'static,
{
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    {
        let run = application.into_run(
            move |_: &crate::app::AppHandle| root.clone(),
            crate::app::application::StartupWindow::Open,
        );
        run_application(run).inspect_err(|error| {
            if let LaunchError::UnsupportedSystem(unsupported) = error {
                tracing::error!(%unsupported, "FLUI cannot run on this system");
            }
        })
    }

    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    {
        let crate::app::Application {
            config,
            capabilities,
            opening,
        } = application;
        // Logging first: `android_main` has no stderr, so the log is where a
        // conflict can be read. Managed startup installs FLUI's default
        // backend only if the slot is empty, so an application that
        // configured its own subscriber first keeps it.
        let _installation = super::logging::init_managed_logging(&config);
        let opening = opening.or_entry_declarations();
        // Before anything starts, as on the desktop: window options no window
        // can be asked for start nothing, though these hosts size their
        // window themselves.
        check_window_options(&config)?;
        // Android's `AndroidApp` before anything is installed, so a run with
        // no entry leaves nothing behind.
        #[cfg(target_os = "android")]
        let android_app = take_android_entry().ok_or(LaunchError::NoAndroidEntry)?;
        // Before anything starts, so a conflict starts nothing (ADR-0152 §1).
        install_capabilities(capabilities)?;
        tracing::info!(
            size = ?config.size,
            "Starting FLUI application"
        );
        #[cfg(target_os = "android")]
        let started = run_android(root, config, opening, android_app);
        #[cfg(target_os = "ios")]
        let started = run_ios(root, config, opening);
        #[cfg(target_arch = "wasm32")]
        let started = run_web(root, config, opening);
        started.inspect_err(|_| discard_capabilities())
    }
}

/// Record what the entry function `#[flui::main]` marks declares it opens,
/// for the runs it starts on this thread, and hand back what was recorded
/// before; refused while this thread's loop state is borrowed, which only an
/// entry called from inside a run's own callback finds.
pub(in crate::app) fn swap_entry_declarations(
    declarations: Option<flui_view::navigation::OpenDeclarations>,
) -> Result<Option<flui_view::navigation::OpenDeclarations>, std::cell::BorrowMutError> {
    host::APP_RUNTIME.with(|slot| {
        let mut state = slot.try_borrow_mut()?;
        Ok(std::mem::replace(
            &mut state.entry_declarations,
            declarations,
        ))
    })
}

/// Lend this thread's development hand-over (ADR-0393 §4 step 2) to `lend`;
/// refused while this thread's loop state is borrowed.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(in crate::app) fn with_hand_over<R>(
    lend: impl FnOnce(&mut crate::app::hot_reload::EntrySlot) -> R,
) -> Result<R, std::cell::BorrowMutError> {
    host::APP_RUNTIME.with(|slot| Ok(lend(&mut slot.try_borrow_mut()?.hand_over)))
}
/// What [`swap_entry_declarations`] recorded, while its entry runs.
pub(in crate::app) fn entry_declarations() -> Option<flui_view::navigation::OpenDeclarations> {
    host::APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .ok()
            .and_then(|state| state.entry_declarations)
    })
}

/// Keep `app` for the run the `#[flui::main]` entry starts on this thread.
#[cfg(target_os = "android")]
pub(in crate::app) fn hand_android_entry(app: android_activity::AndroidApp) {
    let displaced = host::APP_RUNTIME.with(|slot| slot.borrow_mut().android_entry.replace(app));
    drop(displaced);
}

/// The `AndroidApp` [`hand_android_entry`] kept, once.
#[cfg(target_os = "android")]
pub(in crate::app) fn take_android_entry() -> Option<android_activity::AndroidApp> {
    host::APP_RUNTIME.with(|slot| slot.borrow_mut().android_entry.take())
}

/// Check `capabilities` against the built-ins and install the registry the
/// run's UI runtimes acquire capabilities from.
///
/// # Errors
///
/// [`LaunchError::CapabilityConflict`] for the first conflict; nothing is
/// installed then.
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
pub(in crate::app) fn install_capabilities(
    capabilities: crate::app::capability::AppCapabilities,
) -> Result<(), LaunchError> {
    // A plugin's `register` is user code: it runs with no borrow held.
    let slots = host::APP_RUNTIME.with(|slot| slot.borrow().capability_slots());
    let registry = capabilities
        .into_registry(&slots)
        .map_err(LaunchError::CapabilityConflict)?;
    // A displaced registry's providers are user code too: they drop after the
    // borrow is released.
    let displaced = host::APP_RUNTIME.with(|slot| slot.borrow_mut().capabilities.replace(registry));
    drop(displaced);
    Ok(())
}

/// Take back the registry [`install_capabilities`] installed, when the
/// platform failed to start and no teardown will run.
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
fn discard_capabilities() {
    let installed = host::APP_RUNTIME.with(|slot| slot.borrow_mut().capabilities.take());
    drop(installed);
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(in crate::app) mod main_window;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(in crate::app) use main_window::run_application;
#[cfg(all(
    feature = "testing",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(crate) use main_window::{StartedLoop, start_loop};
pub use owner_dispatch::EXIT_CODE_HUNG;
#[cfg(all(
    feature = "testing",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use owner_dispatch::TeardownStep;
#[cfg(all(
    feature = "testing",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use stand_in_renderer::StandInRenderer;
#[cfg(all(
    feature = "testing",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(crate) use stand_in_renderer::StandInRenderers;
#[cfg(all(
    feature = "testing",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(crate) use {host::loop_turn_deadline, host::loop_wake_deadline, raster_host::RasterFactory};

/// Whether this loop journals its backend, so the agent's hello lists the `native` layer.
fn journals_native() -> bool {
    #[cfg(native_journal)]
    {
        native_journal::is_open()
    }
    #[cfg(not(native_journal))]
    {
        false
    }
}

/// End the loop's native journal unless the driver agent is `served`.
fn keep_native_journal(served: bool) {
    #[cfg(native_journal)]
    native_journal::keep_if(served);
    #[cfg(not(native_journal))]
    let _ = served;
}

#[cfg(test)]
mod tests {

    use super::host::{
        APP_RUNTIME, OwnerHostClearGuard, install_owner_platform, with_owner_platform,
    };
    use super::window_routes::{OpenRouted as _, OwnerOpenRouted as _};

    // `teardown_platform_ui_runtime` is `cfg(all(not(ios), not(wasm32)))` -- neither
    // platform runs the desktop teardown path it exercises.

    use super::*;

    /// Bootstrap ordering invariant shared by `bootstrap_desktop`, `run_android`,
    /// and `run_web`: the window must be stored in `AppRuntime`'s redraw-poke
    /// slot before anything that could synchronously observe it (the initial
    /// redraw request, `Lifecycle::Started`) runs — otherwise the first such
    /// observer would silently see nothing installed.
    ///
    /// `bootstrap_desktop`/`run_android`/`run_web` themselves cannot run in a
    /// unit test: each opens its window from inside a live platform event loop
    /// (`ActiveEventLoop` is unreachable outside `Platform::run`) and creates a
    /// real GPU context, gated behind the separate `testing` CI job
    /// (WARP), not this one. This instead drives the exact ordering invariant
    /// headlessly: `HeadlessWindow::request_redraw` (flui-native's headless
    /// backend, used elsewhere in this crate's tests) dispatches its
    /// `WindowHandler::frame_requested` callback SYNCHRONOUSLY — unlike a real
    /// native window, where the frame request arrives from the event loop and
    /// so cannot fire until `on_ready` (and this reordering) has already
    /// returned. That is exactly why the ordering bug was invisible in a real
    /// window's actual first frame but is directly observable here.
    ///
    /// Checks a unique window *size* rather than mere `is_some()`, so this
    /// cannot pass merely because an earlier test left SOME window installed
    /// — only THIS test's window, with THIS test's unmistakable marker size,
    /// proves `set_redraw_window` ran before the callback.
    ///
    /// If reverted: swap the order of the two calls below (request the
    /// redraw, then store the window — the pre-fix shape) and this fails:
    /// `wake_frame` finds no window yet, never calls `request_redraw` on it,
    /// and the callback never fires at all.
    ///
    /// No test lock: this touches `APP_RUNTIME`, a `thread_local!`, and the
    /// standard library test harness runs each `#[test]` on its own freshly
    /// spawned thread, so a fresh `AppRuntime` (no UI runtime, no owner platform)
    /// is what this test's thread starts from regardless of what any other
    /// concurrently-running test does on ITS OWN thread — the same reasoning
    /// this file's other thread-local-only tests below rely on. The retired
    /// `AppBinding`-era version of this test carried a dedicated per-test
    /// window-identity lock, and later, briefly, `UpdateScheduler` carried a
    /// sibling per-test scheduler-phase lock; both are deleted now, not
    /// ported forward, because the state each one guarded
    /// (`AppBinding::instance()`'s active window, and the process-global
    /// half of the `UpdateScheduler` singleton respectively) no longer exists —
    /// `AppBinding` is gone entirely and every `UiRuntime` owns its own fresh
    /// `UpdateScheduler` value — and because a per-test-thread thread-local needs
    /// no cross-test lock in the first place.
    fn desktop_bootstrap_stores_the_window_before_the_first_synchronous_redraw_observes_it() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let marker_size = flui_platform::WindowExtent::try_new(
            flui_foundation::geometry::Size::new(4001.0, 4002.0),
        )
        .expect("a finite extent");

        let platform = flui_native::headless_platform();
        let window = platform
            .open_routed(flui_platform::WindowOptions::default().size(marker_size))
            .expect("headless platform always opens a window");

        // `WindowHandler::frame_requested` requires `Send` on the callback; `AppRuntime` is
        // not `Send` (it holds owner-thread-affine ui_runtime state), so the
        // closure below cannot capture a specific `&AppRuntime`. Resolving
        // `APP_RUNTIME` fresh inside the closure (zero captures for the
        // runtime itself) sidesteps that entirely.
        //
        // Reads through `with_redraw_window`, NOT `wake_frame`/`request_redraw`:
        // a headless window's `request_redraw` dispatches this very callback
        // synchronously, so calling anything that re-locks the redraw-poke
        // slot from in here (the two are on the same thread, same call
        // stack) would deadlock on the slot's own non-reentrant lock.
        let saw_marker_window = Arc::new(AtomicBool::new(false));
        let saw_marker_window_cb = Arc::clone(&saw_marker_window);
        crate::app::runner::window_routes::WindowRoutes::of(&*window).route_frame_requested(
            move |_| {
                let matches_marker = APP_RUNTIME
                    .with(|slot| {
                        slot.borrow()
                            .with_redraw_window(|w| w.state().outer.size() == marker_size)
                    })
                    .unwrap_or(false);
                saw_marker_window_cb.store(matches_marker, Ordering::SeqCst);
                flui_platform::FrameOutcome::Produced
            },
        );

        // Mirrors the FIXED order in `bootstrap_desktop`/`run_android`:
        // store the window BEFORE requesting the initial redraw. `wake_frame`
        // (not a direct `request_redraw()` on the window) clones the window
        // out from under the lock before calling through, so this call
        // cannot deadlock against the callback's own `with_redraw_window`
        // re-entry above.
        APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            state.set_redraw_window(window);
            state.wake_frame();
        });

        assert!(
            saw_marker_window.load(Ordering::SeqCst),
            "set_redraw_window must have taken effect before the initial redraw \
             fires the frame callback that could read the redraw-poke slot",
        );
        // Clean up so this test's window does not linger for whatever test
        // runs next on this pool thread.
        let released = APP_RUNTIME.with(|slot| slot.borrow().clear_redraw_window());
        drop(released);
    }

    // ========================================================================
    // Owner-platform host tests (ADR-0039 §6)
    // ========================================================================

    fn owner_platform_host_panic_in_on_ready_still_clears() {
        use flui_native::headless_platform;

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _clear_guard = OwnerHostClearGuard::arm();
            let platform = headless_platform();
            let _ = platform.run(crate::app::runner::app_handler(|owner| {
                install_owner_platform(owner).expect("install owner wake transport");
                panic!("exercise on_ready panic cleanup");
            }));
        }));

        assert!(
            unwind.is_ok(),
            "on_ready's panic left the platform handler instead of being held"
        );
        let held =
            super::terminal::take_terminal().expect("on_ready's panic is the terminal failure");
        assert_eq!(
            flui_foundation::panic::payload_text(held.as_ref()),
            Some("exercise on_ready panic cleanup")
        );
        assert!(
            with_owner_platform(|_| ()).is_none(),
            "a panic inside on_ready must still leave the run through the clear guard \
             (armed before Platform::run, not inside on_ready) rather than \
             leaking the host onto this thread"
        );
    }

    /// A host that records what it was told to focus, from wherever the
    /// headless platform's factory built it.
    struct FocusLog(std::sync::Arc<parking_lot::Mutex<Vec<bool>>>);

    impl flui_platform::text::store::TextStoreHost for FocusLog {
        fn focus_store(
            &self,
            store: Option<std::rc::Rc<dyn flui_platform::text::store::TextStore>>,
        ) {
            self.0.lock().push(store.is_some());
        }

        fn complete_composition(
            &self,
            _: &std::rc::Rc<dyn flui_platform::text::store::TextStore>,
        ) -> Result<
            flui_platform::text::store::CompositionEnd,
            flui_platform::text::store::TextStoreHostError,
        > {
            Ok(flui_platform::text::store::CompositionEnd::Committed)
        }

        fn show_keyboard(&self, _: &std::rc::Rc<dyn flui_platform::text::store::TextStore>) {}
    }

    /// A pull-model window's host reaches the presentation the runner builds
    /// for it: a field that attaches is focused on the host
    /// (`presentation_window`, ADR-0135).
    ///
    /// Red-check: pass `None` to `with_text_store_host` in
    /// `presentation_window` — the presentation falls back to the window's
    /// push capability and the host hears nothing.
    fn presentation_window_hands_a_pull_window_s_host_to_its_presentation() {
        use std::sync::Arc;

        use flui_native::Platform as _;

        let heard = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let log = Arc::clone(&heard);
        let platform = flui_native::testing::HeadlessPlatform::new()
            .with_text_store_host(move || std::rc::Rc::new(FocusLog(Arc::clone(&log))));
        let _clear_guard = OwnerHostClearGuard::arm();
        Box::new(platform)
            .run(crate::app::runner::app_handler(|owner| {
                install_owner_platform(owner).expect("install owner wake transport");
                let window = with_owner_platform(|owner| {
                    owner.open_routed(flui_platform::WindowOptions::default())
                })
                .expect("BUG: install_owner_platform just ran above")
                .and_then(flui_native::WindowOpen::try_ready)
                .expect("headless open_window is always Ready");
                let ui_runtime =
                    host::build_ui_runtime(presentation_window(window), 1.0).expect("ui_runtime");
                let store = flui_interaction::testing::InMemoryTextStore::new("");
                let _token = ui_runtime
                    .text_input_handle()
                    .attach(flui_interaction::text_input::TextInputClient::new(store))
                    .expect("the presentation takes text input");
                Ok(())
            }))
            .expect("headless run");
        assert_eq!(
            *heard.lock(),
            [true, false],
            "focused, then unfocused at teardown"
        );
    }

    #[test]
    fn runner_bootstrap_matrix() {
        crate::table_test::run_table(
            "runner_bootstrap_matrix",
            &[
                ("desktop_bootstrap_stores_the_window_before_the_first_synchronous_redraw_observes_it", desktop_bootstrap_stores_the_window_before_the_first_synchronous_redraw_observes_it as fn()),
                ("owner_platform_host_panic_in_on_ready_still_clears", owner_platform_host_panic_in_on_ready_still_clears as fn()),
                ("presentation_window_hands_a_pull_window_s_host_to_its_presentation", presentation_window_hands_a_pull_window_s_host_to_its_presentation as fn()),
            ],
        );
    }
}
