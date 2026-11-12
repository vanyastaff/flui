//! What `#[flui::main]` expands to. Not application surface: the attribute is.
//!
//! On Android the system loads the application's `cdylib` and calls the
//! `android_main` the attribute writes with the `AndroidApp` it starts the
//! application with. [`android`] hands that `AndroidApp` to the run on this
//! thread and calls the application's entry function, whose
//! [`Application::run`](crate::Application::run) takes it. On every other
//! target the attribute adds nothing, and this module has nothing.

/// The value the system starts an Android application with.
#[cfg(target_os = "android")]
pub use android_activity::AndroidApp;

/// What an entry function may return: nothing, or a `Result` whose error is
/// logged, since `android_main` has no caller to return it to.
#[diagnostic::on_unimplemented(
    message = "a `#[flui::main]` function returns `()` or `Result<(), E>` with a displayable `E`, not `{Self}`",
    label = "returns `{Self}`"
)]
pub trait EntryOutcome {
    /// Whether this outcome carries a failure to report.
    fn failed(&self) -> bool;

    /// Log the failure this outcome carries, if any.
    fn report(self);
}

impl EntryOutcome for () {
    fn failed(&self) -> bool {
        false
    }

    fn report(self) {}
}

impl<E: std::fmt::Display> EntryOutcome for Result<(), E> {
    fn failed(&self) -> bool {
        self.is_err()
    }

    fn report(self) {
        if let Err(error) = self {
            tracing::error!(%error, "FLUI could not start");
        }
    }
}

/// The body of the `android_main` that `#[flui::main]` writes: `app` is kept
/// for the run `entry` starts on this thread, then `entry` runs and its
/// failure is logged. An `AndroidApp` the run did not take is dropped when
/// `entry` returns, so a later `android_main` starts clean.
#[cfg(target_os = "android")]
pub fn android<R: EntryOutcome>(app: AndroidApp, entry: fn() -> R) {
    crate::app::runner::hand_android_entry(app);
    let outcome = entry();
    // Whatever the run left: an entry that never ran one does not leak the
    // `AndroidApp` into the next start.
    drop(crate::app::runner::take_android_entry());
    if outcome.failed() {
        // An entry that failed before its run started (a `?` on loading its
        // configuration) left no subscriber: install FLUI's default, which
        // keeps one the application installed itself, so the failure reaches
        // logcat instead of a subscriber-less `tracing` event.
        let _installation =
            crate::app::logging::init_managed_logging(&crate::app::AppConfig::default());
        outcome.report();
    }
}

/// Compiles only for an entry function whose outcome is an [`EntryOutcome`];
/// `#[flui::main]` names it on every target.
pub fn check<R: EntryOutcome>(_entry: fn() -> R) {}
