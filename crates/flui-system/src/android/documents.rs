//! The file dialogs as the Storage Access Framework's pickers:
//! `ACTION_OPEN_DOCUMENT` and `ACTION_CREATE_DOCUMENT`, started through the
//! host's Activity for a result.
//!
//! A picker answers content URIs, never paths, so a pick is handed over as
//! its name and its contents ([`PickedFile::try_from_contents`]), read
//! through the URI on a worker: every document is described first (name,
//! type, declared size), the picks are held to the request's filters and
//! their declared sizes to its `max_bytes` before a byte is read, and the
//! bytes read are counted again. A save carries its contents and is written
//! through the URI the user chose; a save without them, which could only
//! answer a path, is refused, and so is a folder dialog, whose tree URI no
//! pick can carry yet.
//!
//! The picker filters by MIME type alone. A filter's MIME types pass as
//! they are, and each extension as the type Android maps its last part to;
//! when Android maps an extension to none, a filter admits every file or
//! names a file exactly (ADR-0374), or the types are more than an `Intent`
//! should carry, the picker shows every document, and the picks are held to
//! the filters all the same (ADR-0360 §4). The picker shows no title, accept
//! label, starting folder or hidden files of the request's.
//!
//! The request lasts by its [`ResultTicket`]. Dropping an open dialog's hold
//! before the picker answers drops the ticket, which finishes the picker
//! (ADR-0344). Dropped after the answer arrived and before its owner turn,
//! it leaves the answer unread: no worker starts. Dropped while a worker
//! reads, it stops three things: the provider's calls that take the
//! cancellation signal (the query of a name or a size, the open of a
//! document, whose `OperationCanceledException` ends the read), and the
//! worker itself before its next document and between two
//! [`READ_CHUNK`]-sized reads ([`LetGo`]). A read the provider holds blocked
//! inside one chunk keeps the worker's place until the provider writes or
//! closes its end. A save that the user has answered completes whoever still
//! waits. When Android replaces the
//! Activity, the request ends as the window closing; the answer that comes
//! later reaches the next Activity, where [`EarlierAnswers`] claims it so
//! the host holds nothing: an open's answer is let go, and the empty
//! document a save created is deleted, since the contents went with the
//! runtime that asked.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_platform::capability::{
    Activity, CompletionSource, IntentFrame, JniFrame, MAX_OPEN_REQUESTS_PER_CAPABILITY,
    ProviderContext, Resolver, ResultCode, ResultKey, ResultRefused, ResultTicket, WindowRef,
};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use super::workers::{NotRun, Workers};
use crate::dialog::handed_over::{
    admit_declared, admit_picks, admit_read, named_after_first_filter,
};
use crate::dialog::{
    DialogError, DialogHold, FileDialogBackend, FileDialogCapability, FileDialogs, FileFilter,
    OpenRequest, OpenResult, PickedFile, Picks, SaveLocation, SaveRequest, SaveResult,
};

/// The key an open dialog's request waits under across Activities.
pub(crate) const OPEN_KEY: ResultKey = key("flui-system.open-document");

/// The key a save dialog's request waits under across Activities.
pub(crate) const SAVE_KEY: ResultKey = key("flui-system.create-document");

const fn key(name: &'static str) -> ResultKey {
    match ResultKey::try_new(name) {
        Ok(key) => key,
        Err(_) => panic!("BUG: a request key is short printable ASCII"),
    }
}

/// The MIME type that is every type.
const ANY_TYPE: &str = "*/*";

/// The type of a document no pattern gives one.
const UNTYPED: &str = "application/octet-stream";

/// The most MIME types a picker's `Intent` carries: past it the picker
/// shows every document, since an `Intent` crosses processes in one bounded
/// transaction.
pub(crate) const MAX_INTENT_TYPES: usize = 64;

/// The most documents one answer is read for; an answer with more is not
/// read at all.
pub(crate) const MAX_PICKS: usize = 1024;

/// The longest display name or MIME type a provider is asked to hand over,
/// in UTF-16 units; a longer one is not copied.
#[cfg(target_os = "android")]
pub(crate) const MAX_NAME_UNITS: usize = 1024;

