//! The crate's conformance table (ADR-0154 §10): what every provider of the
//! system services keeps, run against the scripted doubles and against the
//! platform's own providers.
//!
//! A row asks only what no user has to answer and nothing on the machine
//! changes for: a refusal the seam owes, a window that closed, what the
//! system is. Against the platform's providers a service its host does not
//! provide yet answers a lasting refusal, which the row records and skips;
//! a row that would show a dialog or launch something is not here, and its
//! run on a desktop is owed in the parity source.
//!
//! What the table proves of the platform's providers is narrow: its windows
//! are headless, with no native handle, and its requests are refused at the
//! seam, so beyond acquisition no request reaches a provider's OS code. The
//! one exception reads the system's name and the user's home folder, which
//! changes nothing. Each Windows provider's own behaviour is tested beside
//! it, behind its OS seam, with the system's answers as inputs
//! (`win32::*::tests`).

use std::cell::RefCell;
use std::rc::Rc;

use flui_platform::capability::scripted::Scenario;
use flui_platform::capability::{Abandoned, Capability, Completion};
use flui_platform::launch::{LaunchError, LauncherCapability};
use flui_platform::{Unsupported, UnsupportedReason};
use flui_sdk::view::{CloseReason, LifecycleContext};
use flui_system::SystemPlugin;
use flui_system::dialog::{
    DialogError, FileDialogCapability, MalformedRequest, MessageBoxCapability, MessageBoxRequest,
    OpenRequest, SaveRequest,
};
use flui_system::jump_list::{
    JumpList, JumpListCapability, JumpListError, JumpListTask, MalformedTask,
};
use flui_system::login::LaunchAtLoginCapability;
use flui_system::os::{KnownFolder, OsCapability};
use flui_system::share::{MalformedShare, ShareCapability, ShareError, ShareRequest};
use flui_system::shortcut::GlobalShortcutCapability;
use flui_system::taskbar::{TaskbarCapability, TaskbarError, TaskbarProgress};
use flui_system::testing::ScriptedSystem;
use flui_testing::{HeadlessHost, HeadlessWindow};

use crate::{CapabilitySlot, acquire_in, outcome_slot, turn, within};

/// Whose providers a row runs against.
#[derive(Clone, Copy, Debug)]
enum Subject {
    /// The scripted doubles, with nothing scripted: a row that reaches the
    /// system fails the scenario.
    Scripted,
    /// The platform's own providers ([`SystemPlugin`]).
    Platform,
}

/// One subject's host, and the scenario a scripted subject runs on.
struct Bench {
    host: HeadlessHost,
    scenario: Option<Scenario>,
}

impl Bench {
    fn new(subject: Subject) -> Self {
        match subject {
            Subject::Scripted => {
                let scenario = Scenario::new();
                let plugin = ScriptedSystem::on(&scenario);
                let host =
                    HeadlessHost::with_scenario(HeadlessWindow::new(10, 10), &scenario, &plugin);
                Self {
                    host,
                    scenario: Some(scenario),
                }
            }
            Subject::Platform => Self {
                host: HeadlessHost::with_plugin(HeadlessWindow::new(10, 10), &SystemPlugin::new()),
                scenario: None,
            },
        }
    }

    /// The handle of `C`, acquired in a window of its own, or `None` when
    /// the platform's host refuses it for a lasting reason: not on this
    /// host yet, or not on this platform.
    fn handle<C: Capability>(&mut self) -> Option<C::Handle> {
        let window = self.host.open_window(HeadlessWindow::new(10, 10));
        lasting(acquire_in::<C>(&mut self.host, window))
    }

    /// Nothing reached a scripted system that no step answered.
    fn verify(&self) {
        if let Some(scenario) = &self.scenario {
            scenario.verify().expect("no row reaches the system");
        }
    }
}

/// `acquired`, or `None` for a lasting refusal; any other refusal fails.
fn lasting<H>(acquired: Result<H, Unsupported>) -> Option<H> {
    match acquired {
        Ok(handle) => Some(handle),
        Err(refusal) => {
            assert!(
                matches!(
                    refusal.reason,
                    UnsupportedReason::NotYetOnThisHost | UnsupportedReason::NotOnThisPlatform
                ),
                "a provider is there or refused for a lasting reason: {refusal:?}"
            );
            None
        }
    }
}

