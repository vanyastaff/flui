//! The file dialogs: a widget's dialog reaching the system modal to its
//! window, one dialog per window, a dialog ending with its window or its
//! caller, answers only the request can have, and requests refused at the
//! seam.

use std::cell::RefCell;
use std::rc::Rc;

use flui_platform::capability::{
    Abandoned, CapabilityRegistrar, Plugin, ProviderContext, Resolver, WindowRef,
};
use flui_platform::{PlatformWindow, SystemFailure};
use flui_sdk::view::CloseReason;
use flui_system::dialog::{
    Dialog, DialogError, DialogHold, FileDialogBackend, FileDialogCapability, FileDialogs,
    FileFilter, OpenRequest, OpenResult, PickedFile, Picks, SaveRequest, SaveResult,
};
use flui_system::testing::{DialogRequest, ScriptedFileDialogs, ScriptedLauncher, ScriptedSystem};
use flui_testing::{HeadlessHost, HeadlessWindow};

use crate::{acquire, acquire_in, acquire_twice_on, outcome_slot, turn};

type Slot<T> = Rc<RefCell<Option<Result<T, Abandoned>>>>;

fn land<T: 'static>(dialog: Dialog<T>) -> Slot<T> {
    let slot = outcome_slot::<Result<T, Abandoned>>();
    let into = Rc::clone(&slot);
    dialog.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    slot
}

fn scripted() -> (ScriptedFileDialogs, ScriptedSystem) {
    let dialogs = ScriptedFileDialogs::new();
    let plugin = ScriptedSystem::new(ScriptedLauncher::new(), dialogs.clone());
    (dialogs, plugin)
}

/// The file `name` in the temporary folder: an absolute path on every host.
fn file(name: &str) -> PickedFile {
    PickedFile::try_new(std::env::temp_dir().join(name)).expect("an absolute path is a pick")
}

fn picks(names: &[&str]) -> Picks {
    Picks::try_new(names.iter().copied().map(file).collect()).expect("the test names a pick")
}

#[test]
fn an_open_dialog_is_modal_to_the_widgets_window_and_answers_its_picks() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let picked = picks(&["a.png", "b.png"]);
    dialogs.answer_next_open(Ok(Some(picked.clone())));

    let request = OpenRequest::files()
        .allow_multiple()
        .title("Choose pictures")
        .directory(std::env::temp_dir())
        .filter(
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
        asked.filters[0].extensions(),
        ["png"],
        "the leading dot is dropped"
    );
    assert_eq!(*answered.borrow(), Some(Ok(Ok(Some(picked)))));
}

#[test]
fn a_cancelled_save_dialog_answers_none() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");

    dialogs.answer_next_save(Ok(None));
    let answered = land(handle.save(SaveRequest::new().suggested_name("notes.txt")));
    turn(&mut host);

    assert_eq!(dialogs.requests().len(), 1);
    assert_eq!(*answered.borrow(), Some(Ok(Ok(None))));
}

#[test]
fn picks_are_never_empty_and_keep_their_order() {
    assert_eq!(
        Picks::try_new(Vec::new()),
        None,
        "nothing picked is no pick"
    );
    let picked = picks(&["a", "b", "c"]);
    assert_eq!(picked.len(), 3);
    assert_eq!(*picked.first(), file("a"));
    assert_eq!(
        picked.into_iter().collect::<Vec<PickedFile>>(),
        [file("a"), file("b"), file("c")]
    );

    assert_eq!(
        PickedFile::try_new("relative/name.txt"),
        None,
        "a pick is absolute"
    );
    assert_eq!(
        PickedFile::try_new(std::env::temp_dir().join("a\0b")),
        None,
        "no file has a NUL byte in its path"
    );
}

#[test]
fn several_picks_for_a_single_pick_request_are_a_malformed_answer() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");

    dialogs.answer_next_open(Ok(Some(picks(&["a", "b"]))));
    let single = land(handle.open(OpenRequest::files()));
    turn(&mut host);
    turn(&mut host);
    assert_eq!(
        *single.borrow(),
        Some(Ok(Err(DialogError::Failed(SystemFailure::MalformedAnswer))))
    );

    dialogs.answer_next_open(Ok(Some(picks(&["a"]))));
    let one = land(handle.open(OpenRequest::files()));
    turn(&mut host);
    turn(&mut host);
    assert_eq!(*one.borrow(), Some(Ok(Ok(Some(picks(&["a"]))))));
}

