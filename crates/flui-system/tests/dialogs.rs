//! The file dialogs: a widget's dialog reaching the system modal to its
//! window, one dialog per window, and requests refused at the seam.

use std::cell::RefCell;
use std::rc::Rc;

use flui_platform::PlatformWindow;
use flui_platform::capability::{
    Abandoned, CapabilityRegistrar, Completion, Plugin, ProviderContext, Resolver, WindowRef,
};
use flui_system::dialog::{
    DialogError, FileDialogBackend, FileDialogCapability, FileDialogs, FileFilter, OpenRequest,
    OpenResult, PickedFile, SaveRequest, SaveResult,
};
use flui_system::testing::{DialogRequest, ScriptedFileDialogs, ScriptedLauncher, ScriptedSystem};

use crate::{acquire, outcome_slot, turn};

type Slot<T> = Rc<RefCell<Option<Result<T, Abandoned>>>>;

fn land<T: 'static>(completion: Completion<T>) -> Slot<T> {
    let slot = outcome_slot::<Result<T, Abandoned>>();
    let into = Rc::clone(&slot);
    completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    slot
}

fn scripted() -> (ScriptedFileDialogs, ScriptedSystem) {
    let dialogs = ScriptedFileDialogs::new();
    let plugin = ScriptedSystem::new(ScriptedLauncher::new(), dialogs.clone());
    (dialogs, plugin)
}

#[test]
fn an_open_dialog_is_modal_to_the_widgets_window_and_answers_its_picks() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let picks = vec![
        PickedFile::new("/home/a.png"),
        PickedFile::new("/home/b.png"),
    ];
    dialogs.answer_next_open(Ok(Some(picks.clone())));

    let request = OpenRequest::files()
        .allow_multiple()
        .with_title("Choose pictures")
        .with_directory("/home")
        .with_filter(
            FileFilter::new("Images")
                .with_extension(".png")
                .with_mime_type("image/png"),
        );
    let answered = land(handle.open(request.clone()));
    turn(&mut host);

    let requests: Vec<DialogRequest> = dialogs.requests();
    let [
        DialogRequest::Open {
            window,
            request: asked,
        },
    ] = requests.as_slice()
    else {
        panic!("one open dialog was asked for: {requests:?}");
    };
    assert_eq!(*window, host.window().id());
    assert_eq!(*asked, request);
    assert_eq!(
        asked.filters()[0].extensions(),
        ["png"],
        "the leading dot is dropped"
    );
    assert_eq!(*answered.borrow(), Some(Ok(Ok(Some(picks)))));
}

#[test]
fn a_cancelled_save_dialog_answers_none() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");

    let answered = land(handle.save(SaveRequest::new().with_suggested_name("notes.txt")));
    turn(&mut host);

    assert_eq!(dialogs.requests().len(), 1);
    assert_eq!(*answered.borrow(), Some(Ok(Ok(None))));
}

#[test]
fn a_window_shows_one_dialog_at_a_time() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    dialogs.hold();

    let first = land(handle.open(OpenRequest::files()));
    // A second handle of the same window shares the rule.
    let other = handle.clone();
    let second = land(other.save(SaveRequest::new()));
    turn(&mut host);
    assert!(first.borrow().is_none(), "the first dialog is still open");
    assert_eq!(*second.borrow(), Some(Ok(Err(DialogError::Busy))));
    assert_eq!(
        dialogs.requests().len(),
        1,
        "the busy one never reached the system"
    );

    assert!(dialogs.release_open(Ok(None)));
    turn(&mut host);
    assert_eq!(*first.borrow(), Some(Ok(Ok(None))));

    let third = land(handle.save(SaveRequest::new()));
    turn(&mut host);
    assert!(third.borrow().is_none(), "shown once the first answered");
    assert!(dialogs.release_save(Ok(Some(PickedFile::new("/tmp/out.txt")))));
    turn(&mut host);
    assert_eq!(
        *third.borrow(),
        Some(Ok(Ok(Some(PickedFile::new("/tmp/out.txt")))))
    );
}

#[test]
fn a_malformed_request_is_refused_without_showing_a_dialog() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let images = || FileFilter::new("Images").with_extension("png");

    let opens = [
        OpenRequest::files().with_filter(FileFilter::new("").with_extension("png")),
        OpenRequest::files().with_filter(FileFilter::new("Nothing")),
        OpenRequest::files().with_filter(FileFilter::new("Glob").with_extension("*.png")),
        OpenRequest::files().with_filter(FileFilter::new("Path").with_extension("a/b")),
        OpenRequest::files().with_filter(FileFilter::new("Mime").with_mime_type("image")),
        OpenRequest::folders().with_filter(images()),
        OpenRequest::files().with_directory("relative/dir"),
    ];
    let saves = [
        SaveRequest::new().with_suggested_name("dir/name.txt"),
        SaveRequest::new().with_suggested_name(".."),
        SaveRequest::new().with_suggested_name("  "),
        SaveRequest::new().with_filter(FileFilter::new("Images")),
        SaveRequest::new().with_directory("relative"),
    ];
    let open_answers = opens.map(|request| land(handle.open(request)));
    let save_answers = saves.map(|request| land(handle.save(request)));
    turn(&mut host);

    assert!(dialogs.requests().is_empty(), "no dialog was shown");
    for answer in open_answers {
        assert_eq!(*answer.borrow(), Some(Ok(Err(DialogError::InvalidRequest))));
    }
    for answer in save_answers {
        assert_eq!(*answer.borrow(), Some(Ok(Err(DialogError::InvalidRequest))));
    }

    // A refused request leaves the window free for the next one.
    let valid = land(handle.open(OpenRequest::files().with_filter(images())));
    turn(&mut host);
    assert_eq!(*valid.borrow(), Some(Ok(Ok(None))));
}

/// A backend that drops every resolver unanswered, as one whose OS call
/// failed before it could answer does.
#[derive(Clone)]
struct ForgetfulBackend;

impl FileDialogBackend for ForgetfulBackend {
    fn open(&self, _window: &WindowRef, _request: OpenRequest, answer: Resolver<OpenResult>) {
        drop(answer);
    }

    fn save(&self, _window: &WindowRef, _request: SaveRequest, answer: Resolver<SaveResult>) {
        drop(answer);
    }
}

struct ForgetfulPlugin;

impl Plugin for ForgetfulPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<FileDialogCapability>(|cx: &ProviderContext<'_>| {
            FileDialogs::try_new(ForgetfulBackend, cx)
        });
    }
}

#[test]
fn a_backend_that_drops_its_answer_ends_the_dialog_and_frees_the_window() {
    let (mut host, handle) = acquire::<FileDialogCapability>(&ForgetfulPlugin);
    let handle = handle.expect("the plugin provides dialogs");

    let first = land(handle.open(OpenRequest::files()));
    turn(&mut host);
    turn(&mut host);
    assert_eq!(*first.borrow(), Some(Err(Abandoned::ProviderDropped)));

    let second = land(handle.save(SaveRequest::new()));
    turn(&mut host);
    turn(&mut host);
    assert_eq!(
        *second.borrow(),
        Some(Err(Abandoned::ProviderDropped)),
        "not Busy: the dropped answer freed the window"
    );
}
