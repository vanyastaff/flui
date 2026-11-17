//! Work the owner posts to Android's Java main thread, inside the panic
//! boundary and with the report it owes when it does not finish.
//!
//! `AndroidApp::run_on_java_main_thread` runs a closure under
//! android-activity's own `catch_unwind`, which only logs a panic through the
//! `log` crate and drops its payload: whatever the closure owed (an answer, a
//! settled state, an event for the owner) is lost with no report.
//! [`MainThreadWork`] runs its body inside [`os_entry`] and owes its `failed`
//! report on every exit but success: the body's error, a panic in it, and
//! being dropped unrun. The report runs inside the boundary too, so a panic
//! there is logged and the thread goes on. [`post`] hands one to the Java
//! main thread; every closure the Android host posts there goes through it,
//! which `cargo xtask panic-boundary` holds. Java receives nothing from a
//! posted closure, and an exception its JNI calls leave pending is caught by
//! android-activity's attachment of the thread, so the Activity goes on.
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

    /// Run the body, then the report it owes or, on success, let the report
    /// go: each inside the boundary, so every capture of the work is
    /// destroyed there too (the last reference to a host object may be
    /// one of them).
    pub(crate) fn run(mut self) {
        let Some(body) = self.body.take() else {
            return;
        };
        let outcome = os_entry(
            self.entry,
            (),
            AfterPanic::Answer(|()| Err(PostFailure::Panicked)),
            move || body().map_err(PostFailure::Failed),
        );
        let failed = self.failed.take();
        os_entry(self.entry, (), AfterPanic::Answer(|()| ()), move || {
            if let (Err(failure), Some(failed)) = (outcome, failed) {
                failed(failure);
            }
        });
    }
}

impl<B, F> Drop for MainThreadWork<B, F>
where
    F: FnOnce(PostFailure),
{
    fn drop(&mut self) {
        // Still owed: the work never ran. Its body's captures are destroyed
        // inside the boundary with the report, whichever of them panics.
        let (body, failed) = (self.body.take(), self.failed.take());
        if body.is_none() && failed.is_none() {
            return;
        }
        os_entry(self.entry, (), AfterPanic::Answer(|()| ()), move || {
            let _body = body;
            if let Some(failed) = failed {
                failed(PostFailure::Dropped);
            }
        });
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

    /// A capture whose destructor panics: destroyed outside the boundary, the
    /// panic would leave `run` or the drop.
    struct PanicsWhenDestroyed;

    impl Drop for PanicsWhenDestroyed {
        fn drop(&mut self) {
            panic!("a capture's destructor panicked");
        }
    }

    /// Every capture of posted work is destroyed inside the boundary on every
    /// exit: the report's after success, the body's and the report's after a
    /// failure, and both when the work is dropped unrun.
    #[test]
    fn every_capture_of_posted_work_is_destroyed_inside_the_boundary() {
        let held = PanicsWhenDestroyed;
        MainThreadWork::new(
            "test",
            || Ok(()),
            move |_: PostFailure| {
                let _held = &held;
            },
        )
        .run();

        let held = PanicsWhenDestroyed;
        MainThreadWork::new(
            "test",
            move || -> Result<(), String> {
                let _held = &held;
                Err("refused".to_owned())
            },
            |_: PostFailure| {},
        )
        .run();

        // One panicking capture per work: a second destructor panicking while
        // the first unwinds aborts, as in std.
        let held = PanicsWhenDestroyed;
        drop(MainThreadWork::new(
            "test",
            move || {
                let _held = &held;
                Ok(())
            },
            |_: PostFailure| {},
        ));

        let held = PanicsWhenDestroyed;
        drop(MainThreadWork::new(
            "test",
            || Ok(()),
            move |_: PostFailure| {
                let _held = &held;
            },
        ));
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