/// Why `ContentResolver.openFileDescriptor` handed over no descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotOpened {
    /// The grant to the document has ended (`SecurityException`).
    GrantEnded,
    /// The document does not exist any more (`FileNotFoundException`).
    NotFound,
    /// The library cancelled the open (`OperationCanceledException`).
    Cancelled,
    /// The provider refused or failed otherwise.
    Failed,
}

/// `EACCES`, on Linux and so on Android.
const EACCES: i32 = 13;
/// `ENOENT`, on Linux and so on Android.
const ENOENT: i32 = 2;

/// What a document that was not opened answers its dialog: a grant that
/// ended is the system denying access, a document that is gone is not
/// found, and a cancelled open belongs to a dialog whose window nobody
/// waits for any more.
pub(crate) fn open_refusal(reason: NotOpened) -> DialogError {
    match reason {
        NotOpened::GrantEnded => DialogError::Failed(SystemFailure::Os(EACCES)),
        NotOpened::NotFound => DialogError::Failed(SystemFailure::Os(ENOENT)),
        NotOpened::Cancelled => DialogError::WindowClosed,
        NotOpened::Failed => DialogError::Failed(SystemFailure::MalformedAnswer),
    }
}

/// The failure a read or a write of a document reports: the system's code,
/// or an answer outside its contract when it gives none.
pub(crate) fn io_failure(error: &std::io::Error) -> DialogError {
    DialogError::Failed(
        error
            .raw_os_error()
            .map_or(SystemFailure::MalformedAnswer, SystemFailure::Os),
    )
}

/// What a picker's `Intent` is filled as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PickerIntent {
    /// `ACTION_OPEN_DOCUMENT` for openable documents of `mime_types`, one
    /// at least.
    Open {
        /// The types shown; [`ANY_TYPE`] alone for every document.
        mime_types: Vec<String>,
        /// `EXTRA_ALLOW_MULTIPLE`.
        multiple: bool,
    },
    /// `ACTION_CREATE_DOCUMENT` of `mime_type`.
    Create {
        /// The new document's type.
        mime_type: String,
        /// `EXTRA_TITLE`: the name proposed.
        name: Option<String>,
    },
}

/// The MIME types an open picker shows for `filters`, where `type_of`
/// answers the type Android maps an extension's last part to: every
/// filter's MIME types and the types of its extensions, each once, or
/// [`ANY_TYPE`] alone when there is no filter, an extension has no type, or
/// they are more than [`MAX_INTENT_TYPES`].
pub(crate) fn open_types(
    filters: &[FileFilter],
    type_of: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let every = || vec![ANY_TYPE.to_owned()];
    let mut types: Vec<String> = Vec::new();
    for filter in filters {
        // A filter of every file, or one that names a file exactly (which
        // has no type to show it by), shows every document (ADR-0374).
        if filter.is_all_files() || !filter.file_names().is_empty() {
            return every();
        }
        let of_extensions = filter.extensions().iter().map(|extension| {
            let last_part = extension.rsplit('.').next().unwrap_or(extension);
            type_of(last_part)
        });
        let stated = filter.mime_types().iter().cloned().map(Some);
        for mime_type in stated.chain(of_extensions) {
            let Some(mime_type) = mime_type else {
                return every();
            };
            if !types.contains(&mime_type) {
                if types.len() == MAX_INTENT_TYPES {
                    return every();
                }
                types.push(mime_type);
            }
        }
    }
    if types.is_empty() { every() } else { types }
}

