//! The services beyond launching and file dialogs, through their handles on
//! the scripted doubles: the message box, sharing, launch at login, global
//! shortcuts, the taskbar, the jump list and the OS.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use flui_platform::capability::Abandoned;
use flui_platform::capability::scripted::{Answer, Expect, Scenario};
use flui_platform::keyboard::{Code, Modifiers};
use flui_platform::launch::{ExternalUri, Scheme};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
use flui_sdk::view::{CloseReason, LifecycleContext};
use flui_system::dialog::{
    DialogError, FileDialogCapability, MAX_TEXT_LEN, MalformedRequest, MessageAnswer,
    MessageBoxCapability, MessageBoxRequest, MessageBoxResult, MessageButtons, MessageLevel,
    OpenRequest,
};
use flui_system::jump_list::{
    JumpList, JumpListCapability, JumpListError, JumpListResult, JumpListTask, MalformedTask,
};
use flui_system::login::{
    LaunchAtLoginCapability, LoginItemRequest, LoginItemResult, LoginItemState,
};
use flui_system::os::{
    Architecture, FolderError, FolderResult, KnownFolder, OsCapability, OsInfo, OsVersion,
};
use flui_system::share::{MalformedShare, ShareCapability, ShareError, ShareRequest, ShareResult};
use flui_system::shortcut::{
    GlobalShortcutCapability, InvalidShortcut, Shortcut, ShortcutError, ShortcutResult,
};
use flui_system::taskbar::{
    Fraction, TaskbarBadge, TaskbarCapability, TaskbarError, TaskbarProgress,
};
use flui_system::testing::{InWindow, ScriptedSystem, TaskbarChange};
use flui_testing::{HeadlessHost, HeadlessWindow};

use crate::{CapabilitySlot, acquire_in, acquire_on, outcome_slot, turn, within};

type Slot<T> = Rc<RefCell<Option<Result<T, Abandoned>>>>;

fn land_completion<T: 'static>(completion: flui_platform::capability::Completion<T>) -> Slot<T> {
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

fn host_on(scenario: &Scenario) -> (HeadlessHost, ScriptedSystem) {
    let plugin = ScriptedSystem::on(scenario);
    let host = HeadlessHost::with_scenario(HeadlessWindow::new(10, 10), scenario, &plugin);
    (host, plugin)
}

