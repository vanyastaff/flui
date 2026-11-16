//! One scenario for every host's clipboard: a concealed write
//! (ADR-0152 §6), and the known gap it fails as where a host cannot keep the
//! entry out of the clipboard's history; and the stand-in owner a test of a
//! host's clipboard settles its answers on.
//!
//! A host's test runs [`a_concealed_write_stays_out_of_history`] over its own
//! clipboard and read, and asserts its outcome: `Ok` where the host conceals
//! the entry, [`crate::testing::gap::expect_fail`] with [`CONCEALED_WRITES`]
//! where FLUI cannot yet. The scenario is never skipped: a host that cannot
//! conceal fails as the gap.

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use flui_platform::capability::{
    Abandoned, Completion, CompletionSource, OwnerJob, OwnerPoster, OwnerRetired, OwnerScope,
    PendingCompletions, Resolver,
};
use flui_platform::{ClipboardRead, ClipboardWrite, Concealment};

use crate::host::NativeClipboard;
use crate::testing::gap::GapId;

/// The entry a host fails the scenario as: a concealed write is an ordinary
/// one, kept by the clipboard's history.
pub const CONCEALED_WRITES: GapId = GapId::SecureTextEntryAndConcealedClipboard;

/// The text the scenario writes concealed.
pub const SECRET: &str = "correct horse battery staple";

/// Write [`SECRET`] concealed to `clipboard`, and check it pastes back whole
/// through `read` and the host keeps it out of the clipboard's history.
///
/// The write's own answer is not awaited: every host orders a read after the
/// write before it, so the paste back is what shows the write landed.
///
/// # Errors
///
/// [`CONCEALED_WRITES`] where the host answers [`Concealment::Unavailable`]:
/// the paste works, but the entry is an ordinary one.
///
/// # Panics
///
/// When the concealed write does not paste back as written.
pub fn a_concealed_write_stays_out_of_history(
    clipboard: &dyn NativeClipboard,
    read: impl FnOnce(&dyn NativeClipboard) -> Option<Arc<str>>,
) -> Result<(), GapId> {
    write_unobserved(clipboard, ClipboardWrite::text(SECRET).concealed());
    assert_eq!(
        read(clipboard).as_deref(),
        Some(SECRET),
        "a concealed write pastes like any other"
    );
    match clipboard.concealment() {
        Concealment::Unavailable => Err(CONCEALED_WRITES),
        _ => Ok(()),
    }
}

/// Write `write` to `clipboard` with nobody waiting for its answer, as a copy
/// that has nothing to undo does.
pub fn write_unobserved(clipboard: &dyn NativeClipboard, write: ClipboardWrite) {
    let owner = SettlingOwner::new();
    let (completion, answer) = owner.pending();
    drop(completion);
    clipboard.write(write, answer);
}

/// Read `clipboard` where its host answers at once (the in-memory clipboard,
/// the web's fallback): the answer settled as an owner turn settles it.
///
/// # Panics
///
/// When the host does not answer at once, or the read fails.
#[must_use]
pub fn read_at_once(clipboard: &dyn NativeClipboard) -> Option<Arc<str>> {
    let owner = SettlingOwner::new();
    let (mut completion, answer) = owner.pending::<ClipboardRead>();
    clipboard.read_text(answer);
    owner
        .settle_within(&mut completion, Duration::ZERO)
        .expect("the host answers a read at once")
        .expect("the read is answered, not abandoned")
        .expect("the read succeeds")
}

/// Read `clipboard` where its host answers from a thread of its own (Win32):
/// the answer settled as an owner turn settles it, waited for up to `within`.
///
/// # Panics
///
/// When the host does not answer within `within`, or the read fails.
pub fn read_within(within: Duration) -> impl FnOnce(&dyn NativeClipboard) -> Option<Arc<str>> {
    move |clipboard| {
        let owner = SettlingOwner::new();
        let (mut completion, answer) = owner.pending::<ClipboardRead>();
        clipboard.read_text(answer);
        owner
            .settle_within(&mut completion, within)
            .expect("the host answers the read in time")
            .expect("the read is answered, not abandoned")
            .expect("the read succeeds")
    }
}

/// A stand-in for the owner thread a clipboard's answers settle on: its
/// completions take resolvers that may answer from any thread, and settle
/// when [`settle_within`](Self::settle_within) runs the jobs they posted, as
/// an owner turn runs them.
pub struct SettlingOwner {
    jobs: mpsc::Receiver<OwnerJob>,
    pending: PendingCompletions,
    source: CompletionSource,
}

impl SettlingOwner {
    /// An owner with nothing pending.
    #[must_use]
    pub fn new() -> Self {
        let (sink, jobs) = mpsc::channel::<OwnerJob>();
        let poster = OwnerPoster::new(move |job| sink.send(job).map_err(|_| OwnerRetired));
        let pending = PendingCompletions::new();
        let source = CompletionSource::new(poster, pending.clone());
        Self {
            jobs,
            pending,
            source,
        }
    }

    /// A pending completion and the resolver that answers it, from any
    /// thread.
    pub fn pending<T: Send + 'static>(&self) -> (Completion<T>, Resolver<T>) {
        self.source.pending()
    }

    /// Run the jobs posted until `completion` settles, waiting up to
    /// `within` for them: its outcome, or `None` when it has not settled by
    /// then. A zero `within` runs only what is already posted, and reads no
    /// clock (the web has none `Instant` can read).
    pub fn settle_within<T: 'static>(
        &self,
        completion: &mut Completion<T>,
        within: Duration,
    ) -> Option<Result<T, Abandoned>> {
        let scope = OwnerScope::new(&self.pending);
        let run_posted = || {
            while let Ok(job) = self.jobs.try_recv() {
                job.run(&scope);
            }
        };
        run_posted();
        if within.is_zero() {
            return completion.poll();
        }
        let deadline = Instant::now().checked_add(within);
        loop {
            run_posted();
            if let Some(outcome) = completion.poll() {
                return Some(outcome);
            }
            let left = deadline.map_or(Duration::MAX, |deadline| {
                deadline.saturating_duration_since(Instant::now())
            });
            if left.is_zero() {
                return None;
            }
            match self.jobs.recv_timeout(left) {
                Ok(job) => job.run(&scope),
                Err(_) => return completion.poll(),
            }
        }
    }
}

impl Default for SettlingOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SettlingOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettlingOwner").finish_non_exhaustive()
    }
}
