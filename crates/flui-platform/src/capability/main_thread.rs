//! Work a capability provider hands to Android's Java main thread through
//! the host ([`Activity::on_main_thread`](super::Activity::on_main_thread)).
//!
//! Some of Android's calls belong on its main thread: starting an activity,
//! showing a dialog. The host is the one owner of that thread's queue
//! (ADR-0340 §2): it runs each piece of work inside its panic boundary, ends
//! work Android never takes at a deadline of its own, and refuses work once
//! the Activity has ended, so a provider never posts there by itself and
//! never learns these rules twice.

use std::fmt;

use super::JniFrame;

/// What a [`PostedWork`] is handed once: the frame it runs in, or why it
/// does not run.
type Body = Box<dyn FnOnce(Result<JniFrame, MainThreadRefused>) + Send>;

/// One piece of work for Android's main thread, which ends exactly once: the
/// host runs it there ([`run`](Self::run)) or refuses it
/// ([`refuse`](Self::refuse)); dropped without either, it is refused as
/// [`MainThreadRefused::HandOffFailed`]. No path leaves its caller waiting.
pub struct PostedWork {
    body: Option<Body>,
}

impl PostedWork {
    /// Work whose `body` is handed the frame it runs in, or why it does not
    /// run.
    pub(crate) fn new(
        body: impl FnOnce(Result<JniFrame, MainThreadRefused>) + Send + 'static,
    ) -> Self {
        Self {
            body: Some(Box::new(body)),
        }
    }

    /// Run the work on Android's main thread with the Activity and that
    /// thread's environment lent in `frame`.
    pub fn run(mut self, frame: JniFrame) {
        if let Some(body) = self.body.take() {
            body(Ok(frame));
        }
    }

    /// Refuse the work for `reason`: it never runs.
    pub fn refuse(mut self, reason: MainThreadRefused) {
        if let Some(body) = self.body.take() {
            body(Err(reason));
        }
    }
}

impl Drop for PostedWork {
    fn drop(&mut self) {
        // Dropped by a host that neither ran nor refused it (android-activity
        // dropping a closure it could not deliver): still answered.
        if let Some(body) = self.body.take() {
            body(Err(MainThreadRefused::HandOffFailed));
        }
    }
}

impl fmt::Debug for PostedWork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostedWork")
            .field("ended", &self.body.is_none())
            .finish()
    }
}

/// The most pieces of work the host holds that Android's main thread has not
/// finished, every capability together; one more is refused as
/// [`MainThreadRefused::TooMuchPosted`].
pub const MAX_POSTED_WORK: usize = 16;

/// Why work handed to Android's main thread did not run to its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum MainThreadRefused {
    /// The Activity has ended: the work does not run.
    #[error("the Activity has ended")]
    ActivityEnded,
    /// [`MAX_POSTED_WORK`] pieces of work wait for Android's main thread;
    /// one must finish first.
    #[error("as much work waits for Android's main thread as the host holds")]
    TooMuchPosted,
    /// The host could not hand the work over, or Android's main thread did
    /// not take it within the host's limit: the work never runs.
    #[error("the work did not reach Android's main thread")]
    HandOffFailed,
}
