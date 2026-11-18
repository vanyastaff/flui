//! Grants on Android: a pick's lasting access is its content URI, whose read
//! permission the application persisted once the picks were read
//! (`ContentResolver.takePersistableUriPermission`).
//!
//! Resolving a grant reaches the document again on a worker: the
//! application must still hold the persisted permission
//! (`getPersistedUriPermissions`, which Android trims to a bounded number
//! per application, the oldest first, and empties when the user clears the
//! application's data), and the document is read as a pick is, its declared
//! size held to the request's bound before a byte is read and the bytes
//! read counted again. A document is never handed over as a path: the
//! reacquired file is its name and contents, as a pick is.

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::Resolver;
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use super::documents::{Interrupt, LetGo, PickedDocuments};
use super::workers::{NotRun, Workers};
use crate::dialog::{DialogError, PickedFile};
use crate::grant::{
    Access, Grant, GrantBackend, GrantCapability, GrantError, GrantKind, GrantResult, ResolveHold,
};
use crate::sandbox::{Sandbox, android_app};

/// `EACCES`, on Linux and so on Android.
const EACCES: i32 = 13;
/// `ENOENT`, on Linux and so on Android.
const ENOENT: i32 = 2;

/// The grant of a document whose URI is `uri`, persisted by its provider.
pub(crate) fn grant_of(uri: String) -> Option<Grant> {
    Grant::try_new(GrantKind::ContentUri, uri.into_bytes())
}

/// What a document read failed with, as a grant answers it: a grant that
/// ended is `Revoked`, a document that is gone `Missing`.
fn grant_error(error: DialogError) -> GrantError {
    match error {
        DialogError::Failed(SystemFailure::Os(EACCES)) => GrantError::Revoked,
        DialogError::Failed(SystemFailure::Os(ENOENT)) => GrantError::Missing,
        DialogError::TooLarge { actual, limit } => GrantError::TooLarge { actual, limit },
        DialogError::Failed(failure) => GrantError::Failed(failure),
        _ => GrantError::Failed(SystemFailure::MalformedAnswer),
    }
}

/// The one document `documents` names, reacquired: refused `Revoked` unless
/// the application still holds its persisted permission, `Missing` when its
/// provider describes it no more, and read within `max_bytes`, its declared
/// size checked first, until `let_go` is set.
pub(crate) fn reacquire(
    documents: &mut impl PickedDocuments,
    max_bytes: u64,
    let_go: &LetGo,
) -> GrantResult {
    if documents.len() != 1 {
        return Err(GrantError::Failed(SystemFailure::MalformedAnswer));
    }
    if !documents.is_kept(0) {
        return Err(GrantError::Revoked);
    }
    let described = documents.describe(0);
    // A provider answers no row for a document it no longer has.
    let name = described.name.ok_or(GrantError::Missing)?;
    if let Some(actual) = described.size.filter(|size| *size > max_bytes) {
        return Err(GrantError::TooLarge {
            actual,
            limit: max_bytes,
        });
    }
    // One byte past the bound shows a document larger than it declared.
    let contents = documents
        .read(0, max_bytes.saturating_add(1), let_go)
        .map_err(grant_error)?;
    let actual = u64::try_from(contents.len()).unwrap_or(u64::MAX);
    if actual > max_bytes {
        return Err(GrantError::TooLarge {
            actual,
            limit: max_bytes,
        });
    }
    let file = PickedFile::try_from_contents(name, Arc::<[u8]>::from(contents))
        .ok_or(GrantError::Failed(SystemFailure::MalformedAnswer))?;
    Ok(Access::new(file, None))
}

/// How the owner reaches the document a URI names: the application's
/// content resolver on Android (the URI parsed, a cancellation signal made
/// for the reads), a stand-in in the tests. No provider is called here.
pub(crate) trait DocumentReach: 'static {
    /// What a worker reads the document through.
    type Documents: PickedDocuments;

    /// The document `uri` names, and what interrupts the provider's calls of
    /// its worker; `None` when the URI cannot be read as one.
    fn reach(&self, uri: &str) -> Option<(Self::Documents, Interrupt)>;
}

/// [`GrantBackend`] over persisted URI permissions, on the runtime's workers.
pub(crate) struct AndroidGrants<R: DocumentReach> {
    reach: Rc<R>,
    workers: Rc<Workers>,
}

