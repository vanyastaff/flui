//! The system's open and save dialogs.
//!
//! A widget acquires [`FileDialogs`] through
//! `ctx.capability::<FileDialogCapability>()` in a lifecycle hook of a
//! widget in a window. The handle is bound to that window: the dialog is
//! modal to it, the window holds at most one dialog at a time, and the
//! dialog ends with the window.
//!
//! A dialog lasts while someone waits for its answer. [`FileDialogs::open`]
//! and [`FileDialogs::save`] hand out a [`Dialog`]; dropping it, or the
//! window closing, dismisses the system's dialog through the
//! [`DialogHold`] its backend gave for it, and frees the window for the next
//! one.

mod request;

use std::path::PathBuf;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;
use std::{any::Any, cell::Cell, cell::RefCell, fmt, future::Future, rc::Rc, rc::Weak};

use flui_platform::capability::{
    Abandoned, Capability, Completion, CompletionSource, ProviderContext, Resolver,
    WindowCloseWatch, WindowRef,
};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason, WindowId};
use rustc_hash::FxHashMap;

pub use request::{
    FileFilter, MalformedRequest, OpenRequest, PickedFile, Picks, SaveLocation, SaveRequest,
};

/// The file dialog capability: `ctx.capability::<FileDialogCapability>()`
/// hands out [`FileDialogs`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum FileDialogCapability {}

impl Capability for FileDialogCapability {
    type Handle = FileDialogs;
    const NAME: &'static str = "file-dialog";
}

/// Why a dialog gave no answer. The user cancelling is not an error: it
/// answers `Ok(None)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DialogError {
    /// The window already shows a dialog; ask again once it answers.
    #[error("the window already shows a dialog")]
    Busy,
    /// The request is malformed, as the [`MalformedRequest`] says. Nothing
    /// was shown.
    #[error("the dialog request is malformed: {0}")]
    InvalidRequest(#[from] MalformedRequest),
    /// The service the backend shows dialogs through is not here (no file
    /// chooser portal); the [`Unsupported`] says why and whether trying later
    /// may help.
    #[error("the dialog service cannot do this: {0}")]
    Unsupported(#[from] Unsupported),
    /// The window the dialog was modal to closed: before it answered, or
    /// before it was asked for, in which case nothing was shown.
    #[error("the window closed before the dialog answered")]
    WindowClosed,
    /// The system answered with a pick that is not a local file, which this
    /// handle cannot hand out.
    #[error("the pick is not a local file")]
    NotAFile,
    /// The picks hold more than the request's
    /// [`max_bytes`](OpenRequest::max_bytes), on a system that hands over a
    /// pick's contents (a browser). Nothing was read past the check.
    #[error("the picks hold {actual} bytes, more than the {limit} the request reads")]
    TooLarge {
        /// The picks' size, as the system declared it or as far as it was
        /// read.
        actual: u64,
        /// The request's [`max_bytes`](OpenRequest::max_bytes).
        limit: u64,
    },
    /// The system failed in a way the handle does not name; the
    /// [`SystemFailure`] says where the failure came from, for diagnostics.
    #[error("the system failed: {0}")]
    Failed(#[from] SystemFailure),
}

impl DialogError {
    /// What is not here, when that is why the dialog failed.
    #[must_use]
    pub const fn unsupported(&self) -> Option<Unsupported> {
        match self {
            Self::Unsupported(unsupported) => Some(*unsupported),
            Self::Busy
            | Self::InvalidRequest(_)
            | Self::WindowClosed
            | Self::NotAFile
            | Self::TooLarge { .. }
            | Self::Failed(_) => None,
        }
    }
}

/// What an open dialog answers: what the user picked, `None` when the user
/// cancelled. The picks hold one file unless the request
/// [allowed several](OpenRequest::allow_multiple).
pub type OpenResult = Result<Option<Picks>, DialogError>;

/// What a save dialog answers: where the user chose to save, `None` when the
/// user cancelled.
pub type SaveResult = Result<Option<SaveLocation>, DialogError>;

/// What a [`FileDialogBackend`] gives the handle for each dialog it shows:
/// the system's dialog stays up while the handle holds it.
///
/// The handle drops it when the dialog ends, whatever ended it: the answer,
/// the caller dropping its [`Dialog`], the window closing, the UI runtime
/// retiring. A backend whose system dialog is still up when its hold drops
/// dismisses it there (the portal's `Request.Close`, the ticket of a started
/// activity), and one that has answered does nothing. The handle drops it on
/// the owner thread, with none of its own state borrowed.
#[must_use = "dropping the hold dismisses the dialog it holds"]
pub struct DialogHold(#[expect(dead_code, reason = "held for its drop")] Box<dyn Any>);

impl DialogHold {
    /// A hold whose drop drops `keep`: the value whose `Drop` dismisses the
    /// system's dialog. `()` holds nothing, for a backend whose dialogs
    /// cannot be dismissed or that answered already.
    pub fn new(keep: impl Any) -> Self {
        Self(Box::new(keep))
    }
}

impl fmt::Debug for DialogHold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DialogHold").finish_non_exhaustive()
    }
}

/// The system's dialogs as [`FileDialogs`] reach them.
///
/// Owner-local. A backend answers each dialog through its [`Resolver`] when
/// the user does; it never calls back into the handle. A request it
/// receives is well formed, its window is open and shows no other dialog of
/// the handle's. Each call returns the [`DialogHold`] the dialog lasts by.
pub trait FileDialogBackend: 'static {
    /// Show an open dialog modal to `window`.
    fn open(
        &self,
        window: &WindowRef,
        request: OpenRequest,
        answer: Resolver<OpenResult>,
    ) -> DialogHold;

    /// Show a save dialog modal to `window`.
    fn save(
        &self,
        window: &WindowRef,
        request: SaveRequest,
        answer: Resolver<SaveResult>,
    ) -> DialogHold;
}

