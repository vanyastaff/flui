//! The scripted system services a scenario registers instead of the OS's
//! (ADR-0154 §10, ADR-0243): [`ScriptedSystem`], over a
//! [`ScriptedLauncher`] and a [`ScriptedFileDialogs`], both on the scenario
//! primitive ([`flui_platform::capability::scripted`]).

use std::{cell::RefCell, fmt, rc::Rc};

use flui_platform::Unsupported;
use flui_platform::WindowId;
use flui_platform::capability::scripted::{Answer, Scenario, Script};
use flui_platform::capability::{
    CapabilityRegistrar, Plugin, ProviderContext, Resolver, WindowRef,
};
use flui_platform::launch::{Launcher, LauncherCapability};
pub use flui_platform::testing::{LaunchRequest, ScriptedLauncher};

use crate::dialog::{
    DialogHold, FileDialogBackend, FileDialogCapability, FileDialogs, OpenRequest, OpenResult,
    SaveRequest, SaveResult,
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

/// Dialogs whose user is a script: an open dialog is answered by one
/// [`Script`] and a save dialog by another, since their answers differ in
/// type. A dialog its script does not answer is the scenario's
/// [`Unexpected`](flui_platform::capability::scripted::ScenarioFailure::Unexpected)
/// failure, never a cancel nobody wrote.
///
/// Cheap to clone; clones share the scripts and the record.
#[derive(Clone)]
pub struct ScriptedFileDialogs {
    open: Script<DialogRequest, OpenResult>,
    save: Script<DialogRequest, SaveResult>,
    /// Both kinds, in the order they were asked for.
    requests: Rc<RefCell<Vec<DialogRequest>>>,
}

impl Default for ScriptedFileDialogs {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptedFileDialogs {
    /// Dialogs on a scenario of their own, with nothing scripted.
    #[must_use]
    pub fn new() -> Self {
        Self::on(&Scenario::new())
    }

    /// Dialogs whose scripts are on `scenario`, with nothing scripted.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        Self {
            open: scenario.script("open dialog"),
            save: scenario.script("save dialog"),
            requests: Rc::default(),
        }
    }

    /// The open dialogs' script.
    #[must_use]
    pub fn open_script(&self) -> &Script<DialogRequest, OpenResult> {
        &self.open
    }

    /// The save dialogs' script.
    #[must_use]
    pub fn save_script(&self) -> &Script<DialogRequest, SaveResult> {
        &self.save
    }

    /// Answer the next open dialog with `result`.
    pub fn answer_next_open(&self, result: OpenResult) {
        self.open.answer_next(Answer::value(result));
    }

    /// Answer the next save dialog with `result`.
    pub fn answer_next_save(&self, result: SaveResult) {
        self.save.answer_next(Answer::value(result));
    }

    /// Hold every later dialog open until it is released.
    pub fn hold(&self) {
        self.open.hold();
        self.save.hold();
    }

    /// Answer the oldest held open dialog; `false` when none is held.
    pub fn release_open(&self, result: OpenResult) -> bool {
        self.open.release_oldest(result)
    }

    /// Answer the oldest held save dialog; `false` when none is held.
    pub fn release_save(&self, result: SaveResult) -> bool {
        self.save.release_oldest(result)
    }

    /// The dialogs asked for so far, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<DialogRequest> {
        self.requests.borrow().clone()
    }

    /// How many dialogs were dismissed before they answered: dropped by
    /// their caller, or ended with their window.
    #[must_use]
    pub fn dismissed(&self) -> usize {
        self.open.withdrawn().saturating_add(self.save.withdrawn())
    }
}

impl FileDialogBackend for ScriptedFileDialogs {
    /// The dialog waits on the open script by a ticket: dismissing it
    /// withdraws a held request, as the system takes a dismissed dialog
    /// down.
    fn open(
        &self,
        window: &WindowRef,
        request: OpenRequest,
        answer: Resolver<OpenResult>,
    ) -> DialogHold {
        let request = DialogRequest::Open {
            window: window.id(),
            request,
        };
        self.requests.borrow_mut().push(request.clone());
        DialogHold::new(self.open.receive_with_ticket(request, answer))
    }

    /// As [`open`](Self::open), on the save script.
    fn save(
        &self,
        window: &WindowRef,
        request: SaveRequest,
        answer: Resolver<SaveResult>,
    ) -> DialogHold {
        let request = DialogRequest::Save {
            window: window.id(),
            request,
        };
        self.requests.borrow_mut().push(request.clone());
        DialogHold::new(self.save.receive_with_ticket(request, answer))
    }
}

impl fmt::Debug for ScriptedFileDialogs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptedFileDialogs")
            .field("open", &self.open)
            .field("save", &self.save)
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

    /// The plugin over a launcher and dialogs on `scenario`, with nothing
    /// scripted: the test scripts them through
    /// [`launcher`](Self::launcher) and [`dialogs`](Self::dialogs).
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        Self::new(
            ScriptedLauncher::on(scenario),
            ScriptedFileDialogs::on(scenario),
        )
    }

    /// The launcher the plugin provides over.
    #[must_use]
    pub fn launcher(&self) -> &ScriptedLauncher {
        &self.launcher
    }

    /// The dialogs the plugin provides over.
    #[must_use]
    pub fn dialogs(&self) -> &ScriptedFileDialogs {
        &self.dialogs
    }
}

impl Plugin for ScriptedSystem {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        let launcher = self.launcher.clone();
        registrar.provide::<LauncherCapability>(move |ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Launcher::new(launcher.clone(), ctx))
        });
        let dialogs = self.dialogs.clone();
        registrar.provide::<FileDialogCapability>(move |ctx: &ProviderContext<'_>| {
            FileDialogs::try_new(dialogs.clone(), ctx)
        });
    }
}

/// The system services' real Linux providers, over the desktop portal on the
/// bus at an address rather than the session bus: a scenario that serves a
/// stand-in portal on a private bus registers it to run the providers end to
/// end.
#[cfg(target_os = "linux")]
#[derive(Clone, Debug)]
pub struct PortalOnBus {
    address: std::sync::Arc<str>,
}

#[cfg(target_os = "linux")]
impl PortalOnBus {
    /// The providers on the bus at `address`, a D-Bus address such as
    /// `unix:path=/tmp/bus`.
    #[must_use]
    pub fn new(address: impl Into<std::sync::Arc<str>>) -> Self {
        let address: std::sync::Arc<str> = address.into();
        Self { address }
    }
}

#[cfg(target_os = "linux")]
impl Plugin for PortalOnBus {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        crate::portal::register(registrar, Some(std::sync::Arc::clone(&self.address)));
    }
}