type Slot<T> = Rc<RefCell<Option<Result<T, Abandoned>>>>;

fn land<T: 'static>(completion: Completion<T>) -> Slot<T> {
    let slot = outcome_slot::<Result<T, Abandoned>>();
    let into = Rc::clone(&slot);
    completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    slot
}

fn land_dialog<T: 'static>(dialog: flui_system::dialog::Dialog<T>) -> Slot<T> {
    let slot = outcome_slot::<Result<T, Abandoned>>();
    let into = Rc::clone(&slot);
    dialog.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    slot
}

/// Every service is provided, or refused for a lasting reason; on Windows
/// every one is provided.
fn each_service_is_provided_or_refused_for_a_lasting_reason(subject: Subject) {
    let mut bench = Bench::new(subject);
    let provided = [
        bench.handle::<LauncherCapability>().is_some(),
        bench.handle::<FileDialogCapability>().is_some(),
        bench.handle::<MessageBoxCapability>().is_some(),
        bench.handle::<ShareCapability>().is_some(),
        bench.handle::<LaunchAtLoginCapability>().is_some(),
        bench.handle::<GlobalShortcutCapability>().is_some(),
        bench.handle::<TaskbarCapability>().is_some(),
        bench.handle::<JumpListCapability>().is_some(),
        bench.handle::<OsCapability>().is_some(),
    ];
    if matches!(subject, Subject::Scripted) || cfg!(windows) {
        assert!(provided.iter().all(|provided| *provided), "{provided:?}");
    }
    bench.verify();
}

/// A refusal the seam owes settles at an owner turn, never inside the call.
fn a_refusal_settles_at_an_owner_turn_never_inside_the_call(subject: Subject) {
    let mut bench = Bench::new(subject);
    let Some(launcher) = bench.handle::<LauncherCapability>() else {
        return;
    };
    let relative = land(launcher.open_path(std::path::Path::new("relative.pdf")));
    assert!(relative.borrow().is_none(), "never inside the call");
    turn(&mut bench.host);
    assert_eq!(*relative.borrow(), Some(Ok(Err(LaunchError::RelativePath))));
    bench.verify();
}

/// A malformed request is refused without asking the system, for every
/// service that takes one.
fn a_malformed_request_is_refused_without_asking_the_system(subject: Subject) {
    let mut bench = Bench::new(subject);
    let dialog = bench
        .handle::<FileDialogCapability>()
        .map(|dialogs| land_dialog(dialogs.save(SaveRequest::new().suggested_name("dir/name"))));
    let message = bench
        .handle::<MessageBoxCapability>()
        .map(|boxes| land_dialog(boxes.show(MessageBoxRequest::new(""))));
    let share = bench
        .handle::<ShareCapability>()
        .map(|share| land(share.share(ShareRequest::new())));
    let jump_list = bench.handle::<JumpListCapability>().map(|jump_list| {
        land(
            jump_list.set_tasks(
                (0..=JumpList::MAX_TASKS)
                    .map(|index| JumpListTask::new(format!("T{index}")))
                    .collect::<Vec<JumpListTask>>(),
            ),
        )
    });
    turn(&mut bench.host);
    if let Some(dialog) = dialog {
        assert_eq!(
            *dialog.borrow(),
            Some(Ok(Err(DialogError::InvalidRequest(
                MalformedRequest::SuggestedName
            ))))
        );
    }
    if let Some(message) = message {
        assert_eq!(
            *message.borrow(),
            Some(Ok(Err(DialogError::InvalidRequest(
                MalformedRequest::EmptyMessage
            ))))
        );
    }
    if let Some(share) = share {
        assert_eq!(
            *share.borrow(),
            Some(Ok(Err(ShareError::InvalidRequest(MalformedShare::Empty))))
        );
    }
    if let Some(jump_list) = jump_list {
        assert_eq!(
            *jump_list.borrow(),
            Some(Ok(Err(JumpListError::InvalidTasks(
                MalformedTask::TooManyTasks
            ))))
        );
    }
    bench.verify();
}