/// Answers a dialog's caller with an error, once.
type Refusal = Box<dyn FnOnce(DialogError)>;

/// One dialog a window shows: what ends it and how its caller hears.
struct Shown {
    window: WindowId,
    /// What keeps the backend's dialog up; `None` until the backend returns.
    hold: Cell<Option<DialogHold>>,
    /// Answers the caller with an error; taken by whichever end comes first.
    refuse: Cell<Option<Refusal>>,
    /// Ends the dialog when its window closes.
    watch: Cell<Option<WindowCloseWatch>>,
}

/// How a dialog ended, when the handle ends it.
#[derive(Clone, Copy)]
enum Ending {
    /// The backend answered; the caller hears the answer.
    Answered,
    /// Nobody waits for the answer any more.
    Dismissed,
    /// The window closed; the caller hears [`DialogError::WindowClosed`].
    WindowClosed,
}

/// The dialogs a UI runtime's windows show, one per window, so two handles
/// in one window share the one-dialog rule.
#[derive(Default)]
struct OpenDialogs(RefCell<FxHashMap<WindowId, Rc<Shown>>>);

impl OpenDialogs {
    /// End `shown` if it is still its window's dialog, as `ending` says.
    /// Returns whether it was.
    ///
    /// What it ends runs provider and caller code (the hold's drop, the
    /// refusal, the watch's drop), each after the table's borrow ends.
    fn end(&self, shown: &Rc<Shown>, ending: Ending) -> bool {
        let current = {
            let mut table = self.0.borrow_mut();
            match table.get(&shown.window) {
                Some(entry) if Rc::ptr_eq(entry, shown) => table.remove(&shown.window),
                _ => None,
            }
        };
        let Some(current) = current else {
            return false;
        };
        let refuse = current.refuse.take();
        let watch = current.watch.take();
        let hold = current.hold.take();
        match (ending, refuse) {
            (Ending::WindowClosed, Some(refuse)) => refuse(DialogError::WindowClosed),
            // The answer reaches the caller through its own path, or nobody
            // waits for one.
            (Ending::Answered | Ending::Dismissed | Ending::WindowClosed, refuse) => drop(refuse),
        }
        drop(watch);
        drop(hold);
        true
    }
}

/// Widget-facing file dialogs for one window.
///
/// Owner-thread (`!Send`), cheap to clone. A dialog answers through its
/// [`Dialog`], which settles at an owner turn after the user answers, never
/// inside the call that asked.
#[derive(Clone)]
pub struct FileDialogs {
    backend: Rc<dyn FileDialogBackend>,
    completions: CompletionSource,
    window: WindowRef,
    open: Rc<OpenDialogs>,
}

