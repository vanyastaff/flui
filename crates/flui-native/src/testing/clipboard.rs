//! One scenario for every host's clipboard: a concealed write
//! (ADR-0152 §6), and the known gap it fails as where a host cannot keep the
//! entry out of the clipboard's history.
//!
//! A host's test runs [`a_concealed_write_stays_out_of_history`] over its own
//! clipboard and read, and asserts its outcome: `Ok` where the host conceals
//! the entry, [`expect_fail`] with [`CONCEALED_WRITES`] where FLUI cannot yet.
//! The scenario is never skipped: a host that cannot conceal fails as the gap.

use std::sync::{Arc, Mutex};

use flui_platform::capability::{
    CompletionSource, OwnerJob, OwnerPoster, OwnerRetired, OwnerScope, PendingCompletions,
};
use flui_platform::{ClipboardRead, ClipboardWrite, Concealment};

use crate::host::NativeClipboard;

/// An entry of `docs/gaps.md`: what FLUI has not built on a host yet, which a
/// scenario fails as there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct KnownGap {
    /// The entry's number in `docs/gaps.md`.
    pub number: u32,
}

/// Gap 26: a concealed write is an ordinary one, kept by the clipboard's
/// history.
pub const CONCEALED_WRITES: KnownGap = KnownGap { number: 26 };

/// The text the scenario writes concealed.
pub const SECRET: &str = "correct horse battery staple";

/// Write [`SECRET`] concealed to `clipboard`, and check it pastes back whole
/// through `read` and the host keeps it out of the clipboard's history.
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
) -> Result<(), KnownGap> {
    clipboard.write(ClipboardWrite::text(SECRET).concealed());
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

/// Assert that a scenario's `outcome` is the known failure `gap`: the run
/// passes while the gap is open on this host, and fails once the scenario
/// passes, so the fix closes the gap's entry and this call with it.
///
/// # Panics
///
/// When `outcome` passes, or fails as another gap.
pub fn expect_fail(gap: KnownGap, outcome: Result<(), KnownGap>) {
    match outcome {
        Err(failed) if failed == gap => {
            eprintln!("expected failure: gap {} is open on this host", gap.number);
        }
        Err(failed) => panic!(
            "the scenario failed as gap {}, expected gap {}",
            failed.number, gap.number
        ),
        Ok(()) => panic!(
            "gap {} is closed on this host: remove its expect_fail and update docs/gaps.md",
            gap.number
        ),
    }
}

/// Read `clipboard` where its host answers at once (the in-memory clipboard,
/// the web's fallback): the answer settled as an owner turn settles it.
///
/// # Panics
///
/// When the host does not answer at once, or the read fails.
#[must_use]
pub fn read_at_once(clipboard: &dyn NativeClipboard) -> Option<Arc<str>> {
    let queue = Arc::new(Mutex::new(Vec::<OwnerJob>::new()));
    let sink = Arc::clone(&queue);
    let poster = OwnerPoster::new(move |job| {
        sink.lock().map_err(|_| OwnerRetired)?.push(job);
        Ok(())
    });
    let pending = PendingCompletions::new();
    let source = CompletionSource::new(poster, pending.clone());
    let (mut completion, resolver) = source.pending::<ClipboardRead>();
    clipboard.read_text(resolver);
    let jobs = std::mem::take(&mut *queue.lock().expect("BUG: the queue is never poisoned"));
    let scope = OwnerScope::new(&pending);
    for job in jobs {
        job.run(&scope);
    }
    completion
        .poll()
        .expect("the host answers a read at once")
        .expect("the read is answered, not abandoned")
        .expect("the read succeeds")
}
