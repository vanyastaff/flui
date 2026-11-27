//! The desktop loop's development restart (ADR-0393 §4): the application
//! retires, its entry runs again with the hand-over open, and the
//! application it hands over is composed into the windows the loop kept.
//!
//! The loop's own parts ([`DesktopLoop`]: the development reload, the driver
//! agent's disconnect, the terminal failure; the owner platform, the raster
//! host, the pools, the engine services) are kept. What the loop built from
//! the application is its [`ApplicationScope`](crate::app::runtime::ApplicationScope),
//! dropped and built again.

use std::rc::Rc;

use flui_foundation::panic::{FirstPanic, payload_text};
use flui_view::dev_reload::{RestartFailure, Unapplied};

use super::host::APP_RUNTIME;
use super::main_window::{KeptMain, compose};
use crate::app::LaunchError;
use crate::app::application_control::contain;
use crate::app::hot_reload::WorkerReload;
use crate::app::hot_reload::hand_over::EntryFailure;

/// How long a restart waits for the retired application's flush, its
/// windows' last placements and its services: the services' part of the
/// exit deadline, since what they write is the same.
const RETIRE_BOUND: std::time::Duration = super::owner_dispatch::SERVICE_SHUTDOWN_DEADLINE;

/// Why a restart composed no application.
enum Failure {
    Entry(EntryFailure),
    Launch(LaunchError),
    ComposePanicked,
}

impl Failure {
    fn reason(&self) -> RestartFailure<'_> {
        match self {
            Self::Entry(EntryFailure::Panicked) => RestartFailure::EntryPanicked,
            Self::Entry(EntryFailure::NoApplication) => RestartFailure::NoApplication,
            Self::Entry(EntryFailure::Launch(error)) | Self::Launch(error) => {
                RestartFailure::Launch(error)
            }
            Self::ComposePanicked => RestartFailure::ComposePanicked,
        }
    }
}

/// Run the restart a poll found due, at the end of the owner's turn, and
/// report its outcome; in a loop where no UI runtime polls the hook, poll it
/// here first. Nothing when none is due, or while a UI runtime is
/// checked out (the turn is nested in one's code): the next turn runs it.
pub(super) fn restart_if_due() {
    if super::owner_dispatch::any_runtime_checked_out() {
        return;
    }
    let Some(reload) = APP_RUNTIME.with(|slot| {
        slot.borrow()
            .desktop_loop
            .as_ref()
            .map(|desktop| desktop.reload.reload())
    }) else {
        return;
    };
    reload.poll_without_ui_runtime();
    let Some(version) = reload.take_restart_due() else {
        return;
    };
    tracing::info!(
        layout = version.layout.get(),
        "hot reload: the new build changed the layout of kept values; restarting the application"
    );
    match restart(&reload) {
        Ok(unapplied) => reload.restarted(version, unapplied),
        Err(failure) => {
            let reason = failure.reason();
            tracing::warn!(?reason, "hot reload: the restart composed no application");
            reload.restart_failed(version, reason);
        }
    }
    // §4 step 4: the platform asks the exit question again, which the new
    // application's policy answers.
    super::host::with_owner_platform(flui_native::OwnerPlatform::request_exit_policy_reevaluation);
}