impl FileDialogs {
    /// A handle over `backend` for the acquisition `ctx`, bound to the
    /// acquiring widget's window.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] with [`UnsupportedReason::NoWindow`] when the widget
    /// is in no window: a dialog is modal to one.
    pub fn try_new(
        backend: impl FileDialogBackend,
        ctx: &ProviderContext<'_>,
    ) -> Result<Self, Unsupported> {
        let window = ctx.window().ok_or(Unsupported::of::<FileDialogCapability>(
            UnsupportedReason::NoWindow,
        ))?;
        Ok(Self {
            backend: Rc::new(backend),
            completions: ctx.completions().clone(),
            window,
            open: ctx.runtime_local::<OpenDialogs>(),
        })
    }

    /// Show an open dialog.
    ///
    /// A backend that answers several picks to a request that allowed one
    /// answers [`SystemFailure::MalformedAnswer`]: its answer is not one the
    /// request can have.
    pub fn open(&self, request: OpenRequest) -> Dialog<OpenResult> {
        let checked = request.check();
        let multiple = request.is_multiple();
        self.show(
            checked,
            |backend, window, answer| backend.open(window, request, answer),
            move |result| match result {
                Ok(Some(picks)) if !multiple && picks.len() > 1 => {
                    Err(SystemFailure::MalformedAnswer.into())
                }
                result => result,
            },
        )
    }

    /// Show a save dialog.
    ///
    /// A backend that names a selected filter the request does not have
    /// answers [`SystemFailure::MalformedAnswer`].
    ///
    /// A request that carries [`contents`](SaveRequest::contents) answers
    /// once they are written: a backend that hands over no path (a browser)
    /// writes them itself and answers the file it wrote; to a path the user
    /// chose, the handle writes them on a thread of its own, since no file
    /// system call runs on the owner thread (ADR-0360 §2), and a failed
    /// write answers [`DialogError::Failed`] with the system's code. The
    /// window is free for the next dialog once the user has chosen; the
    /// write itself cannot be called back, so dropping the [`Dialog`] while
    /// it runs drops only the answer.
    pub fn save(&self, request: SaveRequest) -> Dialog<SaveResult> {
        let checked = request.check();
        let filters = request.filters.len();
        let contents = request.contents_to_write().cloned();
        let chosen = self.show(
            checked,
            |backend, window, answer| backend.save(window, request, answer),
            move |result| match result {
                Ok(Some(location)) if location.filter().is_some_and(|index| index >= filters) => {
                    Err(SystemFailure::MalformedAnswer.into())
                }
                result => result,
            },
        );
        match contents {
            Some(contents) => self.write_where_chosen(chosen, contents),
            None => chosen,
        }
    }

    /// `chosen`'s answer, after `contents` are written to the path it chose;
    /// a location without a path was written by the backend.
    fn write_where_chosen(
        &self,
        chosen: Dialog<SaveResult>,
        contents: Arc<[u8]>,
    ) -> Dialog<SaveResult> {
        let Dialog {
            completion: chosen,
            guard,
        } = chosen;
        let completions = self.completions.clone();
        let (completion, caller) = completions.pending();
        let mut waiting = Some((caller, contents));
        chosen.on_complete(move |outcome| {
            let Some((caller, contents)) = waiting.take() else {
                return;
            };
            match outcome {
                Ok(Ok(Some(location))) => match location.file().path() {
                    Some(path) => {
                        write_file(&completions, path.to_path_buf(), contents, location, caller);
                    }
                    // The backend wrote the contents itself (a browser).
                    None => drop(caller.resolve(Ok(Some(location)))),
                },
                // A retired owner has nobody left to answer.
                Ok(unwritten) => drop(caller.resolve(unwritten)),
                // The backend dropped its answer: so does the handle.
                Err(_) => drop(caller),
            }
        });
        Dialog { completion, guard }
    }

