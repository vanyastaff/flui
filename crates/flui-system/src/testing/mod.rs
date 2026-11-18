//! The scripted system services a scenario registers instead of the OS's
//! (ADR-0154 §10, ADR-0243): [`ScriptedSystem`], over a
//! [`ScriptedLauncher`], [`ScriptedFileDialogs`], [`ScriptedMessageBoxes`],
//! [`ScriptedShare`], [`ScriptedAutostart`], [`ScriptedUserDirs`] and
//! [`ScriptedJumpList`] on the scenario primitive
//! ([`flui_platform::capability::scripted`]), and a [`ScriptedShortcuts`],
//! a [`ScriptedTaskbar`] and a [`ScriptedOs`] the test speaks for directly.
//!
//! A double answers what its script says and nothing else: a request no
//! step answers is the scenario's
//! [`Unexpected`](flui_platform::capability::scripted::ScenarioFailure::Unexpected)
//! failure, and its completion ends `ProviderDropped`, never with a success
//! nobody wrote.

#[cfg(test)]
pub(crate) mod android;
#[cfg(test)]
pub(crate) mod owner;
#[cfg(all(test, windows))]
#[expect(
    unsafe_code,
    reason = "a raw window handle naming a window no test reaches, a message pump"
)]
pub(crate) mod win32;
#[cfg(test)]
pub(crate) mod window;

mod grants;

use std::{cell::Cell, cell::RefCell, fmt, rc::Rc};

use flui_platform::Unsupported;
use flui_platform::WindowId;
use flui_platform::capability::scripted::{Answer, Scenario, Script};
use flui_platform::capability::{
    CapabilityRegistrar, Plugin, ProviderContext, Resolver, WindowRef,
};
use flui_platform::launch::{Launcher, LauncherCapability};
pub use flui_platform::testing::{LaunchRequest, ScriptedLauncher};

use crate::autostart::{
    Autostart, AutostartBackend, AutostartCapability, AutostartRequest, AutostartResult,
};
use crate::dialog::{
    DialogHold, FileDialogBackend, FileDialogCapability, FileDialogs, MessageBoxBackend,
    MessageBoxCapability, MessageBoxRequest, MessageBoxResult, MessageBoxes, OpenRequest,
    OpenResult, SaveRequest, SaveResult,
};
use crate::grant::{GrantCapability, Grants};
use crate::jump_list::{
    JumpList, JumpListBackend, JumpListCapability, JumpListResult, JumpListTask,
};
use crate::os::{Architecture, Os, OsBackend, OsCapability, OsInfo, OsVersion};
use crate::share::{Share, ShareBackend, ShareCapability, ShareRequest, ShareResult};
use crate::shortcuts::{
    GlobalShortcutCapability, GlobalShortcuts, Shortcut, ShortcutBackend, ShortcutEvent,
    ShortcutFeed, ShortcutHold,
};
use crate::taskbar::{EntryState, Honoured, TaskbarBackend, TaskbarCapability, TaskbarEntry};
use crate::user_dirs::{UserDir, UserDirBackend, UserDirCapability, UserDirResult, UserDirs};
pub use grants::{GrantStore, ScriptedGrants};

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

/// Autostart whose system is a script: each request is answered by the
/// [`Script`] in the order asked. Cheap to clone; clones share the script.
#[derive(Clone, Debug)]
pub struct ScriptedAutostart {
    script: Script<AutostartRequest, AutostartResult>,
}

impl Default for ScriptedAutostart {
    fn default() -> Self {
        Self::on(&Scenario::new())
    }
}

impl ScriptedAutostart {
    /// Autostart whose script is on `scenario`, with nothing scripted.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        Self {
            script: scenario.script("autostart"),
        }
    }

    /// The requests' script.
    #[must_use]
    pub fn script(&self) -> &Script<AutostartRequest, AutostartResult> {
        &self.script
    }
}

impl AutostartBackend for ScriptedAutostart {
    fn request(&self, request: AutostartRequest, answer: Resolver<AutostartResult>) {
        self.script.receive(request, answer);
    }
}

/// User folders whose system is a script: each lookup is answered by the
/// [`Script`]. Cheap to clone; clones share the script.
#[derive(Clone, Debug)]
pub struct ScriptedUserDirs {
    script: Script<UserDir, UserDirResult>,
}

impl Default for ScriptedUserDirs {
    fn default() -> Self {
        Self::on(&Scenario::new())
    }
}