/// The name a save proposes and the type of the document it creates, where
/// `type_of` answers the type Android maps an extension's last part to: the
/// suggested name with the first filter's extension when it has none, and
/// the type of the name's extension, else of the first filter's first
/// extension, else the first filter's first concrete MIME type, else
/// [`UNTYPED`].
///
/// The type is how Android names a typed file after the selected filter: a
/// provider that keeps files (Android's own) adds the extension of the
/// document's type to a name the user typed without it. Taken from the
/// proposed name's extension first, it never makes the provider add a
/// second extension to that name. A type Android has no extension for
/// ([`UNTYPED`]) leaves the name as the user typed it.
pub(crate) fn create_intent(
    suggested: Option<&str>,
    filters: &[FileFilter],
    type_of: impl Fn(&str) -> Option<String>,
) -> PickerIntent {
    let name = suggested.map(|name| named_after_first_filter(name, filters));
    let first = filters.first();
    let of_name = name
        .as_deref()
        .and_then(|name| name.rsplit_once('.'))
        .filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
        .and_then(|(_, extension)| type_of(&extension.to_ascii_lowercase()));
    let of_extension = || {
        let extension = first?.extensions().first()?;
        type_of(extension.rsplit('.').next().unwrap_or(extension))
    };
    let of_stated_type = || {
        first?
            .mime_types()
            .iter()
            .find(|mime_type| !mime_type.contains('*'))
            .cloned()
    };
    PickerIntent::Create {
        mime_type: of_name
            .or_else(of_extension)
            .or_else(of_stated_type)
            .unwrap_or_else(|| UNTYPED.to_owned()),
        name,
    }
}

/// What a document's provider says of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Described {
    /// The name it shows (`OpenableColumns.DISPLAY_NAME`), when it shows
    /// one within the length the library copies.
    pub(crate) name: Option<String>,
    /// Its MIME type (`ContentResolver.getType`), when the provider states
    /// one.
    pub(crate) mime_type: Option<String>,
    /// Its size in bytes (`OpenableColumns.SIZE`), when the provider states
    /// one.
    pub(crate) size: Option<u64>,
}

/// Stops the calls a worker makes for a request nobody waits for any more:
/// run once, from the owner thread, and never waits for the provider.
pub(crate) type Interrupt = Box<dyn FnOnce()>;

/// Set on the owner thread when an open dialog's hold is dropped; the worker
/// reading the dialog's answer checks it before each document and between
/// chunks of one, and stops.
#[derive(Clone, Debug, Default)]
pub(crate) struct LetGo(Arc<AtomicBool>);

