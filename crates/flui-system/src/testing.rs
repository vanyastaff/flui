//! The scripted system services a scenario registers instead of the OS's
//! (ADR-0154 §10, ADR-0243): [`ScriptedSystem`], over a
//! [`ScriptedLauncher`] and a [`ScriptedFileDialogs`].

use std::{cell::RefCell, collections::VecDeque, fmt, rc::Rc};

use flui_platform::Unsupported;
use flui_platform::WindowId;
use flui_platform::capability::{
    CapabilityRegistrar, Plugin, ProviderContext, Resolver, WindowRef,
};
use flui_platform::launch::{Launcher, LauncherCapability};
pub use flui_platform::testing::{LaunchRequest, ScriptedLauncher};

use crate::dialog::{
    FileDialogBackend, FileDialogCapability, FileDialogs, OpenRequest, OpenResult, SaveRequest,
    SaveResult,
};

/// One dialog a [`ScriptedFileDialogs`] was asked to show.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DialogRequest {
    /// An open dialog, modal to `window`.
    Open {
        /// The window it was modal to.
        window: WindowId,
        /// What it asked for.
        request: OpenRequest,
    },
    /// A save dialog, modal to `window`.
    Save {
        /// The window it was modal to.
        window: WindowId,
        /// What it asked for.
        request: SaveRequest,
    },
}

#[derive(Default)]
struct DialogState {
    requests: Vec<DialogRequest>,
    open_script: VecDeque<OpenResult>,
    save_script: VecDeque<SaveResult>,
    holding: bool,
    held_open: VecDeque<Resolver<OpenResult>>,
    held_save: VecDeque<Resolver<SaveResult>>,
}

/// Dialogs whose user is a script: each dialog is recorded and answered with
/// the next scripted result of its kind, `Ok(None)` (cancelled) when none is
/// scripted, or held until the test releases it.
///
/// Cheap to clone; clones share the script and the record.
#[derive(Clone, Default)]
pub struct ScriptedFileDialogs {
    state: Rc<RefCell<DialogState>>,
}

impl ScriptedFileDialogs {
    /// Dialogs the user cancels.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer the next unscripted open dialog with `result`.
    pub fn answer_next_open(&self, result: OpenResult) {
        self.state.borrow_mut().open_script.push_back(result);
    }

    /// Answer the next unscripted save dialog with `result`.
    pub fn answer_next_save(&self, result: SaveResult) {
        self.state.borrow_mut().save_script.push_back(result);
    }

    /// Hold every later dialog open until it is released.
    pub fn hold(&self) {
        self.state.borrow_mut().holding = true;
    }

    /// Answer the oldest held open dialog; `false` when none is held.
    pub fn release_open(&self, result: OpenResult) -> bool {
        let held = self.state.borrow_mut().held_open.pop_front();
        held.is_some_and(|answer| {
            let _ = answer.resolve(result);
            true
        })
    }

    /// Answer the oldest held save dialog; `false` when none is held.
    pub fn release_save(&self, result: SaveResult) -> bool {
        let held = self.state.borrow_mut().held_save.pop_front();
        held.is_some_and(|answer| {
            let _ = answer.resolve(result);
            true
        })
    }

    /// The dialogs asked for so far, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<DialogRequest> {
        self.state.borrow().requests.clone()
    }
}

impl FileDialogBackend for ScriptedFileDialogs {
    fn open(&self, window: &WindowRef, request: OpenRequest, answer: Resolver<OpenResult>) {
        let scripted = {
            let mut state = self.state.borrow_mut();
            state.requests.push(DialogRequest::Open {
                window: window.id(),
                request,
            });
            if state.holding {
                state.held_open.push_back(answer);
                return;
            }
            (state.open_script.pop_front().unwrap_or(Ok(None)), answer)
        };
        let (result, answer) = scripted;
        let _ = answer.resolve(result);
    }

    fn save(&self, window: &WindowRef, request: SaveRequest, answer: Resolver<SaveResult>) {
        let scripted = {
            let mut state = self.state.borrow_mut();
            state.requests.push(DialogRequest::Save {
                window: window.id(),
                request,
            });
            if state.holding {
                state.held_save.push_back(answer);
                return;
            }
            (state.save_script.pop_front().unwrap_or(Ok(None)), answer)
        };
        let (result, answer) = scripted;
        let _ = answer.resolve(result);
    }
}

impl fmt::Debug for ScriptedFileDialogs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("ScriptedFileDialogs")
            .field("requests", &state.requests.len())
            .field("held", &(state.held_open.len() + state.held_save.len()))
            .finish_non_exhaustive()
    }
}

/// The system services' plugin over a script: registers a [`Launcher`] over
/// `launcher` and [`FileDialogs`] over `dialogs`, which the test keeps
/// clones of.
#[derive(Clone, Debug, Default)]
pub struct ScriptedSystem {
    launcher: ScriptedLauncher,
    dialogs: ScriptedFileDialogs,
}

impl ScriptedSystem {
    /// The plugin over `launcher` and `dialogs`.
    #[must_use]
    pub fn new(launcher: ScriptedLauncher, dialogs: ScriptedFileDialogs) -> Self {
        Self { launcher, dialogs }
    }
}

impl Plugin for ScriptedSystem {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        let launcher = self.launcher.clone();
        registrar.provide::<LauncherCapability>(move |cx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Launcher::new(launcher.clone(), cx))
        });
        let dialogs = self.dialogs.clone();
        registrar.provide::<FileDialogCapability>(move |cx: &ProviderContext<'_>| {
            FileDialogs::try_new(dialogs.clone(), cx)
        });
    }
}
