//! The scripted system services a scenario registers instead of the OS's
//! (ADR-0154 §10, ADR-0243): [`ScriptedSystem`], over one double per
//! service, each on the scenario primitive
//! ([`flui_platform::capability::scripted`]).
//!
//! A double answers what its script says and nothing else: a request no
//! step answers is the scenario's
//! [`Unexpected`](flui_platform::capability::scripted::ScenarioFailure::Unexpected)
//! failure, and its completion ends `ProviderDropped`, never with a success
//! nobody wrote. The taskbar, whose calls answer at once, takes each change
//! unless the test set a refusal, and records it.

use std::{cell::RefCell, fmt, rc::Rc};

use flui_platform::capability::scripted::{Answer, Scenario, Script};
use flui_platform::capability::{
    CapabilityRegistrar, Plugin, ProviderContext, Resolver, WindowRef,
};
use flui_platform::launch::{Launcher, LauncherCapability};
pub use flui_platform::testing::{LaunchRequest, ScriptedLauncher};
use flui_platform::{Unsupported, WindowId};

use crate::dialog::{
    DialogHold, FileDialogBackend, FileDialogCapability, FileDialogs, MessageBoxBackend,
    MessageBoxCapability, MessageBoxRequest, MessageBoxResult, MessageBoxes, OpenRequest,
    OpenResult, SaveRequest, SaveResult,
};
use crate::jump_list::{
    JumpList, JumpListBackend, JumpListCapability, JumpListResult, JumpListTask,
};
use crate::login::{
    LaunchAtLogin, LaunchAtLoginBackend, LaunchAtLoginCapability, LoginItemRequest, LoginItemResult,
};
use crate::os::{
    Architecture, FolderResult, KnownFolder, Os, OsBackend, OsCapability, OsInfo, OsVersion,
};
use crate::share::{Share, ShareBackend, ShareCapability, ShareRequest, ShareResult};
use crate::shortcut::{
    GlobalShortcutBackend, GlobalShortcutCapability, GlobalShortcuts, Shortcut, ShortcutError,
    ShortcutHold, ShortcutPresses,
};
use crate::taskbar::{
    Taskbar, TaskbarBackend, TaskbarBadge, TaskbarCapability, TaskbarError, TaskbarProgress,
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

/// A request a scripted window-bound service received: the window it was
/// for, and what it asked.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct InWindow<R> {
    /// The window the request was for.
    pub window: WindowId,
    /// What it asked.
    pub request: R,
}

/// Message boxes whose user is a [`Script`] of [`InWindow`] requests. A box
/// waits by a ticket, so a dismissed one is withdrawn.
#[derive(Clone, Debug)]
pub struct ScriptedMessageBoxes {
    script: Script<InWindow<MessageBoxRequest>, MessageBoxResult>,
}

impl ScriptedMessageBoxes {
    /// Message boxes whose script is on `scenario`, with nothing scripted.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        Self {
            script: scenario.script("message box"),
        }
    }

    /// The script.
    #[must_use]
    pub fn script(&self) -> &Script<InWindow<MessageBoxRequest>, MessageBoxResult> {
        &self.script
    }
}

impl MessageBoxBackend for ScriptedMessageBoxes {
    fn show(
        &self,
        window: &WindowRef,
        request: MessageBoxRequest,
        answer: Resolver<MessageBoxResult>,
    ) -> DialogHold {
        let request = InWindow {
            window: window.id(),
            request,
        };
        DialogHold::new(self.script.receive_with_ticket(request, answer))
    }
}

/// A share sheet whose user is a [`Script`] of [`InWindow`] requests.
#[derive(Clone, Debug)]
pub struct ScriptedShare {
    script: Script<InWindow<ShareRequest>, ShareResult>,
}

impl ScriptedShare {
    /// A share sheet whose script is on `scenario`, with nothing scripted.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        Self {
            script: scenario.script("share"),
        }
    }

    /// The script.
    #[must_use]
    pub fn script(&self) -> &Script<InWindow<ShareRequest>, ShareResult> {
        &self.script
    }
}