impl LetGo {
    /// Nobody waits for the answer any more.
    pub(crate) fn set(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether nobody waits for the answer any more.
    pub(crate) fn is_set(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// The most bytes one read of a document asks for, between two checks of
/// its dialog's [`LetGo`].
pub(crate) const READ_CHUNK: usize = 64 * 1024;

/// The first `at_most` bytes `reader` holds, or all of them when it holds
/// fewer, read [`READ_CHUNK`] at a time with `let_go` checked before each.
///
/// # Errors
///
/// [`DialogError::WindowClosed`] once `let_go` is set: nobody hears the
/// answer, and the rest is not read. The system's failure of a read.
pub(crate) fn read_bounded(
    mut reader: impl std::io::Read,
    at_most: u64,
    let_go: &LetGo,
) -> Result<Vec<u8>, DialogError> {
    let mut contents = Vec::new();
    let mut chunk = vec![0_u8; READ_CHUNK];
    loop {
        let left = at_most.saturating_sub(u64::try_from(contents.len()).unwrap_or(u64::MAX));
        if left == 0 {
            return Ok(contents);
        }
        if let_go.is_set() {
            return Err(DialogError::WindowClosed);
        }
        let wanted = usize::try_from(left).unwrap_or(usize::MAX).min(READ_CHUNK);
        match reader.read(&mut chunk[..wanted]) {
            Ok(0) => return Ok(contents),
            Ok(read) => contents.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(io_failure(&error)),
        }
    }
}

/// The documents one answer of a picker names, as a worker reaches them:
/// the content URIs and the resolver they are read through on Android, a
/// stand-in in the tests. Every call may block for as long as the
/// document's provider takes.
pub(crate) trait PickedDocuments: Send + 'static {
    /// How many documents the picker answered.
    fn len(&self) -> usize;

    /// What the provider says of document `index`.
    fn describe(&mut self, index: usize) -> Described;

    /// The first `at_most` bytes of document `index`, or all of it when it
    /// holds fewer, read by [`read_bounded`] under `let_go`.
    ///
    /// # Errors
    ///
    /// Why the document could not be read, or `WindowClosed` once `let_go`
    /// is set.
    fn read(&mut self, index: usize, at_most: u64, let_go: &LetGo) -> Result<Vec<u8>, DialogError>;

    /// Replace what document `index` holds with `contents`.
    ///
    /// # Errors
    ///
    /// Why the document could not be written.
    fn write(&mut self, index: usize, contents: &[u8]) -> Result<(), DialogError>;

    /// Delete document `index`, which a save created and did not write.
    fn discard(&mut self, index: usize);

    /// Persist the application's read permission to document `index`, so
    /// that it outlasts the process: its URI, read and bounded before the
    /// permission is taken, or `None` when the URI is too long to keep or
    /// its provider offers no persistable permission, which leaves nothing
    /// persisted.
    fn keep(&mut self, index: usize) -> Option<String>;

    /// Release the persisted read permission of document `index`, which a
    /// pick could not carry after all.
    fn release(&mut self, index: usize);

    /// Whether the application holds a persisted read permission to
    /// document `index`.
    fn is_kept(&mut self, index: usize) -> bool;
}

/// The answer of an open dialog whose picker answered `documents`: each is
/// described, the picks are held to `filters` and to `max_bytes` as
/// declared, and only then read, the bytes counted again, until `let_go` is
/// set. A request that `keep_access` has each pick carry its URI as a
/// grant, its permission persisted once every pick was read, so that an
/// answer refused midway persists nothing.
pub(crate) fn read_picks(
    documents: &mut impl PickedDocuments,
    filters: &[FileFilter],
    multiple: bool,
    max_bytes: u64,
    let_go: &LetGo,
    keep_access: bool,
) -> OpenResult {
    let malformed = || DialogError::Failed(SystemFailure::MalformedAnswer);
    let count = documents.len();
    if count == 0 || (count > 1 && !multiple) {
        return Err(malformed());
    }
    let described = (0..count)
        .map(|index| documents.describe(index))
        .collect::<Vec<Described>>();
    let names = described
        .iter()
        .map(|document| document.name.as_deref())
        .collect::<Option<Vec<&str>>>()
        .ok_or_else(malformed)?;
    admit_picks(
        names
            .iter()
            .zip(&described)
            .map(|(name, document)| (*name, document.mime_type.as_deref())),
        filters,
    )?;
    admit_declared(
        described.iter().filter_map(|document| document.size),
        max_bytes,
    )?;
    let mut read = 0_u64;
    let mut picks = Vec::with_capacity(count);
    for (index, name) in names.into_iter().enumerate() {
        // One byte past what is left shows a document larger than declared.
        let left = max_bytes - read;
        let contents = documents.read(index, left.saturating_add(1), let_go)?;
        read = read.saturating_add(u64::try_from(contents.len()).unwrap_or(u64::MAX));
        admit_read(read, max_bytes)?;
        picks.push(
            PickedFile::try_from_contents(name, Arc::<[u8]>::from(contents))
                .ok_or_else(malformed)?,
        );
    }
    if keep_access {
        picks = picks
            .into_iter()
            .enumerate()
            .map(|(index, pick)| {
                let Some(uri) = documents.keep(index) else {
                    return pick;
                };
                if let Some(grant) = super::grants::grant_of(uri) {
                    pick.granted(grant)
                } else {
                    // Persisted, and no grant holds it: given back, so no
                    // permission outlives the answer that cannot name it.
                    documents.release(index);
                    pick
                }
            })
            .collect();
    }
    Ok(Picks::try_new(picks))
}

/// The answer of a save dialog whose picker created `documents`' one
/// document: `contents` are written to it, and it is named as its provider
/// names it, else as the request `proposed`. A document that could not be
/// written is deleted, so a failed save leaves no empty file behind.
pub(crate) fn write_save(
    documents: &mut impl PickedDocuments,
    contents: &Arc<[u8]>,
    proposed: Option<String>,
) -> SaveResult {
    let malformed = || DialogError::Failed(SystemFailure::MalformedAnswer);
    if documents.len() != 1 {
        return Err(malformed());
    }
    if let Err(error) = documents.write(0, contents) {
        documents.discard(0);
        return Err(error);
    }
    [documents.describe(0).name, proposed]
        .into_iter()
        .flatten()
        .find_map(|name| PickedFile::try_from_contents(name, Arc::clone(contents)))
        .map(|file| Some(SaveLocation::new(file, None)))
        .ok_or_else(malformed)
}

/// Android's side of a picker, on the owner thread: the Storage Access
/// Framework through JNI on Android (`JavaPicker`), a stand-in in the tests.
pub(crate) trait DocumentPicker: 'static {
    /// What a worker reads an answer's documents through.
    type Documents: PickedDocuments;

    /// The MIME type Android maps `extension` (one part, lower case, no
    /// dot) to, if it knows one.
    fn mime_type_of(&self, extension: &str) -> Option<String>;

    /// Fill the `Intent` the host lends in `frame` as `intent`; `false`
    /// when it could not be filled.
    fn fill(&self, frame: IntentFrame, intent: &PickerIntent) -> bool;

    /// The documents the answer `Intent` lent in `frame` names, at most
    /// `max`, and what interrupts their worker.
    ///
    /// # Errors
    ///
    /// The answer names more than `max` documents, or could not be read.
    fn picked(
        &self,
        frame: JniFrame,
        max: usize,
    ) -> Result<(Self::Documents, Interrupt), DialogError>;
}

/// The dialog's answer to a request the Activity did not take or answer.
pub(crate) fn request_refusal(refused: ResultRefused) -> DialogError {
    let unsupported =
        |reason| DialogError::Unsupported(Unsupported::of::<FileDialogCapability>(reason));
    match refused {
        // One of the host's requests must end first, or its hand-over to
        // Android failed this once.
        ResultRefused::TooManyOpen
        | ResultRefused::TokensExhausted
        | ResultRefused::IntentNotMade
        | ResultRefused::HandOffFailed => unsupported(UnsupportedReason::Unavailable),
        // No application shows documents: a device without the system's
        // documents interface.
        ResultRefused::NotLaunched => unsupported(UnsupportedReason::NotOnThisDevice),
        // The Activity's window went with it; a cancelled request has
        // nobody left to hear.
        ResultRefused::ActivityEnded | ResultRefused::Cancelled => DialogError::WindowClosed,
        _ => DialogError::Failed(SystemFailure::MalformedAnswer),
    }
}

/// What a worker's refusal answers a dialog.
fn not_run<T>(reason: NotRun) -> Result<T, DialogError> {
    Err(match reason {
        NotRun::Busy => DialogError::Unsupported(Unsupported::of::<FileDialogCapability>(
            UnsupportedReason::Unavailable,
        )),
        NotRun::NoThread(code) => {
            DialogError::Failed(code.map_or(SystemFailure::MalformedAnswer, SystemFailure::Os))
        }
    })
}

/// What a dialog's hold and its request's answer share.
#[derive(Default)]
struct Asked {
    /// The request's claim; dropping it finishes the picker.
    ticket: Cell<Option<ResultTicket>>,
    /// Stops the worker's calls into the provider, once there is an answer.
    interrupt: Cell<Option<Interrupt>>,
    /// The hold was dropped: nobody waits for the answer. Shared with the
    /// worker reading it.
    let_go: LetGo,
}

/// A dialog's hold: its drop finishes the picker, or stops the read of what
/// it answered.
struct PickerHold(Rc<Asked>);

impl Drop for PickerHold {
    fn drop(&mut self) {
        self.0.let_go.set();
        let ticket = self.0.ticket.take();
        let interrupt = self.0.interrupt.take();
        drop(ticket);
        if let Some(interrupt) = interrupt {
            interrupt();
        }
    }
}

/// Whether a worker's read stops when the dialog's caller lets go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterAnswer {
    /// An open: nobody reads the picks of a dialog let go.
    StopsWithCaller,
    /// A save: the user chose the file, so the write completes.
    Completes,
}

