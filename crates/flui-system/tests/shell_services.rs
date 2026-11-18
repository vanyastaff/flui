//! The message box, the share sheet, the jump list and the system
//! information through their handles on the scripted doubles: what each
//! refuses before asking, what reaches its window, and what it answers.

use std::cell::RefCell;
use std::rc::Rc;

use flui_platform::SystemFailure;
use flui_platform::capability::Abandoned;
use flui_platform::capability::scripted::{Answer, Expect, Scenario};
use flui_platform::launch::{ExternalUri, Scheme};
use flui_sdk::view::{CloseReason, LifecycleContext};
use flui_system::dialog::{
    DialogError, FileDialogCapability, MAX_TEXT_LEN, MalformedRequest, MessageAnswer,
    MessageBoxCapability, MessageBoxRequest, MessageBoxResult, MessageButtons, MessageLevel,
    OpenRequest,
};
use flui_system::jump_list::{
    JumpList, JumpListCapability, JumpListError, JumpListResult, JumpListTask, MalformedTask,
};
use flui_system::os::{Architecture, OsCapability, OsInfo, OsVersion};
use flui_system::share::{MalformedShare, ShareCapability, ShareError, ShareRequest, ShareResult};
use flui_system::testing::{InWindow, ScriptedSystem};
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
        (
            MessageBoxRequest::new("ok").detail("  "),
            MalformedRequest::Text,
        ),
        (
            MessageBoxRequest::new("ok").detail("a\0b"),
            MalformedRequest::Text,
        ),
        (
            MessageBoxRequest::new("ok").detail("d".repeat(MAX_TEXT_LEN + 1)),
            MalformedRequest::Text,
        ),
        // A default the box does not offer would leave Return to no button.
        (
            MessageBoxRequest::new("ok").default_button(MessageAnswer::Cancel),
            MalformedRequest::DefaultButton,
        ),
        (
            MessageBoxRequest::new("Delete?")
                .buttons(MessageButtons::YesNo)
                .default_button(MessageAnswer::Ok),
            MalformedRequest::DefaultButton,
        ),
        // A label names a button of the box, in one line the user can read
        // and tell from the others.
        (
            MessageBoxRequest::new("ok").button_label(MessageAnswer::Yes, "Sure"),
            MalformedRequest::ButtonLabel,
        ),
        (
            MessageBoxRequest::new("ok").button_label(MessageAnswer::Ok, " "),
            MalformedRequest::ButtonLabel,
        ),
        (
            MessageBoxRequest::new("ok").button_label(MessageAnswer::Ok, "Two\nlines"),
            MalformedRequest::ButtonLabel,
        ),
        (
            MessageBoxRequest::new("Delete?")
                .buttons(MessageButtons::YesNo)
                .button_label(MessageAnswer::Yes, "Go on")
                .button_label(MessageAnswer::No, " Go on "),
            MalformedRequest::ButtonLabel,
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
fn a_message_box_carries_its_detail_its_default_button_and_its_labels_to_the_system() {
    let scenario = Scenario::new();
    let (mut host, plugin) = host_on(&scenario);
    let boxes = acquire_on::<MessageBoxCapability>(&mut host).expect("scripted boxes");

    let plain = MessageBoxRequest::new("Delete the note?").buttons(MessageButtons::YesNoCancel);
    assert_eq!(
        plain.title, None,
        "a box shows no title unless it is given one"
    );
    assert_eq!(
        plain.default_answer(),
        MessageAnswer::Yes,
        "the first button is the default unless the request names another"
    );
    assert_eq!(plain.label(MessageAnswer::Yes), None);
    for (buttons, answers) in [
        (MessageButtons::Ok, &[MessageAnswer::Ok][..]),
        (
            MessageButtons::OkCancel,
            &[MessageAnswer::Ok, MessageAnswer::Cancel],
        ),
        (
            MessageButtons::YesNo,
            &[MessageAnswer::Yes, MessageAnswer::No],
        ),
        (
            MessageButtons::YesNoCancel,
            &[MessageAnswer::Yes, MessageAnswer::No, MessageAnswer::Cancel],
        ),
    ] {
        assert_eq!(buttons.answers(), answers, "{buttons:?}");
        for answer in [
            MessageAnswer::Ok,
            MessageAnswer::Cancel,
            MessageAnswer::Yes,
            MessageAnswer::No,
        ] {
            assert_eq!(
                buttons.admits(answer),
                answers.contains(&answer),
                "{buttons:?} {answer:?}"
            );
        }
    }

    // A confirmation that cannot be undone: Return presses the safe answer.
    let careful = plain
        .clone()
        .detail("It cannot be brought back.\nIts attachments go with it.")
        .default_button(MessageAnswer::No)
        .button_label(MessageAnswer::Yes, "Keep")
        .button_label(MessageAnswer::Yes, "Delete")
        .button_label(MessageAnswer::Cancel, "Go back");
    assert_eq!(careful.default_answer(), MessageAnswer::No);
    assert_eq!(
        careful.label(MessageAnswer::Yes),
        Some("Delete"),
        "labelling a button again replaces its label"
    );
    assert_eq!(careful.label(MessageAnswer::No), None);
    assert_eq!(
        careful,
        plain
            .detail("It cannot be brought back.\nIts attachments go with it.")
            .button_label(MessageAnswer::Cancel, "Go back")
            .button_label(MessageAnswer::Yes, "Delete")
            .default_button(MessageAnswer::No),
        "two requests that say the same are equal, whatever order they were built in"
    );

    plugin.message_boxes().script().expect(
        Expect::when("the confirmation as it was built", {
            let careful = careful.clone();
            move |asked: &InWindow<MessageBoxRequest>| {
                asked.request == careful
                    && asked.request.detail.as_deref()
                        == Some("It cannot be brought back.\nIts attachments go with it.")
                    && asked.request.default_button == Some(MessageAnswer::No)
                    && asked.request.label(MessageAnswer::Cancel) == Some("Go back")
            }
        }),
        Answer::value(Ok(MessageAnswer::Yes)),
    );
    let answered = land_dialog(boxes.show(careful));
    turn(&mut host);
    assert_eq!(
        *answered.borrow(),
        Some(Ok(Ok(MessageAnswer::Yes))),
        "a labelled button answers what its button answers"
    );
    scenario
        .verify()
        .expect("only the scripted box reached the system");
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
fn the_os_is_what_its_system_says() {
    let scenario = Scenario::new();
    let (mut host, plugin) = host_on(&scenario);
    let os = acquire_on::<OsCapability>(&mut host).expect("scripted OS");
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
    scenario
        .verify()
        .expect("nothing was asked of the scenario");
}
