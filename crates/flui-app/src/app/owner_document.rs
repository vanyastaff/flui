//! Explicit, versioned document exchange between owners. No reactive graph or
//! owner-local object is shared. Reads and conditional replacements are served
//! by the initial owner; native input and frame work never use this lane.
use super::application_control::{Ingress, OwnerId, OwnerRoute, contain};
use flui_foundation::{ClaimHandle, ClaimOutcome, ClaimSlot, IdMint, claim_slot};
use parking_lot::Mutex;
use std::{
    future::Future,
    num::NonZeroU64,
    pin::Pin,
    sync::{Arc, Weak},
    task::{Context, Poll},
};

/// A revision issued by one document; it cannot wrap or be manufactured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentRevision {
    document: DocumentIdentity,
    revision: NonZeroU64,
}

#[derive(Clone, Debug)]
struct DocumentIdentity(Arc<()>);
impl PartialEq for DocumentIdentity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for DocumentIdentity {}

/// An immutable document value read at a particular revision. Keeping this
/// snapshot does not authorize replacing a newer value.
#[derive(Clone, Debug)]
pub struct DocumentSnapshot {
    revision: DocumentRevision,
    bytes: Arc<[u8]>,
}
impl DocumentSnapshot {
    /// The exact version required for a conditional replacement.
    pub fn revision(&self) -> DocumentRevision {
        self.revision.clone()
    }
    /// The application-defined document encoding.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A document's application identity and service. Capture this in a dedicated
/// factory and bind it on the destination; its bytes are never a shared signal.
#[derive(Clone)]
pub struct SharedDocument {
    pub(crate) data: Arc<DocumentData>,
    pub(crate) run: Weak<Ingress>,
}
impl std::fmt::Debug for SharedDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedDocument")
            .field("id", &self.data.id)
            .finish_non_exhaustive()
    }
}
impl SharedDocument {
    /// Bind messages to the calling registered owner. A retained port refuses
    /// once that incarnation retires, even if another owner reuses its thread.
    pub fn bind(&self) -> Result<DocumentPort, DocumentRefusal> {
        let route = super::runner::current_owner_route().ok_or(DocumentRefusal::OwnerGone)?;
        let run = self.run.upgrade().ok_or(DocumentRefusal::OwnerGone)?;
        if !Arc::ptr_eq(&run, &route.ingress) {
            return Err(DocumentRefusal::WrongRun);
        }
        Ok(DocumentPort {
            document: self.clone(),
            route,
        })
    }
}

/// A fixed source owner's bounded document port. It may be used by background
/// tasks, but never retargets a new owner or application run.
#[derive(Clone)]
pub struct DocumentPort {
    document: SharedDocument,
    route: OwnerRoute,
}
impl std::fmt::Debug for DocumentPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentPort")
            .field("document", &self.document)
            .finish_non_exhaustive()
    }
}
impl DocumentPort {
    /// Request a snapshot. A receipt is not evidence the initial owner has
    /// executed the read; await it or call `try_result`.
    pub fn read(&self) -> Result<DocumentRequest, DocumentSendError> {
        self.route
            .submit_document(Arc::clone(&self.document.data), Edit::Read)
    }
    /// Replace exactly `expected`. A stale answer cannot overwrite a later
    /// edit: conflict returns both the current snapshot and these rejected bytes.
    pub fn replace(
        &self,
        expected: DocumentRevision,
        bytes: Vec<u8>,
    ) -> Result<DocumentRequest, DocumentSendError> {
        self.route.submit_document(
            Arc::clone(&self.document.data),
            Edit::Replace { expected, bytes },
        )
    }
}

/// Typed refusal before admission, or cancellation of accepted work at retirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DocumentRefusal {
    /// The exact source owner or application is gone.
    #[error("document source owner is gone")]
    OwnerGone,
    /// The document belongs to a different application incarnation.
    #[error("document belongs to another application run")]
    WrongRun,
    /// Entry or retained-byte budget is exhausted.
    #[error("document lane is full")]
    Full,
    /// The allocation itself exceeds the payload budget.
    #[error("document payload is too large")]
    Oversize,
    /// The never-reused identity space is exhausted.
    #[error("document identity space is exhausted")]
    IdentityExhausted,
}

/// A refused message, including its input allocation for retry or recovery.
#[derive(Debug, thiserror::Error)]
#[error("{reason}")]
pub struct DocumentSendError {
    /// Why no work was accepted.
    pub reason: DocumentRefusal,
    /// Replacement/create bytes, returned without copying; absent for a read.
    pub rejected: Option<Vec<u8>>,
}

/// The initial owner's answer. A conflict never modifies the current document.
#[derive(Debug)]
pub enum DocumentOutcome {
    /// Snapshot returned by a read or a successful conditional replacement.
    Current(DocumentSnapshot),
    /// A newer value won. The caller still owns its edit and decides how to rebase.
    Conflict {
        /// The value that superseded the expected revision.
        current: DocumentSnapshot,
        /// The caller's unmodified edit for an explicit rebase.
        rejected: Vec<u8>,
    },
    /// Accepted transport could not be executed, with its edit preserved.
    Refused(DocumentSendError),
}

