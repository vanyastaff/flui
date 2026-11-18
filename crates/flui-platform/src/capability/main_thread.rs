//! Work a capability provider hands to Android's Java main thread through
//! the host ([`Activity::on_main_thread`](super::Activity::on_main_thread)).
//!
//! Some of Android's calls belong on its main thread: starting an activity,
//! showing a dialog. The host is the one owner of that thread's queue
//! (ADR-0340 §2): it runs each piece of work inside its panic boundary, ends
//! work Android never takes at a deadline of its own, and refuses work once
//! the Activity has ended, so a provider never posts there by itself and
//! never learns these rules twice.

use super::JniFrame;

/// What a provider runs on Android's main thread: called once there, with
/// the Activity and that thread's environment lent in the frame.
pub type MainThreadWork = Box<dyn FnOnce(JniFrame) + Send>;

/// What a provider hears when its [`MainThreadWork`] did not run to its end:
/// called once, on the owner thread or on Android's main thread.
pub type MainThreadRefusal = Box<dyn FnOnce(MainThreadRefused) + Send>;

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
    /// The host could not hand the work over, Android's main thread did not
    /// take it within the host's limit (the work then never runs), or the
    /// work panicked there.
    #[error("the work did not reach Android's main thread or did not finish there")]
    HandOffFailed,
}