    /// Ask `backend` through `ask` when the request `checked` well formed,
    /// the window is open and shows no dialog; answer the error at once
    /// otherwise. The backend's answer reaches the caller through `check`.
    ///
    /// The window is marked busy until the dialog ends: the backend's answer
    /// settles (a backend that drops its resolver unanswered ends it too,
    /// and the caller's completion ends as `ProviderDropped`), the backend
    /// panics while it shows the dialog, the caller drops its [`Dialog`], or
    /// the window closes.
    fn show<T: 'static>(
        &self,
        checked: Result<(), MalformedRequest>,
        ask: impl FnOnce(
            &dyn FileDialogBackend,
            &WindowRef,
            Resolver<Result<T, DialogError>>,
        ) -> DialogHold,
        check: impl FnOnce(Result<T, DialogError>) -> Result<T, DialogError> + 'static,
    ) -> Dialog<Result<T, DialogError>> {
        let (completion, answer) = self.completions.pending();
        let window = self.window.id();
        let refusal = if let Err(malformed) = checked {
            Some(DialogError::InvalidRequest(malformed))
        } else if self.window.lifetime().is_closed() {
            Some(DialogError::WindowClosed)
        } else if self.open.0.borrow().contains_key(&window) {
            Some(DialogError::Busy)
        } else {
            None
        };
        if let Some(error) = refusal {
            // The owner retiring first leaves nothing waiting for the answer.
            let _ = answer.resolve(Err(error));
            return Dialog {
                completion,
                guard: DialogGuard::default(),
            };
        }

        // The caller's resolver, taken by whichever end answers it.
        let answer = Rc::new(Cell::new(Some(answer)));
        let refused = Rc::clone(&answer);
        let shown = Rc::new(Shown {
            window,
            hold: Cell::new(None),
            refuse: Cell::new(Some(Box::new(move |error| {
                if let Some(answer) = refused.take() {
                    let _ = answer.resolve(Err(error));
                }
            }))),
            watch: Cell::new(None),
        });
        self.open.0.borrow_mut().insert(window, Rc::clone(&shown));
        let open = Rc::downgrade(&self.open);
        let closing = Rc::downgrade(&shown);
        shown
            .watch
            .set(Some(self.window.lifetime().on_close(move || {
                if let (Some(open), Some(shown)) = (open.upgrade(), closing.upgrade()) {
                    open.end(&shown, Ending::WindowClosed);
                }
            })));

        let (backend_answered, backend_answer) = self.completions.pending();
        let open = Rc::downgrade(&self.open);
        let answered = Rc::clone(&shown);
        let mut check = Some(check);
        backend_answered.on_complete(move |outcome| {
            // Runs once. A dialog that ended first (dismissed, its window
            // closed, the runtime retiring) drops the answer: its caller has
            // heard already or waits no more.
            let ended_here = open
                .upgrade()
                .is_some_and(|open| open.end(&answered, Ending::Answered));
            let caller = answer.take();
            if let (true, Ok(result), Some(caller), Some(check)) =
                (ended_here, outcome, caller, check.take())
            {
                let _ = caller.resolve(check(result));
            }
            // An abandoned outcome drops the caller's resolver unanswered, so
            // the caller's completion ends as the backend's did.
        });

        // Built before the backend runs: a backend that panics while it shows
        // the dialog leaves no handle, so the guard ends the dialog on the way
        // out and the window is not left busy.
        let guard = DialogGuard {
            open: Rc::downgrade(&self.open),
            shown: Rc::downgrade(&shown),
        };
        let hold = ask(&*self.backend, &self.window, backend_answer);
        if self
            .open
            .0
            .borrow()
            .get(&window)
            .is_some_and(|entry| Rc::ptr_eq(entry, &shown))
        {
            shown.hold.set(Some(hold));
        } else {
            // The dialog ended while the backend showed it.
            drop(hold);
        }
        Dialog { completion, guard }
    }
}

impl fmt::Debug for FileDialogs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileDialogs")
            .field("window", &self.window.id())
            .finish_non_exhaustive()
    }
}

