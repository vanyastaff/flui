//! The crate's conformance table (ADR-0154 §10): what every provider of the
//! system services keeps, run against the scripted doubles and against the
//! platform's own providers (the Windows shell, the Linux portal).
//!
//! A row asks only what no user has to answer and nothing on the machine
//! changes for: a refusal the seam owes, a window that closed, an entry's
//! shared state, what the system is and where the user's home is. Against
//! the platform's providers a service its host does not provide answers a
//! lasting refusal, which the row records and skips; a row that would show
//! a dialog, launch something, bind a key or write a start-up entry is not
//! here, and its run on a desktop is owed in the parity source.
//!
//! What the table proves of the platform's providers is narrow: its windows
//! are headless, with no native handle, and its requests are refused at the
//! seam, so beyond acquisition no request reaches a provider's OS code. The
//! exceptions read the system's name and the user's home folder, which
//! changes nothing. Each provider's own behaviour is tested beside it,
//! behind its OS seam, with the system's answers as inputs (`win32::*::tests`
//! on Windows, the portal's tests on a private bus on Linux).

use std::cell::RefCell;
use std::rc::Rc;

use flui_platform::capability::scripted::Scenario;
use flui_platform::capability::{Abandoned, Capability, Completion};
use flui_platform::launch::{LaunchError, LauncherCapability};
use flui_platform::{Unsupported, UnsupportedReason};
use flui_sdk::view::{CloseReason, LifecycleContext};
use flui_system::SystemPlugin;
use flui_system::autostart::{
    AutostartCapability, AutostartError, AutostartRequest, MalformedAutostart,
};
use flui_system::dialog::{
    DialogError, FileDialogCapability, MalformedRequest, MessageBoxCapability, MessageBoxRequest,
    OpenRequest, SaveRequest,
};
use flui_system::jump_list::{
    JumpList, JumpListCapability, JumpListError, JumpListTask, MalformedTask,
};
use flui_system::os::OsCapability;
use flui_system::share::{MalformedShare, ShareCapability, ShareError, ShareRequest};
use flui_system::shortcuts::{
    GlobalShortcutCapability, MalformedShortcut, ShortcutError, ShortcutEvent,
};
use flui_system::taskbar::{EntryState, Progress, TaskbarCapability};
use flui_system::testing::ScriptedSystem;
use flui_system::user_dirs::{UserDir, UserDirCapability};
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
    /// the platform's host refuses it for a lasting reason.
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

