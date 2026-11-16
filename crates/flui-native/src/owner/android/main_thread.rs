//! Work the owner posts to Android's Java main thread, inside the panic
//! boundary and with the report it owes when it does not finish.
//!
//! `AndroidApp::run_on_java_main_thread` runs a closure under
//! android-activity's own guard, which aborts the process on a panic.
//! [`MainThreadWork`] runs its body inside [`os_entry`] and owes its `failed`
//! report on every exit but success: the body's error, a panic in it, and
//! being dropped unrun. The report runs inside the boundary too, so a panic
//! there is logged and the thread goes on. [`post`] hands one to the Java
//! main thread; every closure the Android host posts there goes through it,
//! which `cargo xtask panic-boundary` holds.
//!
//! android-activity keeps a closure it cannot deliver (its callback state is
//! leaked, not dropped), so on Android the report for work dropped unrun may
//! never come: nothing settles a request through it alone. A request for a
//! result has its own deadline on the owner's side
//! (`activity_results::HAND_OFF_LIMIT`).

use crate::owner::panic_boundary::{AfterPanic, os_entry};

/// Why work posted to the Java main thread did not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PostFailure {
    /// The body answered this error.
    Failed(String),
    /// The body panicked; the panic was logged.
    Panicked,
    /// The work was dropped before it ran.
    Dropped,
}

/// One piece of work for the Java main thread: its `body`, and the `failed`
/// report owed when the body does not finish.
pub(crate) struct MainThreadWork<B, F>
where
    F: FnOnce(PostFailure),
{
    entry: &'static str,
    body: Option<B>,
    failed: Option<F>,
}

impl<B, F> MainThreadWork<B, F>
where
    B: FnOnce() -> Result<(), String>,
    F: FnOnce(PostFailure),
{
    /// `body`, logged under `entry`, with `failed` owed on every exit but
    /// success.
    pub(crate) fn new(entry: &'static str, body: B, failed: F) -> Self {
        Self {
            entry,
            body: Some(body),
            failed: Some(failed),
        }
    }

    /// Run the body, then the report it owes, each inside the boundary.
    pub(crate) fn run(mut self) {
        let Some(body) = self.body.take() else {
            return;
        };
        let outcome = os_entry(
            self.entry,
            (),
            AfterPanic::Answer(|()| Err(PostFailure::Panicked)),
            || body().map_err(PostFailure::Failed),
        );
        match outcome {
            Ok(()) => drop(self.failed.take()),
            Err(failure) => self.report(failure),
        }
    }

    fn report(&mut self, failure: PostFailure) {
        if let Some(failed) = self.failed.take() {
            os_entry(self.entry, (), AfterPanic::Answer(|()| ()), || {
                failed(failure);
            });
        }
    }
}

impl<B, F> Drop for MainThreadWork<B, F>
where
    F: FnOnce(PostFailure),
{
    fn drop(&mut self) {
        // Still owed: the work never ran.
        if let Some(failed) = self.failed.take() {
            os_entry(self.entry, (), AfterPanic::Answer(|()| ()), || {
                failed(PostFailure::Dropped);
            });
        }
    }
}

/// Post `body` to `app`'s Java main thread under `entry`, with `failed`
/// owed when it does not finish (see [`MainThreadWork`]).
#[cfg(target_os = "android")]
pub(crate) fn post(
    app: &android_activity::AndroidApp,
    entry: &'static str,
    body: impl FnOnce() -> Result<(), String> + Send + 'static,
    failed: impl FnOnce(PostFailure) + Send + 'static,
) {
    let work = MainThreadWork::new(entry, body, failed);
    app.run_on_java_main_thread(Box::new(move || work.run()));
}

/// [`post`] a step whose only report is its failure, which is logged under
/// `entry`; a step that owes more (an obligation, an answer) reports it
/// through [`post`] itself.
#[cfg(target_os = "android")]
pub(crate) fn run(
    app: &android_activity::AndroidApp,
    entry: &'static str,
    body: impl FnOnce() + Send + 'static,
) {
    post(
        app,
        entry,
        move || {
            body();
            Ok(())
        },
        move |failure| {
            tracing::warn!(
                target: "flui_native::android",
                entry,
                ?failure,
                "work posted to the Java main thread did not finish"
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::{MainThreadWork, PostFailure};

    fn reported() -> (Rc<RefCell<Vec<PostFailure>>>, impl FnOnce(PostFailure)) {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&seen);
        (seen, move |failure| sink.borrow_mut().push(failure))
    }

    /// Work that finishes owes nothing; work whose body fails, panics or is
    /// dropped unrun reports exactly once, with its reason, and a panic
    /// stays inside the boundary.
    #[test]
    fn posted_work_reports_every_exit_but_success() {
        let (seen, failed) = reported();
        MainThreadWork::new("test", || Ok(()), failed).run();
        assert!(seen.borrow().is_empty());

        let (seen, failed) = reported();
        MainThreadWork::new("test", || Err("no class".to_owned()), failed).run();
        assert_eq!(*seen.borrow(), [PostFailure::Failed("no class".to_owned())]);

        let (seen, failed) = reported();
        MainThreadWork::new(
            "test",
            || -> Result<(), String> { panic!("in the body") },
            failed,
        )
        .run();
        assert_eq!(*seen.borrow(), [PostFailure::Panicked]);

        let (seen, failed) = reported();
        drop(MainThreadWork::new("test", || Ok(()), failed));
        assert_eq!(*seen.borrow(), [PostFailure::Dropped]);
    }

    /// A report that panics stays inside the boundary too.
    #[test]
    fn a_report_that_panics_is_contained() {
        MainThreadWork::new(
            "test",
            || Err("refused".to_owned()),
            |_: PostFailure| panic!("in the report"),
        )
        .run();
    }
}