/// Write `contents` to `path` on a thread of its own, then answer `caller`
/// with `location`, or with the failure of the write.
#[cfg(not(target_arch = "wasm32"))]
fn write_file(
    completions: &CompletionSource,
    path: PathBuf,
    contents: Arc<[u8]>,
    location: SaveLocation,
    caller: Resolver<SaveResult>,
) {
    let (written, answer) = completions.pending::<Result<(), DialogError>>();
    let mut waiting = Some((caller, location));
    written.on_complete(move |outcome| {
        let Some((caller, location)) = waiting.take() else {
            return;
        };
        match outcome {
            Ok(Ok(())) => drop(caller.resolve(Ok(Some(location)))),
            Ok(Err(error)) => drop(caller.resolve(Err(error))),
            Err(_) => drop(caller),
        }
    });
    // The thread answers through the slot; a thread that cannot start leaves
    // the resolver there, and the failure is answered here.
    let slot = Arc::new(Mutex::new(Some(answer)));
    let writer = Arc::clone(&slot);
    let started = std::thread::Builder::new()
        .name("flui-system-save".to_owned())
        .spawn(move || {
            let written = std::fs::write(&path, &contents).map_err(|error| write_failure(&error));
            let answer = writer
                .lock()
                .expect("BUG: the save slot's holders run no code under its lock")
                .take();
            if let Some(answer) = answer {
                // A retired owner has nobody left to answer.
                let _ = answer.resolve(written);
            }
        });
    if let Err(error) = started {
        let answer = slot
            .lock()
            .expect("BUG: the save slot's holders run no code under its lock")
            .take();
        if let Some(answer) = answer {
            let _ = answer.resolve(Err(write_failure(&error)));
        }
    }
}

/// A save to a path on a system with no file system of its own: a backend
/// of this platform hands over no path, so only a stand-in answers one.
#[cfg(target_arch = "wasm32")]
fn write_file(
    _: &CompletionSource,
    _: PathBuf,
    _: Arc<[u8]>,
    _: SaveLocation,
    caller: Resolver<SaveResult>,
) {
    // A retired owner has nobody left to answer.
    let _ = caller.resolve(Err(DialogError::Unsupported(Unsupported::of::<
        FileDialogCapability,
    >(
        UnsupportedReason::NotOnThisPlatform,
    ))));
}

/// The failure a write reports: the system's code, or a write that took no
/// bytes without one ([`SystemFailure::MalformedAnswer`]).
#[cfg(not(target_arch = "wasm32"))]
fn write_failure(error: &std::io::Error) -> DialogError {
    DialogError::Failed(
        error
            .raw_os_error()
            .map_or(SystemFailure::MalformedAnswer, SystemFailure::Os),
    )
}

/// Dismisses its dialog when dropped, unless the dialog ended first.
#[derive(Default)]
struct DialogGuard {
    open: Weak<OpenDialogs>,
    shown: Weak<Shown>,
}

impl Drop for DialogGuard {
    fn drop(&mut self) {
        if let (Some(open), Some(shown)) = (self.open.upgrade(), self.shown.upgrade()) {
            open.end(&shown, Ending::Dismissed);
        }
    }
}

/// A dialog the system shows, and the answer it will give.
///
/// Owner-thread (`!Send`). The dialog lasts while this value, or the closure
/// or future it was turned into, is held: dropping it dismisses the system's
/// dialog and frees the window for the next one, and nothing is delivered.
/// The answer settles at an owner turn, never inside the call that asked;
/// when the window closes first it is [`DialogError::WindowClosed`], and when
/// the UI runtime retires first the completion ends
/// [`OwnerRetired`](Abandoned::OwnerRetired).
#[must_use = "dropping a dialog dismisses it"]
pub struct Dialog<T: 'static> {
    completion: Completion<T>,
    guard: DialogGuard,
}

impl<T: 'static> Dialog<T> {
    /// Runs `f` once, with the outcome, at an owner turn boundary; the
    /// dialog stays up until then.
    ///
    /// `f` is an `FnMut` called at most once, as
    /// [`Completion::on_complete`] says; the dialog is dismissed if `f` is
    /// dropped unrun.
    pub fn on_complete(self, mut f: impl FnMut(Result<T, Abandoned>) + 'static) {
        let Self { completion, guard } = self;
        completion.on_complete(move |outcome| {
            // Held until the outcome arrives, so the dialog stays up.
            let _guard = &guard;
            f(outcome);
        });
    }

    /// Takes the outcome if it has settled: `None` while the dialog is up,
    /// and after the outcome was taken.
    pub fn poll(&mut self) -> Option<Result<T, Abandoned>> {
        self.completion.poll()
    }

    /// The outcome as a future, for code outside the frame path; dropping
    /// the future dismisses the dialog.
    pub fn into_future(self) -> impl Future<Output = Result<T, Abandoned>> {
        let Self { completion, guard } = self;
        let outcome = completion.into_future();
        async move {
            let _guard = guard;
            outcome.await
        }
    }
}

impl<T: 'static> fmt::Debug for Dialog<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dialog")
            .field("completion", &self.completion)
            .finish_non_exhaustive()
    }
}