#[test]
fn a_message_box_shares_its_windows_one_dialog_rule_and_answers_only_its_buttons() {
    let scenario = Scenario::new();
    let (mut host, plugin) = host_on(&scenario);
    let slots = (
        CapabilitySlot::<MessageBoxCapability>::new(),
        CapabilitySlot::<FileDialogCapability>::new(),
    );
    let into = slots.clone();
    within(&mut host, None, move |ctx: &dyn LifecycleContext| {
        into.0.fill(ctx);
        into.1.fill(ctx);
    });
    let boxes = slots.0.take().expect("scripted boxes");
    let files = slots.1.take().expect("scripted dialogs");

    plugin.dialogs().hold();
    let open = land_dialog(files.open(OpenRequest::files()));
    let busy = land_dialog(boxes.show(MessageBoxRequest::new("Saved")));
    turn(&mut host);
    assert_eq!(
        *busy.borrow(),
        Some(Ok(Err(DialogError::Busy))),
        "a window shows one dialog, whichever handle asked"
    );
    assert!(plugin.dialogs().release_open(Ok(None)));
    turn(&mut host);
    assert_eq!(*open.borrow(), Some(Ok(Ok(None))));

    let question = MessageBoxRequest::new("Delete the note?")
        .title("Notes")
        .level(MessageLevel::Warning)
        .buttons(MessageButtons::YesNo);
    plugin.message_boxes().script().expect(
        Expect::when("the question, in the window", {
            let question = question.clone();
            move |asked: &InWindow<MessageBoxRequest>| asked.request == question
        }),
        Answer::value(Ok(MessageAnswer::Yes)),
    );
    let answered = land_dialog(boxes.show(question.clone()));
    turn(&mut host);
    assert_eq!(*answered.borrow(), Some(Ok(Ok(MessageAnswer::Yes))));

    // A cancel the box does not offer is outside the system's contract.
    plugin
        .message_boxes()
        .script()
        .answer_next(Answer::value(Ok(MessageAnswer::Cancel)));
    let outside = land_dialog(boxes.show(question));
    turn(&mut host);
    assert_eq!(
        *outside.borrow(),
        Some(Ok(Err(DialogError::Failed(SystemFailure::MalformedAnswer))))
    );

    let refused: Vec<(MessageBoxRequest, MalformedRequest)> = vec![
        (MessageBoxRequest::new("  "), MalformedRequest::EmptyMessage),
        (MessageBoxRequest::new("a\0b"), MalformedRequest::Text),
        (
            MessageBoxRequest::new("m".repeat(MAX_TEXT_LEN + 1)),
            MalformedRequest::Text,
        ),
        (
            MessageBoxRequest::new("ok").title("two\nlines"),
            MalformedRequest::Text,
        ),
    ];
    let slots = refused
        .into_iter()
        .map(|(request, reason)| (land_dialog(boxes.show(request)), reason))
        .collect::<Vec<(Slot<MessageBoxResult>, MalformedRequest)>>();
    // A message keeps its line breaks.
    plugin
        .message_boxes()
        .script()
        .answer_next(Answer::value(Ok(MessageAnswer::Ok)));
    let multiline = land_dialog(boxes.show(MessageBoxRequest::new("line one\nline two")));
    turn(&mut host);
    for (slot, reason) in slots {
        assert_eq!(
            *slot.borrow(),
            Some(Ok(Err(DialogError::InvalidRequest(reason)))),
            "{reason:?}"
        );
    }
    assert_eq!(*multiline.borrow(), Some(Ok(Ok(MessageAnswer::Ok))));
    scenario
        .verify()
        .expect("only the scripted boxes reached the system");
}

#[test]
fn a_message_box_ends_with_its_window() {
    let scenario = Scenario::new();
    let (mut host, plugin) = host_on(&scenario);
    let window = host.open_window(HeadlessWindow::new(10, 10));
    let boxes = acquire_in::<MessageBoxCapability>(&mut host, window).expect("scripted boxes");
    plugin.message_boxes().script().expect_held(Expect::any());
    let held = land_dialog(boxes.show(MessageBoxRequest::new("Working…")));
    turn(&mut host);
    assert!(held.borrow().is_none());
    assert!(host.request_close(window, CloseReason::User));
    turn(&mut host);
    assert_eq!(*held.borrow(), Some(Ok(Err(DialogError::WindowClosed))));
    assert_eq!(
        plugin.message_boxes().script().withdrawn(),
        1,
        "the box was taken down"
    );
}