/// [`FileDialogBackend`] over the host's Activity and a [`DocumentPicker`].
pub(crate) struct AndroidDialogs<P: DocumentPicker> {
    activity: Activity,
    completions: CompletionSource,
    picker: Rc<P>,
    workers: Rc<Workers>,
}

impl<P: DocumentPicker> AndroidDialogs<P> {
    pub(crate) fn new(
        activity: Activity,
        completions: CompletionSource,
        picker: Rc<P>,
        workers: Rc<Workers>,
    ) -> Self {
        Self {
            activity,
            completions,
            picker,
            workers,
        }
    }

    /// Start the picker `intent` describes under `key` and answer `answer`
    /// with what `work` makes of the documents it answers, on a worker;
    /// `None` when the user backed out.
    fn ask<T: Send + 'static>(
        &self,
        key: ResultKey,
        intent: &PickerIntent,
        after: AfterAnswer,
        answer: Resolver<Result<Option<T>, DialogError>>,
        work: impl FnOnce(&mut P::Documents, &LetGo) -> Result<Option<T>, DialogError> + Send + 'static,
    ) -> DialogHold {
        let picker = Rc::clone(&self.picker);
        let launched = self
            .activity
            .launch_for_result(&self.completions, key, |frame| picker.fill(frame, intent));
        let request = match launched {
            Ok(request) => request,
            Err(refused) => {
                // The owner retiring first leaves nothing waiting.
                let _ = answer.resolve(Err(request_refusal(refused)));
                return DialogHold::new(());
            }
        };
        let (ticket, answered) = request.into_parts();
        let asked = Rc::new(Asked::default());
        asked.ticket.set(Some(ticket));
        let hold = PickerHold(Rc::clone(&asked));
        let activity = self.activity.clone();
        let workers = Rc::clone(&self.workers);
        let mut pending = Some((answer, work));
        answered.on_complete(move |outcome| {
            let Some((answer, work)) = pending.take() else {
                return;
            };
            // The request is answered: its claim holds nothing more.
            drop(asked.ticket.take());
            let documents = match outcome {
                // The host dropped the request: the caller hears the same.
                Err(_) => return,
                Ok(Err(refused)) => Err(request_refusal(refused)),
                Ok(Ok(mut result)) => match (result.code(), result.take_data()) {
                    (ResultCode::Canceled, _) => Ok(None),
                    (ResultCode::Ok, Some(data)) => activity
                        .take_data(data, |frame| picker.picked(frame, MAX_PICKS))
                        .unwrap_or(Err(DialogError::Failed(SystemFailure::MalformedAnswer)))
                        .map(Some),
                    _ => Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
                },
            };
            match documents {
                Ok(Some((mut documents, interrupt))) => {
                    // A save the user answered is written whoever waits.
                    let let_go = match after {
                        AfterAnswer::StopsWithCaller => {
                            if asked.let_go.is_set() {
                                return;
                            }
                            asked.interrupt.set(Some(interrupt));
                            asked.let_go.clone()
                        }
                        AfterAnswer::Completes => LetGo::default(),
                    };
                    workers.run("flui-system-documents", answer, not_run, move || {
                        work(&mut documents, &let_go)
                    });
                }
                Ok(None) => drop(answer.resolve(Ok(None))),
                Err(error) => drop(answer.resolve(Err(error))),
            }
        });
        DialogHold::new(hold)
    }
}