/// The handles of a closed window answer that it closed, and never ask the
/// system.
fn a_closed_windows_handles_never_ask_the_system(subject: Subject) {
    let mut bench = Bench::new(subject);
    let window = bench.host.open_window(HeadlessWindow::new(10, 10));
    let slots = (
        CapabilitySlot::<FileDialogCapability>::new(),
        CapabilitySlot::<MessageBoxCapability>::new(),
        CapabilitySlot::<ShareCapability>::new(),
        CapabilitySlot::<TaskbarCapability>::new(),
    );
    let into = slots.clone();
    within(
        &mut bench.host,
        Some(window),
        move |ctx: &dyn LifecycleContext| {
            into.0.fill(ctx);
            into.1.fill(ctx);
            into.2.fill(ctx);
            into.3.fill(ctx);
        },
    );
    let dialogs = lasting(slots.0.take());
    let boxes = lasting(slots.1.take());
    let share = lasting(slots.2.take());
    let taskbar = lasting(slots.3.take());
    assert!(bench.host.request_close(window, CloseReason::User));
    turn(&mut bench.host);

    let opened = dialogs.map(|dialogs| land_dialog(dialogs.open(OpenRequest::files())));
    let shown = boxes.map(|boxes| land_dialog(boxes.show(MessageBoxRequest::new("Saved"))));
    let shared = share.map(|share| land(share.share(ShareRequest::new().text("x"))));
    turn(&mut bench.host);
    if let Some(opened) = opened {
        assert_eq!(*opened.borrow(), Some(Ok(Err(DialogError::WindowClosed))));
    }
    if let Some(shown) = shown {
        assert_eq!(*shown.borrow(), Some(Ok(Err(DialogError::WindowClosed))));
    }
    if let Some(shared) = shared {
        assert_eq!(*shared.borrow(), Some(Ok(Err(ShareError::WindowClosed))));
    }
    if let Some(taskbar) = taskbar {
        assert_eq!(
            taskbar.set_progress(TaskbarProgress::Indeterminate),
            Err(TaskbarError::WindowClosed)
        );
    }
    bench.verify();
}

/// The OS says what it is, and the user's home folder is an absolute path
/// that exists. A scripted subject answers what its script says, which
/// [`services`](crate::services) checks; here it is asked nothing.
fn the_os_names_itself_and_the_users_home(subject: Subject) {
    if matches!(subject, Subject::Scripted) {
        return;
    }
    let mut bench = Bench::new(subject);
    let Some(os) = bench.handle::<OsCapability>() else {
        return;
    };
    let info = os.info();
    assert_ne!(info.name, "");
    assert!(info.version.major > 0, "{info:?}");
    let home = land(os.folder(KnownFolder::Home));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while home.borrow().is_none() {
        assert!(std::time::Instant::now() < deadline, "the folder answered");
        turn(&mut bench.host);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let answered = home.borrow_mut().take();
    let path = answered
        .expect("answered")
        .expect("not abandoned")
        .expect("the user has a home");
    assert!(path.is_absolute() && path.is_dir(), "{}", path.display());
}

/// A row of the table: its name and what it checks of a subject.
type Row = (&'static str, fn(Subject));

/// The table: each row, run against each subject.
#[test]
fn every_provider_keeps_the_system_services_contract() {
    let rows: [Row; 5] = [
        (
            "each_service_is_provided_or_refused_for_a_lasting_reason",
            each_service_is_provided_or_refused_for_a_lasting_reason,
        ),
        (
            "a_refusal_settles_at_an_owner_turn_never_inside_the_call",
            a_refusal_settles_at_an_owner_turn_never_inside_the_call,
        ),
        (
            "a_malformed_request_is_refused_without_asking_the_system",
            a_malformed_request_is_refused_without_asking_the_system,
        ),
        (
            "a_closed_windows_handles_never_ask_the_system",
            a_closed_windows_handles_never_ask_the_system,
        ),
        (
            "the_os_names_itself_and_the_users_home",
            the_os_names_itself_and_the_users_home,
        ),
    ];
    for subject in [Subject::Scripted, Subject::Platform] {
        for (name, row) in rows {
            // Named before it runs, so a failing row's panic follows its
            // name in the output.
            eprintln!("row {name} against {subject:?}");
            row(subject);
        }
    }
}