#[test]
fn a_share_reaches_its_windows_sheet_and_a_malformed_one_is_refused_without_asking() {
    let scenario = Scenario::new();
    let (mut host, plugin) = host_on(&scenario);
    let share = acquire_on::<ShareCapability>(&mut host).expect("scripted share");
    let link =
        ExternalUri::try_new("https://example.com/note/3", &[Scheme::HTTPS]).expect("a link");
    let report = std::env::temp_dir().join("flui-share-report.pdf");
    let request = ShareRequest::new()
        .title("Note")
        .text("See this")
        .uri(link)
        .file(report);
    plugin.share().script().expect(
        Expect::when("the share, in the window", {
            let request = request.clone();
            move |asked: &InWindow<ShareRequest>| asked.request == request
        }),
        Answer::value(Ok(())),
    );
    let shared = land_completion(share.share(request));
    assert!(shared.borrow().is_none(), "never inside the call");
    turn(&mut host);
    assert_eq!(*shared.borrow(), Some(Ok(Ok(()))));

    let mut refused: Vec<(ShareRequest, MalformedShare)> = vec![
        (ShareRequest::new(), MalformedShare::Empty),
        (
            ShareRequest::new().title("only a title"),
            MalformedShare::Empty,
        ),
        (ShareRequest::new().text("a\0b"), MalformedShare::Text),
        (
            ShareRequest::new().text("t".repeat(ShareRequest::MAX_TEXT_LEN + 1)),
            MalformedShare::Text,
        ),
        (
            ShareRequest::new().file("relative.pdf"),
            MalformedShare::RelativePath,
        ),
        (
            (0..=ShareRequest::MAX_FILES).fold(ShareRequest::new(), |request, index| {
                request.file(std::env::temp_dir().join(format!("f{index}")))
            }),
            MalformedShare::TooManyFiles,
        ),
    ];
    if cfg!(windows) {
        refused.push((
            ShareRequest::new().file(r"\\server\share\doc.pdf"),
            MalformedShare::NotLocal,
        ));
    }
    let slots = refused
        .into_iter()
        .map(|(request, reason)| (land_completion(share.share(request)), reason))
        .collect::<Vec<(Slot<ShareResult>, MalformedShare)>>();
    turn(&mut host);
    for (slot, reason) in slots {
        assert_eq!(
            *slot.borrow(),
            Some(Ok(Err(ShareError::InvalidRequest(reason)))),
            "{reason:?}"
        );
    }
    scenario
        .verify()
        .expect("no malformed share reached the system");
}

#[test]
fn launch_at_login_answers_the_state_the_system_leaves() {
    let scenario = Scenario::new();
    let (mut host, plugin) = host_on(&scenario);
    let login = acquire_on::<LaunchAtLoginCapability>(&mut host).expect("scripted login items");
    plugin.login().script().expect(
        Expect::exactly(LoginItemRequest::Enable),
        Answer::value(Ok(LoginItemState::DisabledByUser)),
    );
    plugin.login().script().expect(
        Expect::exactly(LoginItemRequest::Read),
        Answer::value(Ok(LoginItemState::DisabledByUser)),
    );
    let enabled = land_completion(login.enable());
    let read = land_completion(login.state());
    turn(&mut host);
    let expected: Option<Result<LoginItemResult, Abandoned>> =
        Some(Ok(Ok(LoginItemState::DisabledByUser)));
    assert_eq!(*enabled.borrow(), expected, "the user's choice stands");
    assert_eq!(*read.borrow(), expected);
    scenario.verify().expect("both requests were scripted");
}

#[test]
fn a_shortcut_holds_control_alt_or_meta_unless_no_application_types_its_key() {
    let cases = [
        (Modifiers::CONTROL | Modifiers::SHIFT, Code::KeyK, Ok(())),
        (Modifiers::META, Code::Space, Ok(())),
        (Modifiers::ALT, Code::F4, Ok(())),
        (Modifiers::NONE, Code::F13, Ok(())),
        (Modifiers::NONE, Code::MediaPlayPause, Ok(())),
        (
            Modifiers::SHIFT,
            Code::KeyK,
            Err(InvalidShortcut::NeedsModifier),
        ),
        (
            Modifiers::NONE,
            Code::F1,
            Err(InvalidShortcut::NeedsModifier),
        ),
        (
            Modifiers::CONTROL | Modifiers::CAPS_LOCK,
            Code::KeyK,
            Err(InvalidShortcut::Modifier),
        ),
        (Modifiers::FN, Code::F13, Err(InvalidShortcut::Modifier)),
    ];
    for (modifiers, code, expected) in cases {
        assert_eq!(
            Shortcut::try_new(modifiers, code).map(|_| ()),
            expected,
            "{modifiers:?} + {code:?}"
        );
    }
}