/// `acquired`, or `None` for a lasting refusal: not on this host yet, not
/// on this platform, or a declaration the headless host does not make (the
/// application's id, which a Linux launcher entry is named by). Any other
/// refusal fails.
fn lasting<H>(acquired: Result<H, Unsupported>) -> Option<H> {
    match acquired {
        Ok(handle) => Some(handle),
        Err(refusal) => {
            assert!(
                matches!(
                    refusal.reason,
                    UnsupportedReason::NotYetOnThisHost
                        | UnsupportedReason::NotOnThisPlatform
                        | UnsupportedReason::NotDeclared { .. }
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

/// Turn the owner until `slot` lands, for an answer a provider's thread
/// gives: ten seconds at most.
fn settle<T>(bench: &mut Bench, slot: &Slot<T>) -> Result<T, Abandoned> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while slot.borrow().is_none() {
        assert!(std::time::Instant::now() < deadline, "the answer came");
        turn(&mut bench.host);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    slot.borrow_mut()
        .take()
        .expect("BUG: the loop above saw it land")
}

/// Every service is provided, or refused for a lasting reason; on Windows
/// every one is provided.
fn each_service_is_provided_or_refused_for_a_lasting_reason(subject: Subject) {
    let mut bench = Bench::new(subject);
    let provided = [
        ("launcher", bench.handle::<LauncherCapability>().is_some()),
        (
            "file dialogs",
            bench.handle::<FileDialogCapability>().is_some(),
        ),
        (
            "message box",
            bench.handle::<MessageBoxCapability>().is_some(),
        ),
        ("share", bench.handle::<ShareCapability>().is_some()),
        ("autostart", bench.handle::<AutostartCapability>().is_some()),
        (
            "global shortcuts",
            bench.handle::<GlobalShortcutCapability>().is_some(),
        ),
        (
            "launcher entry",
            bench.handle::<TaskbarCapability>().is_some(),
        ),
        (
            "user folders",
            bench.handle::<UserDirCapability>().is_some(),
        ),
        ("jump list", bench.handle::<JumpListCapability>().is_some()),
        (
            "system information",
            bench.handle::<OsCapability>().is_some(),
        ),
    ];
    if matches!(subject, Subject::Scripted) || cfg!(windows) {
        assert!(
            provided.iter().all(|(_, provided)| *provided),
            "{provided:?}"
        );
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
    let autostart = bench
        .handle::<AutostartCapability>()
        .map(|autostart| land(autostart.request(AutostartRequest::enable().reason(" "))));
    let binding = bench
        .handle::<GlobalShortcutCapability>()
        .map(|shortcuts| shortcuts.bind(Vec::new()));
    let bound = binding.as_ref().map(|binding| land(binding.next()));
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
    if let Some(autostart) = autostart {
        assert_eq!(
            *autostart.borrow(),
            Some(Ok(Err(AutostartError::InvalidRequest(
                MalformedAutostart::Reason
            ))))
        );
    }
    if let Some(bound) = bound {
        assert_eq!(
            *bound.borrow(),
            Some(Ok(ShortcutEvent::Ended(ShortcutError::InvalidRequest(
                MalformedShortcut::Count
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
    );
    let into = slots.clone();
    within(
        &mut bench.host,
        Some(window),
        move |ctx: &dyn LifecycleContext| {
            into.0.fill(ctx);
            into.1.fill(ctx);
            into.2.fill(ctx);
        },
    );
    let dialogs = lasting(slots.0.take());
    let boxes = lasting(slots.1.take());
    let share = lasting(slots.2.take());
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
    bench.verify();
}

/// The launcher entry is the application's: what one handle sets, another
/// handle of the runtime reads, and setting it again changes nothing. On
/// the platform no headless window has a button, so nothing is shown.
fn the_launcher_entry_handles_share_one_state(subject: Subject) {
    let mut bench = Bench::new(subject);
    let (Some(one), Some(other)) = (
        bench.handle::<TaskbarCapability>(),
        bench.handle::<TaskbarCapability>(),
    ) else {
        return;
    };
    one.set_count(Some(3));
    other.set_progress(Progress::Indeterminate);
    let expected = EntryState::new(Progress::Indeterminate, Some(3));
    assert_eq!(one.state(), expected);
    assert_eq!(other.state(), expected);
    bench.verify();
}

/// The OS says what it is, and the user's home folder is an absolute path
/// that exists. A scripted subject answers what its script says, which the
/// services' tests check; here it is asked nothing.
fn the_os_names_itself_and_the_users_home(subject: Subject) {
    if matches!(subject, Subject::Scripted) {
        return;
    }
    let mut bench = Bench::new(subject);
    if let Some(os) = bench.handle::<OsCapability>() {
        let info = os.info();
        assert_ne!(info.name, "");
        assert!(info.version.major > 0, "{info:?}");
    }
    let Some(dirs) = bench.handle::<UserDirCapability>() else {
        return;
    };
    let home = land(dirs.locate(UserDir::Home));
    let path = settle(&mut bench, &home)
        .expect("not abandoned")
        .expect("the system names folders")
        .expect("the user has a home");
    assert!(path.is_absolute() && path.is_dir(), "{}", path.display());
}

/// A row of the table: its name and what it checks of a subject.
type Row = (&'static str, fn(Subject));

/// The table: each row, run against each subject.
#[test]
fn every_provider_keeps_the_system_services_contract() {
    let rows: [Row; 6] = [
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
            "the_launcher_entry_handles_share_one_state",
            the_launcher_entry_handles_share_one_state,
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