/// One reserved completion. Dropping it abandons observation, not the command.
#[must_use = "observe the document operation's outcome"]
pub struct DocumentRequest {
    reply: ClaimHandle<Completed>,
}
impl std::fmt::Debug for DocumentRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentRequest").finish_non_exhaustive()
    }
}
impl DocumentRequest {
    /// Take the answer without waiting on any owner thread.
    pub fn try_result(&mut self) -> Option<DocumentOutcome> {
        let mut cx = Context::from_waker(std::task::Waker::noop());
        match Pin::new(self).poll(&mut cx) {
            Poll::Ready(outcome) => Some(outcome),
            Poll::Pending => None,
        }
    }
}
impl Future for DocumentRequest {
    type Output = DocumentOutcome;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.reply).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(ClaimOutcome::Delivered(completed)) => Poll::Ready(completed.outcome),
            Poll::Ready(ClaimOutcome::OwnerGone | ClaimOutcome::AlreadyClaimed) => {
                Poll::Ready(DocumentOutcome::Refused(DocumentSendError {
                    reason: DocumentRefusal::OwnerGone,
                    rejected: None,
                }))
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct DocumentData {
    id: NonZeroU64,
    pub(crate) max_bytes: usize,
    state: Mutex<DocumentState>,
}
#[derive(Debug)]
struct DocumentState {
    revisions: IdMint<NonZeroU64>,
    snapshot: DocumentSnapshot,
}
impl DocumentData {
    pub(crate) fn new(id: NonZeroU64, bytes: Vec<u8>, max_bytes: usize) -> Self {
        Self {
            id,
            max_bytes,
            state: Mutex::new(DocumentState {
                revisions: IdMint::starting_after(1),
                snapshot: DocumentSnapshot {
                    revision: DocumentRevision {
                        document: DocumentIdentity(Arc::new(())),
                        revision: NonZeroU64::MIN,
                    },
                    bytes: Arc::from(bytes),
                },
            }),
        }
    }
}

#[derive(Debug)]
pub(crate) enum Edit {
    Read,
    Replace {
        expected: DocumentRevision,
        bytes: Vec<u8>,
    },
}
impl Edit {
    pub(crate) fn retained_bytes(&self, maximum: usize) -> Result<usize, DocumentRefusal> {
        match self {
            Self::Read => Ok(maximum),
            Self::Replace { bytes, .. } if bytes.capacity() > maximum => {
                Err(DocumentRefusal::Oversize)
            }
            Self::Replace { bytes, .. } => maximum
                .checked_add(bytes.capacity())
                .ok_or(DocumentRefusal::Oversize),
        }
    }
    pub(crate) fn rejected(self, reason: DocumentRefusal) -> DocumentSendError {
        DocumentSendError {
            reason,
            rejected: match self {
                Self::Read => None,
                Self::Replace { bytes, .. } => Some(bytes),
            },
        }
    }
}

struct Completed {
    outcome: DocumentOutcome,
    _budget: Budget,
}
struct Budget {
    run: Weak<Ingress>,
    owner: OwnerId,
    bytes: usize,
}
impl Drop for Budget {
    fn drop(&mut self) {
        if let Some(run) = self.run.upgrade() {
            run.release_document_budget(self.owner, self.bytes);
        }
    }
}

/// Closed vocabulary, one sequence and completion per accepted command. The
/// budget stays charged while the completion retains the payload unobserved.
pub(crate) struct Envelope {
    sequence: NonZeroU64,
    data: Arc<DocumentData>,
    edit: Edit,
    reply: ClaimSlot<Completed>,
    budget: Budget,
}
impl std::fmt::Debug for Envelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentCommand")
            .field("sequence", &self.sequence)
            .field("document", &self.data.id)
            .finish_non_exhaustive()
    }
}
impl Envelope {
    pub(crate) fn new(
        sequence: NonZeroU64,
        data: Arc<DocumentData>,
        edit: Edit,
        run: Weak<Ingress>,
        owner: OwnerId,
        bytes: usize,
    ) -> (Self, DocumentRequest) {
        let budget = Budget { run, owner, bytes };
        let (reply, receiver) = claim_slot(Arc::new(|| {}));
        (
            Self {
                sequence,
                data,
                edit,
                reply,
                budget,
            },
            DocumentRequest { reply: receiver },
        )
    }
    pub(crate) fn execute(self, accepting: bool) {
        let Self {
            data,
            edit,
            reply,
            budget,
            ..
        } = self;
        let outcome = if accepting {
            let mut state = data.state.lock();
            match edit {
                Edit::Read => DocumentOutcome::Current(state.snapshot.clone()),
                Edit::Replace { expected, bytes } if expected != state.snapshot.revision => {
                    DocumentOutcome::Conflict {
                        current: state.snapshot.clone(),
                        rejected: bytes,
                    }
                }
                Edit::Replace { bytes, .. } => match state.revisions.next() {
                    Ok(revision) => {
                        state.snapshot = DocumentSnapshot {
                            revision: DocumentRevision {
                                document: state.snapshot.revision.document.clone(),
                                revision,
                            },
                            bytes: Arc::from(bytes),
                        };
                        DocumentOutcome::Current(state.snapshot.clone())
                    }
                    Err(_) => DocumentOutcome::Refused(DocumentSendError {
                        reason: DocumentRefusal::IdentityExhausted,
                        rejected: Some(bytes),
                    }),
                },
            }
        } else {
            DocumentOutcome::Refused(edit.rejected(DocumentRefusal::OwnerGone))
        };
        // The document lock is gone before a receiver's arbitrary waker runs.
        contain(|| {
            let _ = reply.deliver(Completed {
                outcome,
                _budget: budget,
            });
        });
        contain(|| drop(reply));
    }
}