#[test]
fn a_registered_shortcut_runs_its_listener_at_a_turn_until_its_registration_drops() {
    let scenario = Scenario::new();
    let (mut host, plugin) = host_on(&scenario);
    let shortcuts = acquire_on::<GlobalShortcutCapability>(&mut host).expect("scripted shortcuts");
    let chord =
        Shortcut::try_new(Modifiers::CONTROL | Modifiers::ALT, Code::KeyN).expect("a chord");
    let taken = Shortcut::try_new(Modifiers::META, Code::KeyL).expect("a chord");
    plugin
        .shortcuts()
        .script()
        .expect(Expect::exactly(chord), Answer::value(Ok(())));
    plugin.shortcuts().script().expect(
        Expect::exactly(taken),
        Answer::value(Err(ShortcutError::Taken)),
    );

    let pressed = Rc::new(Cell::new(0_u32));
    let counting = Rc::clone(&pressed);
    let registered =
        land_completion(shortcuts.register(chord, move || counting.set(counting.get() + 1)));
    let refused = land_completion(shortcuts.register(taken, || panic!("never registered")));
    turn(&mut host);
    turn(&mut host);
    assert!(matches!(
        &*refused.borrow(),
        Some(Ok(Err(ShortcutError::Taken)))
    ));
    assert_eq!(
        plugin.shortcuts().held(),
        1,
        "a refused chord holds nothing"
    );
    let registration: Option<Result<ShortcutResult, Abandoned>> = registered.borrow_mut().take();
    let registration = registration
        .expect("answered")
        .expect("not abandoned")
        .expect("registered");
    assert_eq!(registration.shortcut(), chord);

    assert_eq!(plugin.shortcuts().press(chord), 1);
    assert_eq!(pressed.get(), 0, "never inside the press");
    turn(&mut host);
    assert_eq!(pressed.get(), 1);

    // A press reported before the drop, delivered after it, runs nothing.
    assert_eq!(plugin.shortcuts().press(chord), 1);
    drop(registration);
    turn(&mut host);
    assert_eq!(pressed.get(), 1);
    assert_eq!(plugin.shortcuts().held(), 0, "the drop unregistered");
    assert_eq!(plugin.shortcuts().press(chord), 0);
    scenario.verify().expect("both registrations were scripted");
}

#[test]
fn a_taskbar_change_reaches_its_windows_button_and_stops_with_the_window() {
    let scenario = Scenario::new();
    let (mut host, plugin) = host_on(&scenario);
    let window = host.open_window(HeadlessWindow::new(10, 10));
    let taskbar = acquire_in::<TaskbarCapability>(&mut host, window).expect("scripted taskbar");
    let half = Fraction::try_new(0.5).expect("a fraction");
    let five = TaskbarBadge::Count(NonZeroU32::new(5).expect("a count"));
    assert_eq!(taskbar.set_progress(TaskbarProgress::Normal(half)), Ok(()));
    assert_eq!(taskbar.set_badge(five), Ok(()));
    let changes = plugin.taskbar().changes();
    assert!(matches!(
        changes.as_slice(),
        [
            TaskbarChange::Progress { progress: TaskbarProgress::Normal(fraction), .. },
            TaskbarChange::Badge { badge, .. },
        ] if *fraction == half && *badge == five
    ));

    let unavailable = TaskbarError::Unsupported(Unsupported::new(UnsupportedReason::Unavailable));
    plugin.taskbar().refuse(Some(unavailable));
    assert_eq!(taskbar.set_badge(TaskbarBadge::Dot), Err(unavailable));
    plugin.taskbar().refuse(None);

    assert!(host.request_close(window, CloseReason::User));
    turn(&mut host);
    assert_eq!(
        taskbar.set_progress(TaskbarProgress::Indeterminate),
        Err(TaskbarError::WindowClosed)
    );
    assert_eq!(
        plugin.taskbar().changes().len(),
        2,
        "a closed window's change never reached it"
    );
}