impl ShareBackend for ScriptedShare {
    fn share(&self, window: &WindowRef, request: ShareRequest, answer: Resolver<ShareResult>) {
        let request = InWindow {
            window: window.id(),
            request,
        };
        self.script.receive(request, answer);
    }
}

/// Login items whose system is a [`Script`] of [`LoginItemRequest`]s.
#[derive(Clone, Debug)]
pub struct ScriptedLaunchAtLogin {
    script: Script<LoginItemRequest, LoginItemResult>,
}

impl ScriptedLaunchAtLogin {
    /// Login items whose script is on `scenario`, with nothing scripted.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        Self {
            script: scenario.script("launch at login"),
        }
    }

    /// The script.
    #[must_use]
    pub fn script(&self) -> &Script<LoginItemRequest, LoginItemResult> {
        &self.script
    }
}

impl LaunchAtLoginBackend for ScriptedLaunchAtLogin {
    fn ask(&self, request: LoginItemRequest, answer: Resolver<LoginItemResult>) {
        self.script.receive(request, answer);
    }
}

/// One registration a [`ScriptedShortcuts`] holds.
struct Registered {
    shortcut: Shortcut,
    presses: ShortcutPresses,
}

/// Global shortcuts whose system is a [`Script`] of [`Shortcut`]s: the
/// script answers each registration, and [`press`](Self::press) presses a
/// chord as the user would.
#[derive(Clone)]
pub struct ScriptedShortcuts {
    script: Script<Shortcut, Result<(), ShortcutError>>,
    /// The registrations not yet dropped, oldest first.
    registered: Rc<RefCell<Vec<Rc<Registered>>>>,
}

impl ScriptedShortcuts {
    /// Shortcuts whose script is on `scenario`, with nothing scripted.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        Self {
            script: scenario.script("global shortcut"),
            registered: Rc::default(),
        }
    }

    /// The script that answers registrations.
    #[must_use]
    pub fn script(&self) -> &Script<Shortcut, Result<(), ShortcutError>> {
        &self.script
    }

    /// Press `shortcut`: every registration of it that is still held hears
    /// the press at the owner's next turn. Returns how many did.
    pub fn press(&self, shortcut: Shortcut) -> usize {
        // Collected first: a press runs no listener inline, and the table's
        // borrow ends before any of it.
        let hearing = self
            .registered
            .borrow()
            .iter()
            .filter(|registered| registered.shortcut == shortcut)
            .map(Rc::clone)
            .collect::<Vec<Rc<Registered>>>();
        for registered in &hearing {
            registered.presses.press();
        }
        hearing.len()
    }

    /// How many registrations are held now.
    #[must_use]
    pub fn held(&self) -> usize {
        self.registered.borrow().len()
    }
}

/// Removes its registration from the double's table when dropped.
struct Unregister {
    registered: Rc<RefCell<Vec<Rc<Registered>>>>,
    entry: Rc<Registered>,
}

impl Drop for Unregister {
    fn drop(&mut self) {
        let removed = {
            let mut table = self.registered.borrow_mut();
            table
                .iter()
                .position(|held| Rc::ptr_eq(held, &self.entry))
                .map(|position| table.remove(position))
        };
        // The presses hold the listener: dropped with the borrow released.
        drop(removed);
    }
}

impl GlobalShortcutBackend for ScriptedShortcuts {
    fn register(
        &self,
        shortcut: Shortcut,
        presses: ShortcutPresses,
        answer: Resolver<Result<(), ShortcutError>>,
    ) -> ShortcutHold {
        let entry = Rc::new(Registered { shortcut, presses });
        self.registered.borrow_mut().push(Rc::clone(&entry));
        let unregister = Unregister {
            registered: Rc::clone(&self.registered),
            entry,
        };
        let ticket = self.script.receive_with_ticket(shortcut, answer);
        ShortcutHold::new((unregister, ticket))
    }
}

impl fmt::Debug for ScriptedShortcuts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptedShortcuts")
            .field("script", &self.script)
            .field("held", &self.held())
            .finish_non_exhaustive()
    }
}

