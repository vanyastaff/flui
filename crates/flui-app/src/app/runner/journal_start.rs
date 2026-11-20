//! How every runner starts its native journal (ADR-0288 §11, ADR-0390): the capture is begun on
//! the loop's thread before the platform starts, since a stream reports only to the capture
//! present when it opened, and it outlives that start only when the launch is read and the
//! driver agent is served. [`JournalStart`] holds the journal from its begin until then: dropped
//! first, as when the platform fails to start or the runner unwinds, it ends the journal, so no
//! runner leaves a capture installed that nobody reads.
//!
//! In a build that journals nothing (`cfg(native_journal)` unset) the guard does nothing.

use flui_protocol::native_recording::Backend;

/// The loop's native journal between its begin and the read of the launch.
#[derive(Debug)]
#[must_use = "dropping it ends the native journal"]
pub(super) struct JournalStart {
    /// A journal was begun: the build journals and named a backend.
    begun: bool,
    /// The journal was settled, or handed to the loop that settles it: the guard lets go.
    released: bool,
}

impl JournalStart {
    /// Begin this loop's journal of `backend`, replacing one an earlier loop on this thread
    /// left; `None` when the build has no backend to name, and journals nothing.
    pub(super) fn begin(backend: Option<Backend>) -> Self {
        #[cfg(native_journal)]
        let begun = backend.map(super::native_journal::begin).is_some();
        #[cfg(not(native_journal))]
        let begun = {
            let _ = backend;
            false
        };
        Self {
            begun,
            released: false,
        }
    }

    /// Whether the loop journals its backend, so the agent's hello lists the `native` layer.
    #[cfg_attr(
        not(any(target_arch = "wasm32", target_os = "android", test)),
        expect(
            dead_code,
            reason = "a loop that reads its launch later asks `journals_native`"
        )
    )]
    pub(super) fn journals(&self) -> bool {
        self.begun && super::journals_native()
    }

    /// The launch was read: keep the journal when the driver agent is `served`, end it
    /// otherwise, so an app nobody drives pays nothing for it.
    #[cfg_attr(
        not(any(target_arch = "wasm32", target_os = "android", test)),
        expect(
            dead_code,
            reason = "a loop that reads its launch later settles it there"
        )
    )]
    pub(super) fn settle(mut self, served: bool) {
        self.released = true;
        super::keep_native_journal(served);
    }

    /// The platform started on a loop that reads the launch later and settles the journal
    /// there (`keep_native_journal`): the guard lets go of it.
    #[cfg_attr(
        all(any(target_os = "android", target_arch = "wasm32"), not(test)),
        expect(
            dead_code,
            reason = "Android and the web read the launch where they start their platform"
        )
    )]
    pub(super) fn release_to_loop(mut self) {
        self.released = true;
    }
}

impl Drop for JournalStart {
    fn drop(&mut self) {
        if !self.released {
            super::keep_native_journal(false);
        }
    }
}

/// Start a loop whose launch is read right after its platform starts, as a page's is: begin
/// the journal of `backend`, `start` the platform, and `install` the driver agent from its
/// launch, told whether the loop journals; the journal is kept when `install` says the agent is
/// served, and ended otherwise, and when `start` fails or either unwinds.
///
/// # Errors
///
/// What `start` returns.
#[cfg_attr(
    not(any(target_arch = "wasm32", test)),
    expect(dead_code, reason = "the web runner starts its loop through it")
)]
pub(super) fn start_journaled<P, E>(
    backend: Backend,
    start: impl FnOnce() -> Result<P, E>,
    install: impl FnOnce(&P, bool) -> bool,
) -> Result<P, E> {
    let journal = JournalStart::begin(Some(backend));
    let platform = start()?;
    let served = install(&platform, journal.journals());
    journal.settle(served);
    Ok(platform)
}

#[cfg(all(test, native_journal))]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// The journal a page's start leaves: none when the platform fails to start or nobody
    /// serves the agent, the open one when the agent is served; the agent is told the loop
    /// journals.
    #[test]
    fn a_loop_keeps_its_journal_only_when_its_platform_starts_and_the_agent_is_served() {
        let told = Cell::new(None);
        let failed: Result<(), &str> = start_journaled(
            Backend::Web,
            || Err("no canvas"),
            |(), _| unreachable!("no agent is installed on a platform that did not start"),
        );
        assert_eq!(failed, Err("no canvas"));
        assert!(!super::super::journals_native(), "a failed start ends it");

        for served in [false, true] {
            start_journaled(
                Backend::Web,
                || Ok::<(), ()>(()),
                |(), journals| {
                    told.set(Some(journals));
                    served
                },
            )
            .expect("the platform starts");
            assert_eq!(told.take(), Some(true), "the agent hears the loop journals");
            assert_eq!(
                super::super::journals_native(),
                served,
                "kept exactly when served"
            );
        }
        super::super::keep_native_journal(false);
    }

    /// A journal begun before a platform is ended when its guard goes before the launch is
    /// read, and kept when the guard hands it to the loop that reads the launch.
    #[test]
    fn a_journal_whose_start_goes_unsettled_ends_and_one_handed_to_its_loop_stays() {
        drop(JournalStart::begin(Some(Backend::Headless)));
        assert!(!super::super::journals_native(), "the guard ended it");

        JournalStart::begin(Some(Backend::Headless)).release_to_loop();
        assert!(super::super::journals_native(), "the loop holds it");
        super::super::keep_native_journal(false);
        assert!(!super::super::journals_native());
    }
}