#[test]
fn a_window_shows_one_dialog_at_a_time() {
    let (dialogs, plugin) = scripted();
    let mut host = HeadlessHost::with_plugin(HeadlessWindow::new(10, 10), &plugin);
    let [first_widget, second_widget] = acquire_twice_on::<FileDialogCapability>(&mut host);
    let first_widget = first_widget.expect("the scripted plugin provides dialogs");
    let second_widget = second_widget.expect("the scripted plugin provides dialogs");
    let other_window = host.open_window(HeadlessWindow::new(10, 10));
    let elsewhere = acquire_in::<FileDialogCapability>(&mut host, other_window)
        .expect("the scripted plugin provides dialogs");
    dialogs.hold();

    let first = land(first_widget.open(OpenRequest::files()));
    let second = land(second_widget.save(SaveRequest::new()));
    let other = land(elsewhere.save(SaveRequest::new()));
    turn(&mut host);
    assert!(first.borrow().is_none(), "the first dialog is still open");
    assert_eq!(
        *second.borrow(),
        Some(Ok(Err(DialogError::Busy))),
        "another widget of the same window shares the rule"
    );
    assert!(
        other.borrow().is_none(),
        "a widget of another window shows its own dialog"
    );
    assert_eq!(
        dialogs.requests().len(),
        2,
        "the busy one never reached the system"
    );

    assert!(dialogs.release_open(Ok(None)));
    turn(&mut host);
    assert_eq!(*first.borrow(), Some(Ok(Ok(None))));

    let third = land(second_widget.save(SaveRequest::new()));
    turn(&mut host);
    assert!(third.borrow().is_none(), "shown once the first answered");
    assert_eq!(
        dialogs
            .save_script()
            .release_held(|_| Ok(Some(file("out.txt")))),
        2
    );
    turn(&mut host);
    assert_eq!(*third.borrow(), Some(Ok(Ok(Some(file("out.txt"))))));
    assert_eq!(*other.borrow(), Some(Ok(Ok(Some(file("out.txt"))))));
}

#[test]
fn a_dialog_ends_with_its_window() {
    let (dialogs, plugin) = scripted();
    let mut host = HeadlessHost::with_plugin(HeadlessWindow::new(10, 10), &plugin);
    let window = host.open_window(HeadlessWindow::new(10, 10));
    let handle = acquire_in::<FileDialogCapability>(&mut host, window)
        .expect("the scripted plugin provides dialogs");
    dialogs.hold();

    let held = land(handle.open(OpenRequest::files()));
    turn(&mut host);
    assert!(held.borrow().is_none(), "the dialog is up");

    assert!(host.request_close(window, CloseReason::User));
    turn(&mut host);
    assert_eq!(
        *held.borrow(),
        Some(Ok(Err(DialogError::WindowClosed))),
        "the caller hears that the window closed"
    );
    assert_eq!(dialogs.dismissed(), 1, "the system's dialog was taken down");
    assert!(
        !dialogs.release_open(Ok(None)),
        "nothing of the closed window is left to answer"
    );

    let later = land(handle.open(OpenRequest::files()));
    turn(&mut host);
    assert_eq!(*later.borrow(), Some(Ok(Err(DialogError::WindowClosed))));
    assert_eq!(
        dialogs.requests().len(),
        1,
        "a closed window's handle never asks the system"
    );
}

#[test]
fn a_dialog_ends_when_its_window_begins_to_close() {
    // The primary headless window stops without being removed, so only the
    // start of the close can tell the dialog.
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    dialogs.hold();

    let held = land(handle.save(SaveRequest::new()));
    turn(&mut host);
    let primary = host.primary_window();
    assert!(host.request_close(primary, CloseReason::User));
    turn(&mut host);
    assert_eq!(*held.borrow(), Some(Ok(Err(DialogError::WindowClosed))));
    assert_eq!(dialogs.dismissed(), 1);
}

#[test]
fn a_dropped_dialog_is_dismissed_and_frees_its_window() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    dialogs.hold();

    drop(handle.open(OpenRequest::files()));
    assert_eq!(dialogs.dismissed(), 1, "the system's dialog was taken down");
    assert!(
        !dialogs.release_open(Ok(None)),
        "the dismissed dialog waits for nothing"
    );

    let next = land(handle.save(SaveRequest::new()));
    turn(&mut host);
    assert!(next.borrow().is_none(), "not Busy: the next dialog is up");
    assert_eq!(dialogs.requests().len(), 2);
    assert!(dialogs.release_save(Ok(None)));
    turn(&mut host);
    assert_eq!(*next.borrow(), Some(Ok(Ok(None))));
    assert_eq!(
        dialogs.dismissed(),
        1,
        "an answered dialog is not dismissed"
    );
}

