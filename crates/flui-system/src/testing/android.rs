//! The Android providers' test kit: a host's side of the Activity that
//! takes requests and answers them as the test says, and a picker whose
//! documents live in the test, so the dialogs' whole path runs on every
//! host.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use flui_platform::capability::{
    ActivityBridge, ActivityResult, CapabilityId, CompletionSource, DataRefused, IntentFrame,
    JniFrame, MainThreadRefusal, MainThreadRefused, MainThreadWork, RequestKey, Resolver,
    ResultCode, ResultIntent, ResultIntentHold, ResultKey, ResultMint, ResultOutcome,
    ResultRefused, ResultRequest, ResultTicketHold, ResultToken, ResultTokenMint,
};

use crate::android::documents::{
    Described, DocumentPicker, Interrupt, LetGo, PickedDocuments, PickerIntent, read_bounded,
};
use crate::dialog::DialogError;

pub(crate) use super::owner::TestOwner;
pub(crate) use super::window::window;

/// A frame that lends nothing: the stand-ins read no pointer of it.
fn empty_frame() -> JniFrame {
    let nothing = NonNull::<c_void>::dangling();
    JniFrame::new(nothing, nothing, nothing)
}

/// One request the stand-in Activity took.
pub(crate) struct Taken {
    /// The key it waits under.
    pub(crate) key: RequestKey,
    /// Whether the provider filled its `Intent`.
    pub(crate) filled: bool,
    token: ResultToken,
    answer: Option<Resolver<ResultOutcome>>,
    ticket: ResultTicketHold,
}

/// An answer to a request of an earlier Activity, waiting to be claimed.
struct Earlier {
    key: ResultKey,
    code: ResultCode,
    carries_intent: bool,
}

/// A host's side of the Activity: it takes every request unless told to
/// refuse the next, and answers each as the test says.
#[derive(Default)]
pub(crate) struct ActivityStandIn {
    tokens: ResultTokenMint,
    mint: RefCell<Option<ResultMint>>,
    taken: RefCell<Vec<Taken>>,
    refuse_next: Cell<Option<ResultRefused>>,
    earlier: RefCell<VecDeque<Earlier>>,
    intents: RefCell<Vec<ResultIntentHold>>,
    /// How many answer `Intent`s a provider read.
    intents_read: Cell<usize>,
    /// How many times a provider reattached, answered or not.
    reattachments: Cell<usize>,
}

impl ActivityStandIn {
    fn mint(&self) -> ResultMint {
        self.mint
            .borrow()
            .clone()
            .expect("the owner offered the Activity before a provider reached it")
    }

    fn result(&self, token: ResultToken, code: ResultCode, carries_intent: bool) -> ActivityResult {
        let data = carries_intent.then(|| {
            let (data, hold) = self.mint().data(token, Rc::new(|| {}));
            self.intents.borrow_mut().push(hold);
            data
        });
        self.mint().result(code, data)
    }

    /// Refuse the next launch with `refused`.
    pub(crate) fn refuse_next(&self, refused: ResultRefused) {
        self.refuse_next.set(Some(refused));
    }

    /// An answer `code` to a request an earlier Activity took under `key`
    /// waits to be claimed.
    pub(crate) fn earlier_answer(&self, key: ResultKey, code: ResultCode, carries_intent: bool) {
        self.earlier.borrow_mut().push_back(Earlier {
            key,
            code,
            carries_intent,
        });
    }

    /// How many requests were taken.
    pub(crate) fn taken(&self) -> usize {
        self.taken.borrow().len()
    }

    /// The key and whether its `Intent` was filled, of request `index`.
    pub(crate) fn request(&self, index: usize) -> (RequestKey, bool) {
        let taken = self.taken.borrow();
        (taken[index].key, taken[index].filled)
    }

    /// Whether the provider let request `index`'s ticket go, which cancels
    /// it.
    pub(crate) fn is_cancelled(&self, index: usize) -> bool {
        self.taken.borrow()[index].ticket.is_released()
    }

    /// The started activity of request `index` answers `code`, with an
    /// `Intent` when `carries_intent`.
    pub(crate) fn answer(&self, index: usize, code: ResultCode, carries_intent: bool) {
        let (answer, token) = {
            let mut taken = self.taken.borrow_mut();
            let answer = taken[index].answer.take().expect("answered once");
            (answer, taken[index].token)
        };
        let outcome = Ok(self.result(token, code, carries_intent));
        assert!(answer.resolve(outcome).is_ok(), "the owner runs");
    }