/// One change a [`ScriptedTaskbar`] took.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum TaskbarChange {
    /// The window's progress.
    Progress {
        /// The window.
        window: WindowId,
        /// What it shows.
        progress: TaskbarProgress,
    },
    /// The window's badge.
    Badge {
        /// The window.
        window: WindowId,
        /// What it shows.
        badge: TaskbarBadge,
    },
}

/// A taskbar that records each change and takes it, or refuses every change
/// with the error [`refuse`](Self::refuse) set.
#[derive(Clone, Debug, Default)]
pub struct ScriptedTaskbar {
    changes: Rc<RefCell<Vec<TaskbarChange>>>,
    refusal: Rc<RefCell<Option<TaskbarError>>>,
}

impl ScriptedTaskbar {
    /// A taskbar that takes every change.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Refuse every later change with `error`, or take them again with
    /// `None`.
    pub fn refuse(&self, error: Option<TaskbarError>) {
        *self.refusal.borrow_mut() = error;
    }

    /// The changes taken so far, oldest first.
    #[must_use]
    pub fn changes(&self) -> Vec<TaskbarChange> {
        self.changes.borrow().clone()
    }

    fn take(&self, change: TaskbarChange) -> Result<(), TaskbarError> {
        if let Some(error) = *self.refusal.borrow() {
            return Err(error);
        }
        self.changes.borrow_mut().push(change);
        Ok(())
    }
}

impl TaskbarBackend for ScriptedTaskbar {
    fn set_progress(
        &self,
        window: &WindowRef,
        progress: TaskbarProgress,
    ) -> Result<(), TaskbarError> {
        self.take(TaskbarChange::Progress {
            window: window.id(),
            progress,
        })
    }

    fn set_badge(&self, window: &WindowRef, badge: TaskbarBadge) -> Result<(), TaskbarError> {
        self.take(TaskbarChange::Badge {
            window: window.id(),
            badge,
        })
    }
}

/// A jump list whose system is a [`Script`] of task lists.
#[derive(Clone, Debug)]
pub struct ScriptedJumpList {
    script: Script<Vec<JumpListTask>, JumpListResult>,
}

impl ScriptedJumpList {
    /// A jump list whose script is on `scenario`, with nothing scripted.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        Self {
            script: scenario.script("jump list"),
        }
    }

    /// The script.
    #[must_use]
    pub fn script(&self) -> &Script<Vec<JumpListTask>, JumpListResult> {
        &self.script
    }
}

impl JumpListBackend for ScriptedJumpList {
    fn set_tasks(&self, tasks: Vec<JumpListTask>, answer: Resolver<JumpListResult>) {
        self.script.receive(tasks, answer);
    }
}

/// An operating system whose folders a [`Script`] of [`KnownFolder`]s
/// answers, and which says it is the [`OsInfo`] it was made with.
#[derive(Clone, Debug)]
pub struct ScriptedOs {
    folders: Script<KnownFolder, FolderResult>,
    info: Rc<RefCell<OsInfo>>,
}

impl ScriptedOs {
    /// A system whose script is on `scenario`, with nothing scripted, that
    /// says it is "Scripted OS" 1.0.0 on x86-64 until
    /// [`set_info`](Self::set_info).
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        let version = OsVersion {
            major: 1,
            minor: 0,
            patch: 0,
        };
        Self {
            folders: scenario.script("known folder"),
            info: Rc::new(RefCell::new(OsInfo::new(
                "Scripted OS",
                version,
                Architecture::X86_64,
            ))),
        }
    }

    /// The script that answers folders.
    #[must_use]
    pub fn folders(&self) -> &Script<KnownFolder, FolderResult> {
        &self.folders
    }

    /// Say the system is `info` from now on.
    pub fn set_info(&self, info: OsInfo) {
        *self.info.borrow_mut() = info;
    }
}

impl OsBackend for ScriptedOs {
    fn folder(&self, folder: KnownFolder, answer: Resolver<FolderResult>) {
        self.folders.receive(folder, answer);
    }

    fn info(&self) -> OsInfo {
        self.info.borrow().clone()
    }
}