#[test]
fn a_malformed_request_is_refused_without_showing_a_dialog() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let images = || FileFilter::new("Images").with_extension("png");

    let opens = [
        OpenRequest::files().filter(FileFilter::new("").with_extension("png")),
        OpenRequest::files().filter(FileFilter::new("Nothing")),
        OpenRequest::files().filter(FileFilter::new("Glob").with_extension("*.png")),
        OpenRequest::files().filter(FileFilter::new("Path").with_extension("a/b")),
        OpenRequest::files().filter(FileFilter::new("Mime").with_mime_type("image")),
        OpenRequest::folders().filter(images()),
        OpenRequest::files().directory("relative/dir"),
    ];
    let saves = [
        SaveRequest::new().suggested_name("dir/name.txt"),
        SaveRequest::new().suggested_name(".."),
        SaveRequest::new().suggested_name("  "),
        SaveRequest::new().filter(FileFilter::new("Images")),
        SaveRequest::new().directory("relative"),
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
    dialogs.answer_next_open(Ok(None));
    let valid = land(handle.open(OpenRequest::files().filter(images())));
    turn(&mut host);
    assert_eq!(*valid.borrow(), Some(Ok(Ok(None))));
}

/// A backend that drops every resolver unanswered, as one whose OS call
/// failed before it could answer does.
#[derive(Clone)]
struct ForgetfulBackend;

impl FileDialogBackend for ForgetfulBackend {
    fn open(
        &self,
        _window: &WindowRef,
        _request: OpenRequest,
        answer: Resolver<OpenResult>,
    ) -> DialogHold {
        drop(answer);
        DialogHold::new(())
    }

    fn save(
        &self,
        _window: &WindowRef,
        _request: SaveRequest,
        answer: Resolver<SaveResult>,
    ) -> DialogHold {
        drop(answer);
        DialogHold::new(())
    }
}

struct ForgetfulPlugin;

impl Plugin for ForgetfulPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        registrar.provide::<FileDialogCapability>(|ctx: &ProviderContext<'_>| {
            FileDialogs::try_new(ForgetfulBackend, ctx)
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

/// A backend that keeps the resolver it was handed, then panics before it
/// returns its hold, as one whose OS call unwinds after it registered the
/// request does.
#[derive(Clone, Default)]
struct PanickingBackend {
    kept: Rc<RefCell<Vec<Resolver<OpenResult>>>>,
}

impl FileDialogBackend for PanickingBackend {
    fn open(
        &self,
        _window: &WindowRef,
        _request: OpenRequest,
        answer: Resolver<OpenResult>,
    ) -> DialogHold {
        self.kept.borrow_mut().push(answer);
        panic!("the backend failed while showing the dialog");
    }

    fn save(
        &self,
        _window: &WindowRef,
        _request: SaveRequest,
        answer: Resolver<SaveResult>,
    ) -> DialogHold {
        drop(answer);
        DialogHold::new(())
    }
}

struct PanickingPlugin(PanickingBackend);

impl Plugin for PanickingPlugin {
    fn register(&self, registrar: &mut CapabilityRegistrar) {
        let backend = self.0.clone();
        registrar.provide::<FileDialogCapability>(move |ctx: &ProviderContext<'_>| {
            FileDialogs::try_new(backend.clone(), ctx)
        });
    }
}

#[test]
fn a_backend_that_panics_while_showing_leaves_the_window_free() {
    let backend = PanickingBackend::default();
    let (mut host, handle) = acquire::<FileDialogCapability>(&PanickingPlugin(backend.clone()));
    let handle = handle.expect("the plugin provides dialogs");

    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: the test catches the backend's panic to show what the handle left behind"
    )]
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(handle.open(OpenRequest::files()));
    }));
    assert!(unwound.is_err(), "the backend's panic reaches the caller");
    assert_eq!(
        backend.kept.borrow().len(),
        1,
        "the backend kept its resolver"
    );

    let next = land(handle.save(SaveRequest::new()));
    turn(&mut host);
    turn(&mut host);
    assert_eq!(
        *next.borrow(),
        Some(Err(Abandoned::ProviderDropped)),
        "not Busy: the unwound dialog freed the window"
    );
}

#[test]
fn a_dialog_error_carries_its_unsupported() {
    use std::error::Error;

    use flui_platform::{Unsupported, UnsupportedReason};

    let unsupported = Unsupported::new(UnsupportedReason::Unavailable);
    let error = DialogError::from(unsupported);
    assert_eq!(error.unsupported(), Some(unsupported));
    assert_eq!(
        error
            .source()
            .and_then(|source| source.downcast_ref::<Unsupported>()),
        Some(&unsupported)
    );
    assert_eq!(
        error.to_string(),
        "the dialog service cannot do this: unsupported: unavailable for now"
    );
    assert_eq!(DialogError::Busy.unsupported(), None);

    let failed = DialogError::from(SystemFailure::PortalResponse(2));
    assert_eq!(
        failed
            .source()
            .and_then(|source| source.downcast_ref::<SystemFailure>()),
        Some(&SystemFailure::PortalResponse(2))
    );
    assert_eq!(
        failed.to_string(),
        "the system failed: the desktop portal ended the request with response 2"
    );
}
