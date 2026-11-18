//! The file dialogs: a widget's dialog reaching the system modal to its
//! window, one dialog per window, a dialog ending with its window or its
//! caller, answers only the request can have, and requests refused at the
//! seam.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::{
    Abandoned, CapabilityRegistrar, Plugin, ProviderContext, Resolver, WindowRef,
};
use flui_platform::{PlatformWindow, SystemFailure};
use flui_sdk::view::CloseReason;
use flui_system::dialog::{
    Dialog, DialogError, DialogHold, FileDialogBackend, FileDialogCapability, FileDialogs,
    FileFilter, MAX_TEXT_LEN, MalformedRequest, OpenRequest, OpenResult, PickedFile, PickedForm,
    Picks, SaveLocation, SaveRequest, SaveResult,
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

/// A save to the file `name`, with no filter reported.
fn saved(name: &str) -> SaveLocation {
    SaveLocation::new(file(name), None)
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
fn a_save_reports_the_filter_selected_when_the_user_accepted() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let request = || {
        SaveRequest::new()
            .filter(FileFilter::new("PNG").with_extension("png"))
            .filter(FileFilter::new("JPEG").with_extension("jpg"))
    };

    dialogs.answer_next_save(Ok(Some(SaveLocation::new(file("picture"), Some(1)))));
    let chosen = land(handle.save(request()));
    turn(&mut host);
    turn(&mut host);
    let answered = chosen.borrow_mut().take();
    let location = match answered {
        Some(Ok(Ok(Some(location)))) => location,
        other => panic!("a save location: {other:?}"),
    };
    assert_eq!(location.filter(), Some(1), "the JPEG filter was selected");
    assert_eq!(location.into_file(), file("picture"));

    // A filter the request did not offer is not one the system can report.
    let malformed = DialogError::Failed(SystemFailure::MalformedAnswer);
    dialogs.answer_next_save(Ok(Some(SaveLocation::new(file("picture"), Some(2)))));
    let beyond = land(handle.save(request()));
    turn(&mut host);
    turn(&mut host);
    assert_eq!(*beyond.borrow(), Some(Ok(Err(malformed))));
    dialogs.answer_next_save(Ok(Some(SaveLocation::new(file("picture"), Some(0)))));
    let unoffered = land(handle.save(SaveRequest::new()));
    turn(&mut host);
    turn(&mut host);
    assert_eq!(*unoffered.borrow(), Some(Ok(Err(malformed))));
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
            .release_held(|_| Ok(Some(saved("out.txt")))),
        2
    );
    turn(&mut host);
    assert_eq!(*third.borrow(), Some(Ok(Ok(Some(saved("out.txt"))))));
    assert_eq!(*other.borrow(), Some(Ok(Ok(Some(saved("out.txt"))))));
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

/// The last window's close retires its runtime, as on a real host: the
/// system's dialog is taken down, and the caller, gone with the runtime,
/// hears nothing, even from an answer released after the close.
#[test]
fn a_dialog_of_the_last_window_ends_with_its_runtime() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    dialogs.hold();

    let held = land(handle.save(SaveRequest::new()));
    turn(&mut host);
    let primary = host.primary_window();
    assert!(host.request_close(primary, CloseReason::User));
    assert!(host.is_retired(), "the last close retires the runtime");
    assert_eq!(dialogs.dismissed(), 1, "the system's dialog was taken down");
    assert!(
        !dialogs.release_save(Ok(None)),
        "nothing of the closed window is left to answer"
    );
    turn(&mut host);
    assert!(
        held.borrow().is_none(),
        "the caller's closure went with the runtime, unrun"
    );
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
fn a_filter_keeps_each_pattern_once_as_the_dialog_compares_it() {
    let filter = FileFilter::new("Archives")
        .with_extension(".TAR.GZ")
        .with_extension("tar.gz")
        .with_extension("Zip")
        .with_extension(".zip")
        .with_mime_type("Application/ZIP")
        .with_mime_type("application/zip")
        .with_mime_type("image/*");
    assert_eq!(
        filter.extensions(),
        ["tar.gz", "zip"],
        "lower case, no leading dot, each once"
    );
    assert_eq!(filter.mime_types(), ["application/zip", "image/*"]);
    assert_eq!(
        FileFilter::new("Images").with_extension("PNG"),
        FileFilter::new("Images").with_extension("png"),
        "two spellings of one extension are one filter"
    );
}

#[test]
fn a_malformed_request_is_refused_without_showing_a_dialog() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let images = || FileFilter::new("Images").with_extension("png");
    let longest = "a".repeat(FileFilter::MAX_PATTERN_LEN);
    let too_long = "a".repeat(FileFilter::MAX_PATTERN_LEN + 1);

    let opens = [
        (
            OpenRequest::files().filter(FileFilter::new("").with_extension("png")),
            MalformedRequest::FilterName,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Bell\u{7}").with_extension("png")),
            MalformedRequest::FilterName,
        ),
        (
            OpenRequest::files().filter(
                FileFilter::new("n".repeat(FileFilter::MAX_NAME_LEN + 1)).with_extension("png"),
            ),
            MalformedRequest::FilterName,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Nothing")),
            MalformedRequest::EmptyFilter,
        ),
        (
            OpenRequest::files().filter(
                (0..=FileFilter::MAX_PATTERNS).fold(FileFilter::new("Many"), |filter, index| {
                    filter.with_extension(format!("e{index}"))
                }),
            ),
            MalformedRequest::TooManyPatterns,
        ),
        (
            (0..=FileFilter::MAX_PER_REQUEST).fold(OpenRequest::files(), |request, index| {
                request.filter(FileFilter::new(format!("F{index}")).with_extension("png"))
            }),
            MalformedRequest::TooManyFilters,
        ),
        // A wildcard, a separator or an empty part reads differently on
        // each system, so no extension holds one.
        (
            OpenRequest::files().filter(FileFilter::new("Glob").with_extension("*.png")),
            MalformedRequest::Extension,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("One").with_extension("pn?")),
            MalformedRequest::Extension,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Set").with_extension("[pP]ng")),
            MalformedRequest::Extension,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("List").with_extension("png;exe")),
            MalformedRequest::Extension,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Path").with_extension("a/b")),
            MalformedRequest::Extension,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Gap").with_extension("tar..gz")),
            MalformedRequest::Extension,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Dot").with_extension(".")),
            MalformedRequest::Extension,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Space").with_extension("p ng")),
            MalformedRequest::Extension,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Accent").with_extension("pñg")),
            MalformedRequest::Extension,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Long").with_extension(too_long.as_str())),
            MalformedRequest::Extension,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Mime").with_mime_type("image")),
            MalformedRequest::MimeType,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Any").with_mime_type("*/*")),
            MalformedRequest::MimeType,
        ),
        (
            OpenRequest::files()
                .filter(FileFilter::new("Charset").with_mime_type("text/plain;charset=utf-8")),
            MalformedRequest::MimeType,
        ),
        (
            OpenRequest::files()
                .filter(images())
                .filter(FileFilter::new(" Images ").with_extension("jpg")),
            MalformedRequest::DuplicateFilterName,
        ),
        (
            OpenRequest::folders().filter(images()),
            MalformedRequest::FiltersOnFolders,
        ),
        (
            OpenRequest::files().directory("relative/dir"),
            MalformedRequest::RelativeDirectory,
        ),
        // Windows would show a title up to its NUL; D-Bus refuses one.
        (
            OpenRequest::files().title("Open\0 secretly"),
            MalformedRequest::Text,
        ),
        (
            OpenRequest::files().accept_label("Two\nlines"),
            MalformedRequest::Text,
        ),
        (
            OpenRequest::files().title("t".repeat(MAX_TEXT_LEN + 1)),
            MalformedRequest::Text,
        ),
    ];
    let saves = [
        (
            SaveRequest::new().suggested_name("dir/name.txt"),
            MalformedRequest::SuggestedName,
        ),
        (
            SaveRequest::new().suggested_name(".."),
            MalformedRequest::SuggestedName,
        ),
        (
            SaveRequest::new().suggested_name("  "),
            MalformedRequest::SuggestedName,
        ),
        (
            SaveRequest::new().filter(FileFilter::new("Images")),
            MalformedRequest::EmptyFilter,
        ),
        (
            SaveRequest::new().filter(images()).filter(images()),
            MalformedRequest::DuplicateFilterName,
        ),
        (
            SaveRequest::new().directory("relative"),
            MalformedRequest::RelativeDirectory,
        ),
        (
            SaveRequest::new().title("Save\u{7}"),
            MalformedRequest::Text,
        ),
    ];
    let open_answers = opens.map(|(request, malformed)| (land(handle.open(request)), malformed));
    let save_answers = saves.map(|(request, malformed)| (land(handle.save(request)), malformed));
    turn(&mut host);

    // Every row is checked, and every row that answers otherwise is named.
    let wrong_opens = open_answers.iter().filter_map(|(answer, malformed)| {
        let answered = answer.borrow();
        let as_wanted = matches!(
            &*answered,
            Some(Ok(Err(DialogError::InvalidRequest(reason)))) if reason == malformed
        );
        (!as_wanted).then(|| format!("{malformed:?}: {answered:?}"))
    });
    let wrong_saves = save_answers.iter().filter_map(|(answer, malformed)| {
        let answered = answer.borrow();
        let as_wanted = matches!(
            &*answered,
            Some(Ok(Err(DialogError::InvalidRequest(reason)))) if reason == malformed
        );
        (!as_wanted).then(|| format!("{malformed:?}: {answered:?}"))
    });
    let wrong = wrong_opens.chain(wrong_saves).collect::<Vec<String>>();
    assert!(wrong.is_empty(), "{wrong:#?}");
    assert!(dialogs.requests().is_empty(), "no dialog was shown");

    // Several parts, digits and `-_+` are an extension; the longest one is
    // taken, and a MIME type may name every subtype.
    dialogs.answer_next_open(Ok(None));
    let wide = land(
        handle.open(
            OpenRequest::files().filter(
                FileFilter::new("Many")
                    .with_extension("tar.gz")
                    .with_extension("c++")
                    .with_extension("my_ext-2")
                    .with_extension(longest.as_str())
                    .with_mime_type("image/*")
                    .with_mime_type("application/vnd.ms-excel"),
            ),
        ),
    );
    turn(&mut host);
    assert_eq!(*wide.borrow(), Some(Ok(Ok(None))));

    // Each bound is taken at its limit.
    let at_limits = (1..FileFilter::MAX_PER_REQUEST).fold(
        OpenRequest::files().filter((1..FileFilter::MAX_PATTERNS).fold(
            FileFilter::new("n".repeat(FileFilter::MAX_NAME_LEN)).with_extension("e0"),
            |filter, index| filter.with_extension(format!("e{index}")),
        )),
        |request, index| request.filter(FileFilter::new(format!("F{index}")).with_extension("png")),
    );
    assert_eq!(at_limits.filters.len(), FileFilter::MAX_PER_REQUEST);
    assert_eq!(
        at_limits.filters[0].extensions().len(),
        FileFilter::MAX_PATTERNS
    );
    dialogs.answer_next_open(Ok(None));
    let limits = land(handle.open(at_limits));
    turn(&mut host);
    assert_eq!(*limits.borrow(), Some(Ok(Ok(None))));

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

/// Turn until `slot` holds an outcome, or panic after ten seconds: a save's
/// write answers from a thread of its own.
#[cfg(not(target_arch = "wasm32"))]
fn turn_until_landed<T>(host: &mut HeadlessHost, slot: &Slot<T>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while slot.borrow().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the save did not answer in ten seconds"
        );
        turn(host);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn a_save_with_contents_answers_once_they_are_written_where_the_user_chose() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let folder = tempfile::tempdir().expect("a test folder");
    let chosen = SaveLocation::new(
        PickedFile::try_new(folder.path().join("notes.txt")).expect("an absolute path"),
        Some(0),
    );
    dialogs.answer_next_save(Ok(Some(chosen.clone())));

    let answered = land(
        handle.save(
            SaveRequest::new()
                .filter(FileFilter::new("Text").with_extension("txt"))
                .contents(b"written".to_vec()),
        ),
    );
    turn_until_landed(&mut host, &answered);

    assert_eq!(*answered.borrow(), Some(Ok(Ok(Some(chosen)))));
    assert_eq!(
        std::fs::read(folder.path().join("notes.txt")).expect("the file was written"),
        b"written"
    );
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn a_save_whose_write_fails_answers_the_systems_failure() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let folder = tempfile::tempdir().expect("a test folder");
    let unwritable = folder.path().join("missing").join("notes.txt");
    dialogs.answer_next_save(Ok(Some(SaveLocation::new(
        PickedFile::try_new(&unwritable).expect("an absolute path"),
        None,
    ))));

    let answered = land(handle.save(SaveRequest::new().contents(b"lost".to_vec())));
    turn_until_landed(&mut host, &answered);

    let failure = match answered.borrow_mut().take() {
        Some(Ok(Err(DialogError::Failed(failure)))) => failure,
        other => panic!("the write's failure: {other:?}"),
    };
    assert!(
        matches!(failure, SystemFailure::Os(code) if code != 0),
        "the system's code: {failure:?}"
    );
    assert!(!unwritable.exists());
}

#[test]
fn a_save_the_system_wrote_itself_answers_the_file_it_wrote() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let written = SaveLocation::new(
        PickedFile::try_from_contents("notes.txt", Arc::from(&b"written"[..]))
            .expect("a file name"),
        None,
    );
    dialogs.answer_next_save(Ok(Some(written.clone())));

    let answered = land(handle.save(SaveRequest::new().contents(b"written".to_vec())));
    turn(&mut host);
    turn(&mut host);

    assert_eq!(*answered.borrow(), Some(Ok(Ok(Some(written)))));
}

#[test]
fn a_file_handed_over_without_a_path_carries_its_name_and_contents() {
    let contents: Arc<[u8]> = Arc::from(&b"private words"[..]);
    let handed =
        PickedFile::try_from_contents("notes.txt", Arc::clone(&contents)).expect("a file name");
    assert_eq!(handed.path(), None);
    assert_eq!(handed.contents(), Some(&b"private words"[..]));
    assert_eq!(handed.file_name(), Some(OsStr::new("notes.txt")));
    let shown = format!("{handed:?}");
    assert!(
        shown.contains("notes.txt") && shown.contains("13") && !shown.contains("private"),
        "a pick shows its name and length, never its contents: {shown}"
    );
    for name in ["", " ", ".", "..", "a/b", "a\\b", "a\nb"] {
        assert_eq!(
            PickedFile::try_from_contents(name, Arc::clone(&contents)),
            None,
            "{name:?} is not a file name"
        );
    }

    let at_path = file("notes.txt");
    assert_eq!(at_path.contents(), None);
    assert_eq!(at_path.file_name(), Some(OsStr::new("notes.txt")));
    let request = format!(
        "{:?}",
        SaveRequest::new().contents(b"private words".to_vec())
    );
    assert!(
        request.contains("13") && !request.contains("112"),
        "a request shows its contents' length, never its bytes: {request}"
    );
}

#[test]
fn a_pick_says_whether_it_is_at_a_path_handed_over_or_a_download_of_unknown_fate() {
    let contents: Arc<[u8]> = Arc::from(&b"words"[..]);
    let handed =
        PickedFile::try_from_contents("notes.txt", Arc::clone(&contents)).expect("a file name");
    let downloaded = PickedFile::try_download("notes.txt").expect("a file name");
    let at_path = file("notes.txt");

    assert_eq!(
        handed.form(),
        PickedForm::Contents {
            name: "notes.txt",
            contents: b"words"
        }
    );
    assert_eq!(
        downloaded.form(),
        PickedForm::Download { name: "notes.txt" }
    );
    assert_eq!(downloaded.contents(), None, "nobody saw a download written");
    assert_eq!(downloaded.path(), None);
    assert_eq!(downloaded.file_name(), Some(OsStr::new("notes.txt")));
    assert_eq!(
        at_path.form(),
        PickedForm::Path(at_path.path().expect("a path"))
    );
    assert_eq!(PickedFile::try_download("a/b"), None);
}

#[test]
fn a_filter_means_a_files_name_or_its_type_ignoring_ascii_case() {
    let images = FileFilter::new("Images")
        .with_extension("png")
        .with_extension("tar.gz")
        .with_mime_type("image/*")
        .with_mime_type("application/pdf");
    let cases: [(&str, Option<&str>, bool); 11] = [
        ("photo.png", None, true),
        ("PHOTO.PNG", None, true),
        ("a.TAR.GZ", None, true),
        ("png", None, false),
        (".png", None, false),
        ("photo.png.zip", None, false),
        ("scan", Some("image/jpeg"), true),
        ("scan", Some("IMAGE/HEIC"), true),
        ("doc", Some("application/pdf; charset=binary"), true),
        ("doc", Some("application/zip"), false),
        ("doc", Some(""), false),
    ];
    for (name, mime_type, matches) in cases {
        assert_eq!(
            images.matches(name, mime_type),
            matches,
            "{name:?} of {mime_type:?}"
        );
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn a_save_replaces_the_chosen_file_whole_and_leaves_nothing_beside_it() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let folder = tempfile::tempdir().expect("a test folder");
    let target = folder.path().join("notes.txt");
    std::fs::write(&target, b"the old, longer contents").expect("a file to replace");
    // A second name for the old file: a file written in place would change
    // under it, a replaced one does not.
    let old = folder.path().join("old");
    std::fs::hard_link(&target, &old).expect("a hard link");
    dialogs.answer_next_save(Ok(Some(SaveLocation::new(
        PickedFile::try_new(&target).expect("an absolute path"),
        None,
    ))));

    let answered = land(handle.save(SaveRequest::new().contents(b"new".to_vec())));
    turn_until_landed(&mut host, &answered);

    assert!(matches!(&*answered.borrow(), Some(Ok(Ok(Some(_))))));
    assert_eq!(std::fs::read(&target).expect("the file"), b"new");
    assert_eq!(
        std::fs::read(&old).expect("the old file"),
        b"the old, longer contents",
        "the chosen name now names a new file; the old one was never truncated"
    );
    let mut names: Vec<std::ffi::OsString> = std::fs::read_dir(folder.path())
        .expect("the folder")
        .map(|entry| entry.expect("an entry").file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["notes.txt", "old"], "no temporary file is left");
}