impl<R: DocumentReach> AndroidGrants<R> {
    pub(crate) fn new(reach: Rc<R>, workers: Rc<Workers>) -> Self {
        Self { reach, workers }
    }
}

/// A resolve's hold: dropped before the answer, it stops the worker's read
/// before its next chunk and cancels the provider's calls that take the
/// signal (a query, an open), so the worker's place comes back.
struct ReadHold {
    let_go: LetGo,
    interrupt: Option<Interrupt>,
}

impl Drop for ReadHold {
    fn drop(&mut self) {
        self.let_go.set();
        if let Some(interrupt) = self.interrupt.take() {
            interrupt();
        }
    }
}

/// What a worker's refusal answers a grant.
fn not_run(reason: NotRun) -> GrantResult {
    Err(match reason {
        NotRun::Busy => GrantError::Unsupported(Unsupported::of::<GrantCapability>(
            UnsupportedReason::Unavailable,
        )),
        NotRun::NoThread(code) => {
            GrantError::Failed(code.map_or(SystemFailure::MalformedAnswer, SystemFailure::Os))
        }
    })
}

impl<R: DocumentReach> GrantBackend for AndroidGrants<R> {
    fn kind(&self) -> GrantKind {
        GrantKind::ContentUri
    }

    /// An Android application reaches no path: what it reacquires is
    /// contents.
    fn sandbox(&self, answer: Resolver<Sandbox>) {
        // The owner retiring first leaves nothing waiting.
        let _ = answer.resolve(android_app());
    }

