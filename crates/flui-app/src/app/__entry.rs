//! What `#[flui::main]` expands to. Not application surface: the attribute is.
//!
//! The attribute reads the crate's `[package.metadata.flui.declares]` as it
//! compiles, and the function it marks starts with [`declare`]: the URL
//! schemes and web domains the manifest registers with the operating system
//! are the ones a run that function starts opens, unless the application
//! gives its own with [`Application::declares`](crate::Application::declares).
//!
//! On Android the system loads the application's `cdylib` and calls the
//! `android_main` the attribute writes with the `AndroidApp` it starts the
//! application with. `android` hands that `AndroidApp` to the run on this
//! thread and calls the application's entry function, whose
//! [`Application::run`](crate::Application::run) takes it.

/// The value the system starts an Android application with.
#[cfg(target_os = "android")]
pub use android_activity::AndroidApp;
/// What the application opens.
pub use flui_view::navigation::LinkDeclarations;

/// How many entries [`join`] gives for `parts`.
#[must_use]
pub const fn joined_len(parts: &[&[&'static str]]) -> usize {
    let mut len = 0;
    let mut part = 0;
    while part < parts.len() {
        len += parts[part].len();
        part += 1;
    }
    len
}

/// The entries of `parts` in order, in one array: the manifest's own list
/// followed by the lists of the features the crate is compiled with, which
/// the attribute chooses with `cfg!` and leaves empty otherwise.
///
/// # Panics
///
/// When `N` is not [`joined_len`] of `parts`, as the compilation of the
/// attribute's constant.
#[must_use]
pub const fn join<const N: usize>(parts: &[&[&'static str]]) -> [&'static str; N] {
    assert!(
        joined_len(parts) == N,
        "BUG: an array joins as many entries as its parts hold"
    );
    let mut joined = [""; N];
    let mut at = 0;
    let mut part = 0;
    while part < parts.len() {
        let mut entry = 0;
        while entry < parts[part].len() {
            joined[at] = parts[part][entry];
            at += 1;
            entry += 1;
        }
        part += 1;
    }
    joined
}

/// Record `declarations` for the runs started on this thread until the
/// returned value drops, at the end of the entry function: a run reads them
/// as it starts when its `Application` declares nothing itself.
pub fn declare(declarations: LinkDeclarations) -> Declared {
    let previous = match crate::app::runner::swap_entry_declarations(Some(declarations)) {
        Ok(previous) => Restore::Previous(previous),
        Err(_busy) => {
            tracing::warn!(
                "a `#[flui::main]` function was called from inside a run; \
                 its manifest's declarations are not recorded"
            );
            Restore::Nothing
        }
    };
    Declared {
        previous,
        _owner_thread: std::marker::PhantomData,
    }
}

/// The record [`declare`] made, undone when it drops.
#[must_use = "the declarations last while this value lives"]
pub struct Declared {
    /// What drop puts back.
    previous: Restore,
    /// The record lives on the thread that made it.
    _owner_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

/// What a [`Declared`] puts back when it drops.
enum Restore {
    /// Nothing was recorded, so nothing is undone.
    Nothing,
    /// What was recorded before the entry started.
    Previous(Option<LinkDeclarations>),
}

impl std::fmt::Debug for Declared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Declared").finish_non_exhaustive()
    }
}

impl Drop for Declared {
    fn drop(&mut self) {
        if let Restore::Previous(previous) = std::mem::replace(&mut self.previous, Restore::Nothing)
            && crate::app::runner::swap_entry_declarations(previous).is_err()
        {
            tracing::warn!(
                "a `#[flui::main]` function returned inside a run; its declarations stay recorded"
            );
        }
    }
}

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