    /// Request `index` ends without an answer, as `refused` says.
    pub(crate) fn end(&self, index: usize, refused: ResultRefused) {
        let answer = self.taken.borrow_mut()[index]
            .answer
            .take()
            .expect("answered once");
        assert!(answer.resolve(Err(refused)).is_ok(), "the owner runs");
    }

    /// How many answer `Intent`s a provider read.
    pub(crate) fn intents_read(&self) -> usize {
        self.intents_read.get()
    }

    /// How many answer `Intent`s the provider still holds unread.
    pub(crate) fn intents_held(&self) -> usize {
        self.intents
            .borrow()
            .iter()
            .filter(|hold| !hold.is_released())
            .count()
    }

    /// How many times a provider reattached.
    pub(crate) fn reattachments(&self) -> usize {
        self.reattachments.get()
    }
}

impl ActivityBridge for ActivityStandIn {
    fn bind(&self, mint: ResultMint) {
        self.mint.replace(Some(mint));
    }

    fn launch_for_result(
        &self,
        completions: &CompletionSource,
        _capability: &CapabilityId,
        key: RequestKey,
        intent: &mut dyn FnMut(IntentFrame) -> bool,
    ) -> Result<ResultRequest, ResultRefused> {
        if let Some(refused) = self.refuse_next.take() {
            return Err(refused);
        }
        let token = self
            .tokens
            .next()
            .map_err(|_| ResultRefused::TokensExhausted)?;
        let filled = intent(IntentFrame::new(empty_frame(), NonNull::dangling()));
        if !filled {
            return Err(ResultRefused::IntentNotMade);
        }
        let (completion, answer) = completions.pending();
        let (ticket, hold) = self.mint().ticket(token, Rc::new(|| {}));
        self.taken.borrow_mut().push(Taken {
            key,
            filled,
            token,
            answer: Some(answer),
            ticket: hold,
        });
        Ok(self.mint().request(ticket, completion))
    }

    fn reattach(
        &self,
        completions: &CompletionSource,
        _capability: &CapabilityId,
        key: RequestKey,
    ) -> Result<ResultRequest, ResultRefused> {
        self.reattachments.set(self.reattachments.get() + 1);
        let waiting = {
            let mut earlier = self.earlier.borrow_mut();
            let at = earlier
                .iter()
                .position(|answer| answer.key == key.key())
                .ok_or(ResultRefused::NothingPending)?;
            earlier
                .remove(at)
                .expect("BUG: the position was just found")
        };
        let token = self
            .tokens
            .next()
            .map_err(|_| ResultRefused::TokensExhausted)?;
        let (completion, answer) = completions.pending();
        let (ticket, _hold) = self.mint().ticket(token, Rc::new(|| {}));
        let outcome = Ok(self.result(token, waiting.code, waiting.carries_intent));
        assert!(answer.resolve(outcome).is_ok(), "the owner runs");
        Ok(self.mint().request(ticket, completion))
    }

    fn take_data(
        &self,
        _capability: &CapabilityId,
        data: ResultIntent,
        f: &mut dyn FnMut(JniFrame),
    ) -> Result<(), DataRefused> {
        self.intents_read.set(self.intents_read.get() + 1);
        f(empty_frame());
        drop(data);
        Ok(())
    }

    fn lend(&self, _f: &mut dyn FnMut(JniFrame)) -> bool {
        false
    }

    /// The stand-in has no Java main thread: work is refused as a hand-off
    /// that failed, and never runs.
    fn post(&self, work: MainThreadWork, refused: MainThreadRefusal) {
        drop(work);
        refused(MainThreadRefused::HandOffFailed);
    }
}

/// One document of a stand-in picker's answer.
#[derive(Clone, Debug, Default)]
pub(crate) struct Document {
    /// What its provider says of it.
    pub(crate) described: Described,
    /// What it holds.
    pub(crate) contents: Vec<u8>,
    /// What a write to it answers instead of writing.
    pub(crate) write_error: Option<DialogError>,
}

impl Document {
    /// A document named `name` of `mime_type` holding `contents`, its size
    /// declared truthfully.
    pub(crate) fn new(name: &str, mime_type: &str, contents: &[u8]) -> Self {
        Self {
            described: Described {
                name: Some(name.to_owned()),
                mime_type: Some(mime_type.to_owned()),
                size: Some(u64::try_from(contents.len()).expect("a test's document is small")),
            },
            contents: contents.to_vec(),
            write_error: None,
        }
    }
}

/// What a worker did with a stand-in answer's documents, and what the owner
/// did to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// It asked the provider about document `0`.
    Described(usize),
    /// It read at most `1` bytes of document `0`.
    Read(usize, u64),
    /// It wrote `1` to document `0`.
    Wrote(usize, Vec<u8>),
    /// It deleted document `0`.
    Discarded(usize),
    /// The owner interrupted it.
    Interrupted,
}

