//! The system's open and save dialogs.
//!
//! A widget acquires [`FileDialogs`] through
//! `cx.capability::<FileDialogCapability>()` in a lifecycle hook of a
//! widget in a window. The handle is bound to that window: the dialog is
//! modal to it, and the window holds at most one dialog at a time.

mod request;

use std::{cell::RefCell, collections::HashSet, fmt, rc::Rc};

use flui_platform::capability::{
    Capability, Completion, CompletionSource, ProviderContext, Resolver, WindowRef,
};
use flui_platform::{Unsupported, UnsupportedReason, WindowId};

pub use request::{FileFilter, OpenRequest, PickedFile, SaveRequest};

/// The file dialog capability: `cx.capability::<FileDialogCapability>()`
/// hands out [`FileDialogs`].
#[derive(Debug)]
#[allow(
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
    /// The request is malformed: a filter without a name or without
    /// anything to match, an extension or MIME type that is not one, filters
    /// on a folder dialog, a suggested name that is a path, or a relative
    /// directory. Nothing was shown.
    #[error("the dialog request is malformed")]
    InvalidRequest,
    /// The service the backend shows dialogs through is missing or not
    /// running (no file chooser portal); the reason says whether trying
    /// later may help.
    #[error("the dialog service is unavailable: {0:?}")]
    Unavailable(UnsupportedReason),
    /// The window the dialog belonged to closed before it answered.
    #[error("the window closed before the dialog answered")]
    WindowClosed,
    /// The system answered with a pick that is not a local file, which this
    /// handle cannot hand out.
    #[error("the pick is not a local file")]
    NotAFile,
    /// The system answered with an error the handle does not name; `code`
    /// is the system's, for diagnostics only.
    #[error("the system failed with code {code}")]
    Failed {
        /// The operating system's error code.
        code: i32,
    },
}

/// What an open dialog answers: the picks, `None` when the user cancelled.
pub type OpenResult = Result<Option<Vec<PickedFile>>, DialogError>;

/// What a save dialog answers: the pick, `None` when the user cancelled.
pub type SaveResult = Result<Option<PickedFile>, DialogError>;

/// The system's dialogs as [`FileDialogs`] reach them.
///
/// Owner-local. A backend answers each dialog through its [`Resolver`] when
/// the user does; it never calls back into the handle. A request it
/// receives is well formed, and its window shows no other dialog of the
/// handle's.
pub trait FileDialogBackend: 'static {
    /// Show an open dialog modal to `window`.
    fn open(&self, window: &WindowRef, request: OpenRequest, answer: Resolver<OpenResult>);

    /// Show a save dialog modal to `window`.
    fn save(&self, window: &WindowRef, request: SaveRequest, answer: Resolver<SaveResult>);
}

/// The windows of a UI runtime that show a dialog, so two handles in one
/// window share the one-dialog rule.
#[derive(Default)]
struct OpenDialogs(RefCell<HashSet<WindowId>>);

/// Widget-facing file dialogs for one window.
///
/// Owner-thread (`!Send`), cheap to clone. A dialog answers with a
/// [`Completion`], which settles at an owner turn after the user answers,
/// never inside the call that asked.
#[derive(Clone)]
pub struct FileDialogs {
    backend: Rc<dyn FileDialogBackend>,
    completions: CompletionSource,
    window: WindowRef,
    open: Rc<OpenDialogs>,
}

impl FileDialogs {
    /// A handle over `backend` for the acquisition `cx`, bound to the
    /// acquiring widget's window.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] with [`UnsupportedReason::NoWindow`] when the widget
    /// is in no window: a dialog is modal to one.
    pub fn try_new(
        backend: impl FileDialogBackend,
        cx: &ProviderContext<'_>,
    ) -> Result<Self, Unsupported> {
        let window = cx.window().ok_or(Unsupported::of::<FileDialogCapability>(
            UnsupportedReason::NoWindow,
        ))?;
        Ok(Self {
            backend: Rc::new(backend),
            completions: cx.completions().clone(),
            window,
            open: cx.runtime_local::<OpenDialogs>(),
        })
    }

    /// Show an open dialog.
    pub fn open(&self, request: OpenRequest) -> Completion<OpenResult> {
        let valid = request.is_valid();
        self.show(valid, |backend, window, answer| {
            backend.open(window, request, answer);
        })
    }

    /// Show a save dialog.
    pub fn save(&self, request: SaveRequest) -> Completion<SaveResult> {
        let valid = request.is_valid();
        self.show(valid, |backend, window, answer| {
            backend.save(window, request, answer);
        })
    }

    /// Ask `backend` through `ask` when the request is `valid` and the window
    /// shows no dialog; answer the error at once otherwise.
    ///
    /// The window is marked busy until the backend's answer settles. The
    /// mark is cleared by the answer's own completion, so a backend that
    /// panics or drops its resolver unanswered clears it too: the dropped
    /// resolver ends that completion, and the caller's ends as
    /// `ProviderDropped`.
    fn show<T: Send + 'static>(
        &self,
        valid: bool,
        ask: impl FnOnce(&dyn FileDialogBackend, &WindowRef, Resolver<Result<T, DialogError>>),
    ) -> Completion<Result<T, DialogError>> {
        let (completion, answer) = self.completions.pending();
        let window = self.window.id();
        let refusal = if !valid {
            Some(DialogError::InvalidRequest)
        } else if !self.open.0.borrow_mut().insert(window) {
            Some(DialogError::Busy)
        } else {
            None
        };
        if let Some(error) = refusal {
            // The owner retiring first leaves nothing waiting for the answer.
            let _ = answer.resolve(Err(error));
            return completion;
        }

        let (backend_answered, backend_answer) = self.completions.pending();
        let open = Rc::clone(&self.open);
        let mut answer = Some(answer);
        backend_answered.on_complete(move |outcome| {
            open.0.borrow_mut().remove(&window);
            // Runs once; an abandoned outcome drops `answer` unanswered, so
            // the caller's completion ends as the backend's did.
            let answer = answer.take();
            if let (Ok(result), Some(answer)) = (outcome, answer) {
                let _ = answer.resolve(result);
            }
        });
        ask(&*self.backend, &self.window, backend_answer);
        completion
    }
}

impl fmt::Debug for FileDialogs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileDialogs")
            .field("window", &self.window.id())
            .finish_non_exhaustive()
    }
}