impl ScriptedUserDirs {
    /// User folders whose script is on `scenario`, with nothing scripted.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        Self {
            script: scenario.script("user folders"),
        }
    }

    /// The lookups' script.
    #[must_use]
    pub fn script(&self) -> &Script<UserDir, UserDirResult> {
        &self.script
    }
}

impl UserDirBackend for ScriptedUserDirs {
    fn locate(&self, dir: UserDir, answer: Resolver<UserDirResult>) {
        self.script.receive(dir, answer);
    }
}

/// A launcher entry that records what it was asked to show, honouring what
/// the test says. Cheap to clone; clones share the record.
#[derive(Clone, Debug)]
pub struct ScriptedTaskbar {
    shown: Rc<RefCell<Vec<EntryState>>>,
    honours: Rc<Cell<Honoured>>,
}

impl Default for ScriptedTaskbar {
    fn default() -> Self {
        Self {
            shown: Rc::default(),
            honours: Rc::new(Cell::new(Honoured::new(true, true, true, true))),
        }
    }
}

impl ScriptedTaskbar {
    /// Honour `honours` from now on; every part, confirmed, until then.
    pub fn set_honours(&self, honours: Honoured) {
        self.honours.set(honours);
    }

    /// Every state the entry was asked to show, oldest first.
    #[must_use]
    pub fn shown(&self) -> Vec<EntryState> {
        self.shown.borrow().clone()
    }
}

impl TaskbarBackend for ScriptedTaskbar {
    fn show(&self, state: EntryState) {
        self.shown.borrow_mut().push(state);
    }

    fn honours(&self) -> Honoured {
        self.honours.get()
    }
}

/// One binding a [`ScriptedShortcuts`] was asked for: the shortcuts, where
/// the test sends what the system says, and whether the binding still holds.
#[derive(Clone, Debug)]
pub struct ScriptedBinding {
    shortcuts: Vec<Shortcut>,
    feed: ShortcutFeed,
    held: Rc<Cell<bool>>,
}

impl ScriptedBinding {
    /// The shortcuts it asked for.
    #[must_use]
    pub fn shortcuts(&self) -> &[Shortcut] {
        &self.shortcuts
    }

    /// Send `event` as the system.
    pub fn send(&self, event: ShortcutEvent) {
        self.feed.send(event);
    }

    /// Whether the application still holds the binding.
    #[must_use]
    pub fn is_held(&self) -> bool {
        self.held.get()
    }
}

/// Global shortcuts whose system is the test: each binding is recorded
/// ([`bindings`](Self::bindings)), and the test sends what the system says
/// through it. Cheap to clone; clones share the record.
#[derive(Clone, Debug, Default)]
pub struct ScriptedShortcuts {
    bindings: Rc<RefCell<Vec<ScriptedBinding>>>,
}

impl ScriptedShortcuts {
    /// The bindings asked for, oldest first.
    #[must_use]
    pub fn bindings(&self) -> Vec<ScriptedBinding> {
        self.bindings.borrow().clone()
    }
}

/// Marks its binding let go when dropped.
struct Unbind(Rc<Cell<bool>>);

