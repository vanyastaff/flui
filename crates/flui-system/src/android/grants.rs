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

use std::sync::Arc;

use flui_platform::capability::Resolver;
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use super::documents::{LetGo, PickedDocuments};
use super::workers::{NotRun, Workers};
use crate::dialog::{DialogError, PickedFile};
use crate::grant::{
    Access, Grant, GrantBackend, GrantCapability, GrantError, GrantKind, GrantResult,
};

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
/// size checked first.
pub(crate) fn reacquire(documents: &mut impl PickedDocuments, max_bytes: u64) -> GrantResult {
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
        .read(0, max_bytes.saturating_add(1), &LetGo::default())
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
    Ok(Access::new(file, None, None))
}

/// How a worker reaches the document a URI names: the application's content
/// resolver on Android, a stand-in in the tests.
pub(crate) trait DocumentReach: Send + Sync + 'static {
    /// What the worker reads the document through.
    type Documents: PickedDocuments;

    /// The document `uri` names; `None` when the URI cannot be read as one.
    fn reach(&self, uri: &str) -> Option<Self::Documents>;
}

/// [`GrantBackend`] over persisted URI permissions, on the runtime's workers.
pub(crate) struct AndroidGrants<R: DocumentReach> {
    reach: Arc<R>,
    workers: std::rc::Rc<Workers>,
}

impl<R: DocumentReach> AndroidGrants<R> {
    pub(crate) fn new(reach: Arc<R>, workers: std::rc::Rc<Workers>) -> Self {
        Self { reach, workers }
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

    fn resolve(&self, grant: Grant, max_bytes: u64, answer: Resolver<GrantResult>) {
        let Ok(uri) = String::from_utf8(grant.payload().to_vec()) else {
            // A grant this library made holds a URI's text.
            let _ = answer.resolve(Err(GrantError::Failed(SystemFailure::MalformedAnswer)));
            return;
        };
        let reach = Arc::clone(&self.reach);
        self.workers
            .run("flui-system-grant", answer, not_run, move || {
                let mut documents = reach
                    .reach(&uri)
                    .ok_or(GrantError::Failed(SystemFailure::MalformedAnswer))?;
                reacquire(&mut documents, max_bytes)
            });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_platform::SystemFailure;

    use super::{AndroidGrants, DocumentReach, grant_of, reacquire};
    use crate::dialog::{DialogError, PickedForm};
    use crate::grant::{GrantError, Grants};
    use crate::testing::android::{Document, DocumentsStandIn, Steps};
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

    #[test]
    fn a_document_is_reacquired_only_while_its_permission_is_held_and_within_the_bound() {
        let (mut held, _) = documents(kept("notes.txt", b"notes"));
        let access = reacquire(&mut held, 5).expect("the permission is held");
        assert_eq!(
            access.file().form(),
            PickedForm::Contents {
                name: "notes.txt",
                contents: b"notes"
            }
        );
        assert!(access.file().path().is_none(), "no path on Android");

        let (mut dropped, steps) = documents(Document::new("notes.txt", "text/plain", b"x"));
        assert_eq!(reacquire(&mut dropped, 5).err(), Some(GrantError::Revoked));
        assert!(
            steps.taken().is_empty(),
            "a document no longer granted is not read"
        );

        let (mut large, steps) = documents(kept("big.bin", b"123456"));
        assert_eq!(
            reacquire(&mut large, 5).err(),
            Some(GrantError::TooLarge {
                actual: 6,
                limit: 5
            })
        );
        assert!(
            !steps
                .taken()
                .iter()
                .any(|step| matches!(step, crate::testing::android::Step::Read(..))),
            "refused by its declared size before a byte is read"
        );

        let mut undeclared = kept("big.bin", b"123456");
        undeclared.described.size = None;
        let (mut undeclared, _) = documents(undeclared);
        assert_eq!(
            reacquire(&mut undeclared, 5).err(),
            Some(GrantError::TooLarge {
                actual: 6,
                limit: 5
            }),
            "the bytes read are counted again"
        );

        let mut gone = kept("gone.txt", b"");
        gone.described.name = None;
        let (mut gone, _) = documents(gone);
        assert_eq!(reacquire(&mut gone, 5).err(), Some(GrantError::Missing));

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
            assert_eq!(reacquire(&mut failing, 5).err(), Some(answer), "{error:?}");
        }
    }

    /// Reaches each URI as one stand-in document.
    struct StandInReach(Document);

    impl DocumentReach for StandInReach {
        type Documents = DocumentsStandIn;

        fn reach(&self, _uri: &str) -> Option<DocumentsStandIn> {
            Some(DocumentsStandIn::new(
                vec![self.0.clone()],
                &Steps::default(),
            ))
        }
    }

    #[test]
    fn a_grant_resolves_on_a_worker_into_the_documents_contents() {
        let owner = TestOwner::new();
        let ctx = owner.ctx(None);
        let grants = Grants::new(
            AndroidGrants::new(
                Arc::new(StandInReach(kept("notes.txt", b"notes"))),
                std::rc::Rc::default(),
            ),
            crate::sandbox::android_app(),
            &ctx,
        );
        let grant = grant_of("content://com.example.documents/document/7".to_owned())
            .expect("a URI fits a grant");
        let access = owner
            .settle(grants.resolve(&grant, 1024))
            .expect("not abandoned")
            .expect("the permission is held");
        assert_eq!(access.file().contents(), Some(&b"notes"[..]));
    }
}