impl<P: DocumentPicker> FileDialogBackend for AndroidDialogs<P> {
    fn open(
        &self,
        _window: &WindowRef,
        request: OpenRequest,
        answer: Resolver<OpenResult>,
    ) -> DialogHold {
        if request.picks_folders() {
            // A picked tree is a URI no pick carries yet.
            let _ = answer.resolve(Err(DialogError::Unsupported(Unsupported::of::<
                FileDialogCapability,
            >(
                UnsupportedReason::NotYetOnThisHost,
            ))));
            return DialogHold::new(());
        }
        let multiple = request.is_multiple();
        let intent = PickerIntent::Open {
            mime_types: open_types(&request.filters, |extension| {
                self.picker.mime_type_of(extension)
            }),
            multiple,
        };
        let (filters, max_bytes, keep_access) =
            (request.filters, request.max_bytes, request.keep_access);
        self.ask(
            OPEN_KEY,
            &intent,
            AfterAnswer::StopsWithCaller,
            answer,
            move |documents, let_go| {
                read_picks(
                    documents,
                    &filters,
                    multiple,
                    max_bytes,
                    let_go,
                    keep_access,
                )
            },
        )
    }

    fn save(
        &self,
        _window: &WindowRef,
        request: SaveRequest,
        answer: Resolver<SaveResult>,
    ) -> DialogHold {
        let Some(contents) = request.contents else {
            // A save without contents answers a path to write, which a
            // content URI is not.
            let _ = answer.resolve(Err(DialogError::Unsupported(Unsupported::of::<
                FileDialogCapability,
            >(
                UnsupportedReason::NotOnThisPlatform,
            ))));
            return DialogHold::new(());
        };
        let intent = create_intent(
            request.suggested_name.as_deref(),
            &request.filters,
            |extension| self.picker.mime_type_of(extension),
        );
        let proposed = match &intent {
            PickerIntent::Create { name, .. } => name.clone(),
            PickerIntent::Open { .. } => None,
        };
        self.ask(
            SAVE_KEY,
            &intent,
            AfterAnswer::Completes,
            answer,
            move |documents, _| write_save(documents, &contents, proposed),
        )
    }
}