impl Drop for Unbind {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl ShortcutBackend for ScriptedShortcuts {
    fn bind(&self, shortcuts: Vec<Shortcut>, feed: ShortcutFeed) -> ShortcutHold {
        let held = Rc::new(Cell::new(true));
        self.bindings.borrow_mut().push(ScriptedBinding {
            shortcuts,
            feed,
            held: Rc::clone(&held),
        });
        ShortcutHold::new(Unbind(held))
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

/// A system that says it is the [`OsInfo`] the test set: "Scripted OS"
/// 1.0.0 on x86-64 until [`set_info`](Self::set_info). Cheap to clone;
/// clones share the answer.
#[derive(Clone, Debug)]
pub struct ScriptedOs {
    info: Rc<RefCell<OsInfo>>,
}

impl Default for ScriptedOs {
    fn default() -> Self {
        let version = OsVersion {
            major: 1,
            minor: 0,
            patch: 0,
        };
        Self {
            info: Rc::new(RefCell::new(OsInfo::new(
                "Scripted OS",
                version,
                Architecture::X86_64,
            ))),
        }
    }
}

impl ScriptedOs {
    /// Say the system is `info` from now on.
    pub fn set_info(&self, info: OsInfo) {
        *self.info.borrow_mut() = info;
    }
}

impl OsBackend for ScriptedOs {
    fn info(&self) -> OsInfo {
        self.info.borrow().clone()
    }
}

/// The system services' plugin over a script: registers each handle over
/// the scripted system the test keeps clones of.
#[derive(Clone, Debug)]
pub struct ScriptedSystem {
    launcher: ScriptedLauncher,
    dialogs: ScriptedFileDialogs,
    message_boxes: ScriptedMessageBoxes,
    share: ScriptedShare,
    autostart: ScriptedAutostart,
    shortcuts: ScriptedShortcuts,
    taskbar: ScriptedTaskbar,
    user_dirs: ScriptedUserDirs,
    jump_list: ScriptedJumpList,
    os: ScriptedOs,
    grants: ScriptedGrants,
}

impl Default for ScriptedSystem {
    fn default() -> Self {
        Self::on(&Scenario::new())
    }
}

impl ScriptedSystem {
    /// The plugin over `launcher` and `dialogs`, with the other services
    /// unscripted.
    #[must_use]
    pub fn new(launcher: ScriptedLauncher, dialogs: ScriptedFileDialogs) -> Self {
        Self {
            launcher,
            dialogs,
            ..Self::default()
        }
    }

    /// The plugin over services on `scenario`, with nothing scripted: the
    /// test scripts them through the accessors.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        Self {
            launcher: ScriptedLauncher::on(scenario),
            dialogs: ScriptedFileDialogs::on(scenario),
            message_boxes: ScriptedMessageBoxes::on(scenario),
            share: ScriptedShare::on(scenario),
            autostart: ScriptedAutostart::on(scenario),
            shortcuts: ScriptedShortcuts::default(),
            taskbar: ScriptedTaskbar::default(),
            user_dirs: ScriptedUserDirs::on(scenario),
            jump_list: ScriptedJumpList::on(scenario),
            os: ScriptedOs::default(),
            grants: ScriptedGrants::default(),
        }
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

    /// The autostart the plugin provides over.
    #[must_use]
    pub fn autostart(&self) -> &ScriptedAutostart {
        &self.autostart
    }

    /// The global shortcuts the plugin provides over.
    #[must_use]
    pub fn shortcuts(&self) -> &ScriptedShortcuts {
        &self.shortcuts
    }

    /// The launcher entry the plugin provides over.
    #[must_use]
    pub fn taskbar(&self) -> &ScriptedTaskbar {
        &self.taskbar
    }

    /// The user folders the plugin provides over.
    #[must_use]
    pub fn user_dirs(&self) -> &ScriptedUserDirs {
        &self.user_dirs
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

    /// The grants the plugin provides over.
    #[must_use]
    pub fn grants(&self) -> &ScriptedGrants {
        &self.grants
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
        let autostart = self.autostart.clone();
        registrar.provide::<AutostartCapability>(move |ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Autostart::new(autostart.clone(), ctx))
        });
        let shortcuts = self.shortcuts.clone();
        registrar.provide::<GlobalShortcutCapability>(move |ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(GlobalShortcuts::new(shortcuts.clone(), ctx))
        });
        let taskbar = self.taskbar.clone();
        registrar.provide::<TaskbarCapability>(move |ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(TaskbarEntry::new(taskbar.clone(), ctx))
        });
        let user_dirs = self.user_dirs.clone();
        registrar.provide::<UserDirCapability>(move |ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(UserDirs::new(user_dirs.clone(), ctx))
        });
        let jump_list = self.jump_list.clone();
        registrar.provide::<JumpListCapability>(move |ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(JumpList::new(jump_list.clone(), ctx))
        });
        let os = self.os.clone();
        registrar.provide::<OsCapability>(move |ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Os::new(os.clone(), ctx))
        });
        let grants = self.grants.clone();
        registrar.provide::<GrantCapability>(move |ctx: &ProviderContext<'_>| {
            Ok::<_, Unsupported>(Grants::new(grants.clone(), grants.sandbox(), ctx))
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
    app_id: Option<crate::portal::AppName>,
}

#[cfg(target_os = "linux")]
impl PortalOnBus {
    /// The providers on the bus at `address`, a D-Bus address such as
    /// `unix:path=/tmp/bus`, naming the application as the composition root
    /// does.
    #[must_use]
    pub fn new(address: impl Into<std::sync::Arc<str>>) -> Self {
        Self {
            address: address.into(),
            app_id: None,
        }
    }

    /// Name the application `app_id` to the portal, as a composition root
    /// that declares it does.
    #[must_use]
    pub fn app_id(mut self, app_id: &str) -> Self {
        self.app_id = Some(crate::portal::AppName::scripted(app_id));
        self
    }
}

#[cfg(target_os = "linux")]
impl Plugin for PortalOnBus {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        crate::portal::register(
            registrar,
            Some(std::sync::Arc::clone(&self.address)),
            self.app_id.clone(),
        );
    }
}