#[test]
fn a_jump_list_is_refused_without_asking_when_its_tasks_are_malformed() {
    let scenario = Scenario::new();
    let (mut host, plugin) = host_on(&scenario);
    let jump_list = acquire_on::<JumpListCapability>(&mut host).expect("scripted jump list");
    let tasks = vec![
        JumpListTask::new("New note")
            .argument("--new")
            .argument("my notes"),
        JumpListTask::new("Open inbox").description("The inbox"),
    ];
    plugin
        .jump_list()
        .script()
        .expect(Expect::exactly(tasks.clone()), Answer::value(Ok(())));
    let set = land_completion(jump_list.set_tasks(tasks));
    let refused: Vec<(Vec<JumpListTask>, MalformedTask)> = vec![
        (vec![JumpListTask::new(" ")], MalformedTask::Text),
        (vec![JumpListTask::new("a\tb")], MalformedTask::Text),
        (
            vec![JumpListTask::new("x").argument("nul\0")],
            MalformedTask::Argument,
        ),
        (
            vec![
                (0..=JumpListTask::MAX_ARGUMENTS).fold(JumpListTask::new("x"), |task, index| {
                    task.argument(index.to_string())
                }),
            ],
            MalformedTask::TooManyArguments,
        ),
        (
            (0..=JumpList::MAX_TASKS)
                .map(|index| JumpListTask::new(format!("T{index}")))
                .collect::<Vec<JumpListTask>>(),
            MalformedTask::TooManyTasks,
        ),
    ];
    let slots = refused
        .into_iter()
        .map(|(tasks, reason)| (land_completion(jump_list.set_tasks(tasks)), reason))
        .collect::<Vec<(Slot<JumpListResult>, MalformedTask)>>();
    turn(&mut host);
    assert_eq!(*set.borrow(), Some(Ok(Ok(()))));
    for (slot, reason) in slots {
        assert_eq!(
            *slot.borrow(),
            Some(Ok(Err(JumpListError::InvalidTasks(reason)))),
            "{reason:?}"
        );
    }
    scenario
        .verify()
        .expect("only the well-formed list reached the system");
}

#[test]
fn a_folder_is_absolute_and_the_os_is_what_its_system_says() {
    let scenario = Scenario::new();
    let (mut host, plugin) = host_on(&scenario);
    let os = acquire_on::<OsCapability>(&mut host).expect("scripted OS");
    let home = std::env::temp_dir();
    plugin.os().folders().expect(
        Expect::exactly(KnownFolder::Home),
        Answer::value(Ok(home.clone())),
    );
    plugin.os().folders().expect(
        Expect::exactly(KnownFolder::Downloads),
        Answer::value(Ok(PathBuf::from("relative"))),
    );
    plugin.os().folders().expect(
        Expect::exactly(KnownFolder::Videos),
        Answer::value(Err(FolderError::NotFound)),
    );
    let found = land_completion(os.folder(KnownFolder::Home));
    let relative = land_completion(os.folder(KnownFolder::Downloads));
    let missing = land_completion(os.folder(KnownFolder::Videos));
    for _ in 0..2 {
        let _ = host.pump(Duration::ZERO);
    }
    let answered = |slot: &Slot<FolderResult>| slot.borrow().clone();
    assert_eq!(answered(&found), Some(Ok(Ok(home))));
    assert_eq!(
        answered(&relative),
        Some(Ok(Err(FolderError::Failed(SystemFailure::MalformedAnswer)))),
        "a folder is absolute"
    );
    assert_eq!(answered(&missing), Some(Ok(Err(FolderError::NotFound))));

    let info = OsInfo::new(
        "Windows 11",
        OsVersion {
            major: 10,
            minor: 0,
            patch: 26_100,
        },
        Architecture::Aarch64,
    );
    plugin.os().set_info(info.clone());
    assert_eq!(os.info(), info);
    assert_eq!(info.version.to_string(), "10.0.26100");
    scenario.verify().expect("every folder was scripted");
}