/// Retire the running application, run the entry and compose what it hands
/// over (§4 steps 1 to 3), and say what of it the loop did not apply. A
/// failure after step 1 leaves the loop with its
/// kept windows and no application.
fn restart(reload: &WorkerReload) -> Result<Unapplied, Failure> {
    // The loop holds the kept window from here: whatever fails after this,
    // it stays open for the next restart.
    keep(retire_application());
    let mut handed = reload.run_entry().map_err(Failure::Entry)?;
    if let Err(error) = super::check_window_options(&handed.config) {
        contain(|| drop(handed));
        return Err(Failure::Launch(error));
    }
    let unapplied = Unapplied::default().executors(handed.config.executors.is_some());
    if unapplied.executors {
        tracing::warn!(
            "hot reload: the later application configures its executors; the loop keeps the \
             pools it started with until the process restarts"
        );
    }
    // What the operating system opens is declared to it as the run starts,
    // so the declarations stay the loop's until a process restart; the
    // later application's callback is where opened files go from now on.
    let mut opening = std::mem::take(&mut handed.opening);
    let displaced = super::host::current_open_routing()
        .and_then(|routing| routing.replace_open_files(opening.on_open_files.take()));
    contain(|| drop((displaced, opening)));
    let capabilities = std::mem::take(&mut handed.capabilities);
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: composing code that panics fails the restart, which retires what it built"
    )]
    let composed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // A plugin's `register` is user code: it runs with no borrow held.
        let slots = APP_RUNTIME.with(|slot| slot.borrow().capability_slots());
        let registry = capabilities
            .into_registry(&slots)
            .map_err(LaunchError::CapabilityConflict)?;
        compose(handed, registry)
    }));
    let failure = match composed {
        Ok(Ok(())) => return Ok(unapplied),
        Ok(Err(error)) => Failure::Launch(error),
        Err(payload) => {
            tracing::error!(
                panic = payload_text(payload.as_ref()).unwrap_or("a panic without a message"),
                "the later application's composition panicked"
            );
            flui_foundation::panic::discard_payload(payload);
            Failure::ComposePanicked
        }
    };
    // What the composition built retires as the earlier application did; the
    // window it was given stays.
    keep(retire_application());
    Err(failure)
}

/// §4 step 1: the running application retires. Its main window's controller
/// and ingress go first, so no request of an earlier `AppHandle` is served,
/// then every UI runtime unmounts (each `State` disposed by the code that
/// made it) and every window it opened closes, with no close request asked;
/// then its flush, placements and services end and the scope drops in
/// reverse build order. The first panic is logged and discarded.
///
/// Returns the main window, kept open for the next application: the
/// controller's, or the one an earlier failed restart kept.
fn retire_application() -> Option<KeptMain> {
    let mut first = FirstPanic::new();
    let (controller, ingress, kept) = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        let kept = runtime
            .desktop_loop
            .as_mut()
            .and_then(|desktop| desktop.kept.take());
        (
            runtime.application.main_controller.take(),
            runtime.application.main_ingress.take(),
            kept,
        )
    });
    if let Some(ingress) = ingress {
        ingress.close();
    }
    let mut controller = controller;
    let open = controller.as_mut().and_then(|controller| {
        first
            .contain(|| controller.retire_keeping_window())
            .flatten()
    });
    first.contain(|| drop(controller));
    let kept = match open {
        Some(open) => {
            let window = Rc::clone(&open.window);
            first.contain(|| drop(open));
            let routes = APP_RUNTIME.with(|slot| slot.borrow().window_routes(window.id()));
            routes.map(|routes| KeptMain { window, routes })
        }
        None => kept,
    };
    let kept_id = kept.as_ref().map(|kept| kept.window.id());
    let opened: Vec<_> = APP_RUNTIME
        .with(|slot| slot.borrow().open_windows())
        .into_iter()
        .filter(|window| Some(window.id()) != kept_id)
        .collect();
    super::owner_dispatch::retire_installed_host(&mut first);
    // Closed once their presentations are gone, so no close request reaches
    // a handler of the earlier application.
    for window in opened {
        first.contain(|| {
            super::discard_window(window.as_ref(), "a window of a retired application");
        });
        first.contain(|| drop(window));
    }
    super::owner_dispatch::end_application_work(
        &mut first,
        std::time::Instant::now() + RETIRE_BOUND,
        &mut |_| {},
    );
    let scope = APP_RUNTIME.with(|slot| std::mem::take(&mut slot.borrow_mut().application));
    first.contain(|| drop(scope));
    // Its callback for opened files goes with it: a file opened before the
    // later application is composed is logged, as with no callback.
    let open_files =
        super::host::current_open_routing().and_then(|routing| routing.replace_open_files(None));
    first.contain(|| drop(open_files));
    if let Some(payload) = first.into_payload() {
        tracing::error!(
            panic = payload_text(payload.as_ref()).unwrap_or("a panic without a message"),
            "the earlier application panicked while it retired; the restart goes on"
        );
        flui_foundation::panic::discard_payload(payload);
    }
    kept
}

/// Hold `kept` on the loop until a later restart composes an application
/// into it.
fn keep(kept: Option<KeptMain>) {
    let displaced = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        let desktop = runtime.desktop_loop.as_mut()?;
        std::mem::replace(&mut desktop.kept, kept)
    });
    contain(|| drop(displaced));
}