    fn resolve(&self, grant: Grant, max_bytes: u64, answer: Resolver<GrantResult>) -> ResolveHold {
        let malformed = GrantError::Failed(SystemFailure::MalformedAnswer);
        // A grant this library made holds a URI's text.
        let reached = std::str::from_utf8(grant.payload())
            .ok()
            .and_then(|uri| self.reach.reach(uri));
        let Some((mut documents, interrupt)) = reached else {
            // The owner retiring first leaves nothing waiting.
            let _ = answer.resolve(Err(malformed));
            return ResolveHold::new(());
        };
        let hold = ReadHold {
            let_go: LetGo::default(),
            interrupt: Some(interrupt),
        };
        let let_go = hold.let_go.clone();
        self.workers
            .run("flui-system-grant", answer, not_run, move || {
                reacquire(&mut documents, max_bytes, &let_go)
            });
        ResolveHold::new(hold)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::mpsc::Sender;

    use flui_platform::SystemFailure;

    use super::{AndroidGrants, DocumentReach, Interrupt, LetGo, grant_of, reacquire};
    use crate::dialog::{DialogError, PickedForm};
    use crate::grant::{GrantError, GrantResult, Grants, Resolving};
    use crate::testing::android::{Document, DocumentsStandIn, Step, Steps};
    use crate::testing::owner::TestOwner;

    fn documents(document: Document) -> (DocumentsStandIn, Steps) {
        let steps = Steps::default();
        (DocumentsStandIn::new(vec![document], &steps), steps)
    }

    fn kept(name: &str, contents: &[u8]) -> Document {
        Document {
            persisted: true,
            ..Document::new(name, "text/plain", contents)
        }
    }

    fn read(documents: &mut DocumentsStandIn, max_bytes: u64) -> Option<GrantError> {
        reacquire(documents, max_bytes, &LetGo::default()).err()
    }

    #[test]
    fn a_document_is_reacquired_only_while_its_permission_is_held_and_within_the_bound() {
        let (mut held, _) = documents(kept("notes.txt", b"notes"));
        let access = reacquire(&mut held, 5, &LetGo::default()).expect("the permission is held");
        assert_eq!(
            access.file().form(),
            PickedForm::Contents {
                name: "notes.txt",
                contents: b"notes"
            }
        );
        assert!(access.file().path().is_none(), "no path on Android");

        let (mut dropped, steps) = documents(Document::new("notes.txt", "text/plain", b"x"));
        assert_eq!(read(&mut dropped, 5), Some(GrantError::Revoked));
        assert!(
            steps.taken().is_empty(),
            "a document no longer granted is not read"
        );

        let (mut large, steps) = documents(kept("big.bin", b"123456"));
        let too_large = Some(GrantError::TooLarge {
            actual: 6,
            limit: 5,
        });
        assert_eq!(read(&mut large, 5), too_large);
        assert!(
            !steps
                .taken()
                .iter()
                .any(|step| matches!(step, Step::Read(..))),
            "refused by its declared size before a byte is read"
        );

        let mut undeclared = kept("big.bin", b"123456");
        undeclared.described.size = None;
        let (mut undeclared, _) = documents(undeclared);
        assert_eq!(
            read(&mut undeclared, 5),
            too_large,
            "the bytes read are counted again"
        );

        let mut gone = kept("gone.txt", b"");
        gone.described.name = None;
        let (mut gone, _) = documents(gone);
        assert_eq!(read(&mut gone, 5), Some(GrantError::Missing));

        for (error, answer) in [
            (
                DialogError::Failed(SystemFailure::Os(13)),
                GrantError::Revoked,
            ),
            (
                DialogError::Failed(SystemFailure::Os(2)),
                GrantError::Missing,
            ),
        ] {
            let mut failing = kept("a.txt", b"a");
            failing.read_error = Some(error);
            let (mut failing, _) = documents(failing);
            assert_eq!(read(&mut failing, 5), Some(answer), "{error:?}");
        }
    }

    /// Reaches each URI as one stand-in document, its reads held until the
    /// test lets them go when `gated`.
    struct StandInReach {
        document: Document,
        steps: Steps,
        gate: RefCell<Option<Sender<()>>>,
        gated: bool,
    }

    impl StandInReach {
        fn new(document: Document, gated: bool) -> Self {
            Self {
                document,
                steps: Steps::default(),
                gate: RefCell::new(None),
                gated,
            }
        }
    }

    impl DocumentReach for StandInReach {
        type Documents = DocumentsStandIn;

        fn reach(&self, _uri: &str) -> Option<(DocumentsStandIn, Interrupt)> {
            let documents = DocumentsStandIn::new(vec![self.document.clone()], &self.steps);
            let documents = if self.gated {
                let (documents, gate) = documents.holding_reads();
                self.gate.replace(Some(gate));
                documents
            } else {
                documents
            };
            let steps = self.steps.clone();
            Some((documents, Box::new(move || steps.push(Step::Interrupted))))
        }
    }

    fn grants(reach: &Rc<StandInReach>, owner: &TestOwner) -> Grants {
        Grants::new(
            AndroidGrants::new(Rc::clone(reach), Rc::default()),
            &owner.ctx(None),
        )
    }

    fn settle(owner: &TestOwner, resolving: Resolving) -> GrantResult {
        let landed = Rc::new(RefCell::new(None));
        let into = Rc::clone(&landed);
        resolving.on_complete(move |outcome| {
            into.replace(Some(outcome));
        });
        owner.turn_until(|| landed.borrow().is_some());
        landed
            .take()
            .expect("BUG: the loop above saw it land")
            .expect("not abandoned")
    }

    #[test]
    fn a_grant_resolves_on_a_worker_into_the_documents_contents() {
        let owner = TestOwner::new();
        let reach = Rc::new(StandInReach::new(kept("notes.txt", b"notes"), false));
        let grant = grant_of("content://com.example.documents/document/7".to_owned())
            .expect("a URI fits a grant");
        let access = settle(&owner, grants(&reach, &owner).resolve(&grant, 1024))
            .expect("the permission is held");
        assert_eq!(access.file().contents(), Some(&b"notes"[..]));
    }

    #[test]
    fn a_dropped_resolve_cancels_its_calls_and_stops_its_read() {
        let owner = TestOwner::new();
        let reach = Rc::new(StandInReach::new(kept("notes.txt", b"notes"), true));
        let grants = grants(&reach, &owner);
        let grant = grant_of("content://com.example.documents/document/7".to_owned())
            .expect("a URI fits a grant");
        let resolving = grants.resolve(&grant, 1024);
        owner.turn_until(|| {
            reach
                .steps
                .taken()
                .iter()
                .any(|step| matches!(step, Step::Read(..)))
        });
        drop(resolving);
        assert!(
            reach.steps.taken().contains(&Step::Interrupted),
            "the provider's calls are cancelled when the caller lets go"
        );
        // The read waiting on its provider goes on, finds nobody waits, and
        // stops before it reads a chunk, which frees its worker.
        drop(reach.gate.take());
        owner.turn_until(|| reach.steps.taken().contains(&Step::Stopped(0)));
    }
}