/// The steps a stand-in answer's documents went through, in order.
#[derive(Clone, Default)]
pub(crate) struct Steps(Arc<Mutex<Vec<Step>>>);

impl Steps {
    fn push(&self, step: Step) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(step);
    }

    /// The steps so far.
    pub(crate) fn taken(&self) -> Vec<Step> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// The documents of one stand-in answer, as a worker reaches them.
pub(crate) struct DocumentsStandIn {
    documents: Vec<Document>,
    steps: Steps,
    /// When set, a read waits here until the test lets it go on.
    reads: Option<Receiver<()>>,
}

impl DocumentsStandIn {
    /// `documents`, whose worker's steps are recorded in `steps`.
    pub(crate) fn new(documents: Vec<Document>, steps: &Steps) -> Self {
        Self {
            documents,
            steps: steps.clone(),
            reads: None,
        }
    }
}

impl PickedDocuments for DocumentsStandIn {
    fn len(&self) -> usize {
        self.documents.len()
    }

    fn describe(&mut self, index: usize) -> Described {
        self.steps.push(Step::Described(index));
        self.documents[index].described.clone()
    }

    fn read(&mut self, index: usize, at_most: u64, let_go: &LetGo) -> Result<Vec<u8>, DialogError> {
        self.steps.push(Step::Read(index, at_most));
        if let Some(reads) = &self.reads {
            // A dropped sender lets the read go on as well.
            let _ = reads.recv();
        }
        read_bounded(self.documents[index].contents.as_slice(), at_most, let_go)
    }

    fn write(&mut self, index: usize, contents: &[u8]) -> Result<(), DialogError> {
        if let Some(error) = self.documents[index].write_error {
            return Err(error);
        }
        self.steps.push(Step::Wrote(index, contents.to_vec()));
        contents.clone_into(&mut self.documents[index].contents);
        Ok(())
    }

    fn discard(&mut self, index: usize) {
        self.steps.push(Step::Discarded(index));
    }
}

/// A picker whose answers the test writes: the types it maps extensions to,
/// the `Intent`s it filled, and the documents its next answer names.
#[derive(Default)]
pub(crate) struct PickerStandIn {
    /// The extension to MIME type pairs it knows.
    pub(crate) types: Vec<(&'static str, &'static str)>,
    /// The `Intent`s it was asked to fill, in order.
    pub(crate) filled: RefCell<Vec<PickerIntent>>,
    /// What its next answer names; `None` answers more than it reads.
    pub(crate) answer: RefCell<Option<Vec<Document>>>,
    /// What happened to its answers' documents.
    pub(crate) steps: Steps,
    /// The gate of the next answer's reads, when the test holds them.
    hold_reads: RefCell<Option<Receiver<()>>>,
}

impl PickerStandIn {
    /// A picker that maps `types`.
    pub(crate) fn mapping(types: &[(&'static str, &'static str)]) -> Self {
        Self {
            types: types.to_vec(),
            ..Self::default()
        }
    }

    /// The next answer names `documents`.
    pub(crate) fn will_answer(&self, documents: Vec<Document>) {
        self.answer.replace(Some(documents));
    }

    /// Hold the reads of the next answer's documents until the returned
    /// sender sends, or drops.
    pub(crate) fn hold_reads(&self) -> Sender<()> {
        let (release, reads) = channel();
        self.hold_reads.replace(Some(reads));
        release
    }
}

impl DocumentPicker for PickerStandIn {
    type Documents = DocumentsStandIn;

    fn mime_type_of(&self, extension: &str) -> Option<String> {
        self.types
            .iter()
            .find(|(known, _)| *known == extension)
            .map(|(_, mime_type)| (*mime_type).to_owned())
    }

    fn fill(&self, _frame: IntentFrame, intent: &PickerIntent) -> bool {
        self.filled.borrow_mut().push(intent.clone());
        true
    }

    fn picked(
        &self,
        _frame: JniFrame,
        max: usize,
    ) -> Result<(DocumentsStandIn, Interrupt), DialogError> {
        let documents = self
            .answer
            .take()
            .filter(|documents| documents.len() <= max)
            .ok_or(DialogError::Failed(
                flui_platform::SystemFailure::MalformedAnswer,
            ))?;
        let steps = self.steps.clone();
        let mut documents = DocumentsStandIn::new(documents, &self.steps);
        documents.reads = self.hold_reads.take();
        Ok((documents, Box::new(move || steps.push(Step::Interrupted))))
    }
}