/// The answers to requests an earlier Activity took, claimed once per UI
/// runtime so the host holds none of them: an open's answer is let go, and
/// the document a save created is deleted.
#[derive(Default)]
pub(crate) struct EarlierAnswers {
    claimed: Cell<bool>,
}

impl EarlierAnswers {
    /// Claim every earlier answer under the dialogs' keys, the first time a
    /// runtime's dialogs are acquired.
    pub(crate) fn claim<P: DocumentPicker>(
        &self,
        activity: &Activity,
        completions: &CompletionSource,
        picker: &Rc<P>,
        workers: &Rc<Workers>,
    ) {
        if self.claimed.replace(true) {
            return;
        }
        for (key, created) in [(OPEN_KEY, false), (SAVE_KEY, true)] {
            for _ in 0..MAX_OPEN_REQUESTS_PER_CAPABILITY {
                let Ok(request) = activity.reattach(completions, key) else {
                    break;
                };
                let (ticket, answered) = request.into_parts();
                // The claim waits for its answer in the closure the answer
                // runs, and goes with the runtime otherwise.
                let mut claim = Some(ticket);
                let activity = activity.clone();
                let picker = Rc::clone(picker);
                let workers = Rc::clone(workers);
                answered.on_complete(move |outcome| {
                    drop(claim.take());
                    let Ok(Ok(mut result)) = outcome else {
                        return;
                    };
                    let (ResultCode::Ok, true, Some(data)) =
                        (result.code(), created, result.take_data())
                    else {
                        // Dropping the result lets the host release its
                        // `Intent`.
                        return;
                    };
                    let Ok(Ok((mut documents, _interrupt))) =
                        activity.take_data(data, |frame| picker.picked(frame, 1))
                    else {
                        return;
                    };
                    // With every worker busy the empty document stays, as it
                    // would had the process died.
                    let _ = workers.spawn("flui-system-documents", move || {
                        for index in 0..documents.len() {
                            documents.discard(index);
                        }
                    });
                });
            }
        }
    }
}

/// The file dialogs of the acquisition `ctx` over `activity` and `picker`;
/// the first acquisition of a UI runtime claims what an earlier Activity's
/// requests were answered.
///
/// # Errors
///
/// [`Unsupported`] with [`UnsupportedReason::NoWindow`] for a widget in no
/// window.
pub(crate) fn provide<P: DocumentPicker>(
    ctx: &ProviderContext<'_>,
    activity: Activity,
    picker: Rc<P>,
) -> Result<FileDialogs, Unsupported> {
    let workers = ctx.runtime_local::<Workers>();
    ctx.runtime_local::<EarlierAnswers>()
        .claim(&activity, ctx.completions(), &picker, &workers);
    FileDialogs::try_new(
        AndroidDialogs::new(activity, ctx.completions().clone(), picker, workers),
        ctx,
    )
}

#[cfg(test)]
mod tests;