/// The system services' plugin over scripts: registers every handle of the
/// crate over its double, which the test keeps clones of.
#[derive(Clone, Debug)]
pub struct ScriptedSystem {
    launcher: ScriptedLauncher,
    dialogs: ScriptedFileDialogs,
    message_boxes: ScriptedMessageBoxes,
    share: ScriptedShare,
    login: ScriptedLaunchAtLogin,
    shortcuts: ScriptedShortcuts,
    taskbar: ScriptedTaskbar,
    jump_list: ScriptedJumpList,
    os: ScriptedOs,
}

impl Default for ScriptedSystem {
    fn default() -> Self {
        Self::on(&Scenario::new())
    }
}

impl ScriptedSystem {
    /// The plugin over `launcher` and `dialogs`, and the other doubles on a
    /// scenario of their own.
    #[must_use]
    pub fn new(launcher: ScriptedLauncher, dialogs: ScriptedFileDialogs) -> Self {
        Self {
            launcher,
            dialogs,
            ..Self::default()
        }
    }

    /// The plugin over doubles on `scenario`, with nothing scripted: the
    /// test scripts them through the accessors.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        Self {
            launcher: ScriptedLauncher::on(scenario),
            dialogs: ScriptedFileDialogs::on(scenario),
            message_boxes: ScriptedMessageBoxes::on(scenario),
            share: ScriptedShare::on(scenario),
            login: ScriptedLaunchAtLogin::on(scenario),
            shortcuts: ScriptedShortcuts::on(scenario),
            taskbar: ScriptedTaskbar::new(),
            jump_list: ScriptedJumpList::on(scenario),
            os: ScriptedOs::on(scenario),
        }
    }

    /// The launcher the plugin provides over.
    #[must_use]
    pub fn launcher(&self) -> &ScriptedLauncher {
        &self.launcher
    }

    /// The file dialogs the plugin provides over.
    #[must_use]
    pub fn dialogs(&self) -> &ScriptedFileDialogs {
        &self.dialogs
    }

    /// The message boxes the plugin provides over.
    #[must_use]
    pub fn message_boxes(&self) -> &ScriptedMessageBoxes {
        &self.message_boxes
    }

    /// The share sheet the plugin provides over.
    #[must_use]
    pub fn share(&self) -> &ScriptedShare {
        &self.share
    }

    /// The login items the plugin provides over.
    #[must_use]
    pub fn login(&self) -> &ScriptedLaunchAtLogin {
        &self.login
    }

    /// The global shortcuts the plugin provides over.
    #[must_use]
    pub fn shortcuts(&self) -> &ScriptedShortcuts {
        &self.shortcuts
    }

    /// The taskbar the plugin provides over.
    #[must_use]
    pub fn taskbar(&self) -> &ScriptedTaskbar {
        &self.taskbar
    }

    /// The jump list the plugin provides over.
    #[must_use]
    pub fn jump_list(&self) -> &ScriptedJumpList {
        &self.jump_list
    }

    /// The operating system the plugin provides over.
    #[must_use]
    pub fn os(&self) -> &ScriptedOs {
        &self.os
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
        let message_boxes = self.message_boxes.clone();
        registrar.provide::<MessageBoxCapability>(move |ctx: &ProviderContext<'_>| {
            MessageBoxes::try_new(message_boxes.clone(), ctx)
        });
        let share = self.share.clone();
        registrar.provide::<ShareCapability>(move |ctx: &ProviderContext<'_>| {
            Share::try_new(share.clone(), ctx)
        });
        let login = self.login.clone();
        registrar.provide::<LaunchAtLoginCapability>(move |ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(LaunchAtLogin::new(login.clone(), ctx))
        });
        let shortcuts = self.shortcuts.clone();
        registrar.provide::<GlobalShortcutCapability>(move |ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(GlobalShortcuts::new(shortcuts.clone(), ctx))
        });
        let taskbar = self.taskbar.clone();
        registrar.provide::<TaskbarCapability>(move |ctx: &ProviderContext<'_>| {
            Taskbar::try_new(taskbar.clone(), ctx)
        });
        let jump_list = self.jump_list.clone();
        registrar.provide::<JumpListCapability>(move |ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(JumpList::new(jump_list.clone(), ctx))
        });
        let os = self.os.clone();
        registrar.provide::<OsCapability>(move |ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Os::new(os.clone(), ctx))
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
