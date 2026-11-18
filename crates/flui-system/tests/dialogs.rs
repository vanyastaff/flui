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
    FileFilter, FolderFile, FolderPath, MAX_TEXT_LEN, MalformedRequest, OpenRequest, OpenResult,
    PickedFile, PickedForm, Picks, SaveLocation, SaveRequest, SaveResult,
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

/// A folder a system handed over without a path, holding one file.
fn handed_folder() -> PickedFile {
    let notes = folder_file("notes.txt", b"words");
    PickedFile::try_from_folder("Project", vec![notes]).expect("a folder")
}

#[test]
fn a_pick_of_a_form_the_request_cannot_have_is_a_malformed_answer() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let handed_file = || {
        PickedFile::try_from_contents("notes.txt", Arc::from(&b"words"[..])).expect("a file name")
    };
    let download = || PickedFile::try_download("notes.txt").expect("a file name");

    let opens: [(OpenRequest, PickedFile, bool); 8] = [
        (OpenRequest::files(), file("notes.txt"), true),
        (OpenRequest::files(), handed_file(), true),
        (OpenRequest::files(), handed_folder(), false),
        (OpenRequest::files(), download(), false),
        // Whether a path names a file or a folder is the file system's to say.
        (OpenRequest::folders(), file("Project"), true),
        (OpenRequest::folders(), handed_folder(), true),
        (OpenRequest::folders(), handed_file(), false),
        (OpenRequest::folders(), download(), false),
    ];
    for (request, pick, admitted) in opens {
        let picked = Picks::try_new(vec![pick]).expect("one pick");
        dialogs.answer_next_open(Ok(Some(picked.clone())));
        let shown = format!("{request:?} answered {picked:?}");
        let answered = land(handle.open(request));
        turn(&mut host);
        turn(&mut host);
        let expected = if admitted {
            Ok(Some(picked))
        } else {
            Err(DialogError::Failed(SystemFailure::MalformedAnswer))
        };
        assert_eq!(*answered.borrow(), Some(Ok(expected)), "{shown}");
    }

    // A save writes a file: a download is its answer, a folder is not.
    for (pick, admitted) in [(download(), true), (handed_folder(), false)] {
        let location = SaveLocation::new(pick, None);
        dialogs.answer_next_save(Ok(Some(location.clone())));
        let answered = land(handle.save(SaveRequest::new()));
        turn(&mut host);
        turn(&mut host);
        let expected = if admitted {
            Ok(Some(location))
        } else {
            Err(DialogError::Failed(SystemFailure::MalformedAnswer))
        };
        assert_eq!(*answered.borrow(), Some(Ok(expected)));
    }
}

#[test]
fn a_pick_whose_name_ends_in_no_extension_of_the_filters_refuses_the_whole_answer() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let by_name = || {
        vec![
            FileFilter::new("Text").with_extension("txt"),
            FileFilter::new("Archives").with_extension("tar.gz"),
        ]
    };
    let by_name_or_type = || {
        vec![
            FileFilter::new("Text")
                .with_extension("txt")
                .with_mime_type("text/*"),
        ]
    };
    // "All files" offered beside a specific filter, as an application does
    // so the user can reach a file the filter does not name.
    let with_all_files = || {
        vec![
            FileFilter::new("Text").with_extension("txt"),
            FileFilter::all_files("All files"),
        ]
    };
    let by_file_name = || {
        vec![
            FileFilter::new("Build files")
                .with_file_name("Makefile")
                .with_file_name(".env")
                .with_extension("mk"),
        ]
    };
    let cases: [(Vec<FileFilter>, &[&str], bool); 19] = [
        // Whichever filter was selected, the request offered every file.
        (with_all_files(), &["README"], true),
        (with_all_files(), &["notes.txt", "run.command"], true),
        // A name with no extension to match by, matched whole.
        (by_file_name(), &["Makefile"], true),
        (by_file_name(), &["makefile", "MAKEFILE", "rules.mk"], true),
        // A dot and a word: a hidden file's name, which no extension names.
        (by_file_name(), &[".env"], true),
        (by_file_name(), &["Makefile.txt"], false),
        (by_file_name(), &["GNUMakefile"], false),
        (by_file_name(), &["prod.env"], false),
        (by_file_name(), &["Makefile", "notes.txt"], false),
        (by_name(), &[".txt"], false),
        (by_name(), &["notes.txt"], true),
        (by_name(), &["NOTES.TXT", "backup.Tar.Gz"], true),
        // A type that conforms to an allowed one is not one of its names.
        (by_name(), &["main.swift"], false),
        // A dialog that matched the last part of `tar.gz` only.
        (by_name(), &["notes.gz"], false),
        (by_name(), &["notes.txt", "run.command"], false),
        (by_name(), &["txt"], false),
        // A file's type is its system's to tell: the backend judged it.
        (by_name_or_type(), &["main.swift"], true),
        (Vec::new(), &["anything"], true),
        (by_name(), &["notes.txt.zip"], false),
    ];
    for (filters, names, admitted) in cases {
        let picked = picks(names);
        dialogs.answer_next_open(Ok(Some(picked.clone())));
        let mut request = OpenRequest::files().allow_multiple();
        request.filters = filters;
        let answered = land(handle.open(request));
        turn(&mut host);
        turn(&mut host);
        let expected = if admitted {
            Ok(Some(picked))
        } else {
            Err(DialogError::NotInFilter)
        };
        assert_eq!(*answered.borrow(), Some(Ok(expected)), "{names:?}");
    }
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

    let build = FileFilter::new("Build files")
        .with_file_name("Makefile")
        .with_file_name("makefile")
        .with_file_name(".env");
    assert_eq!(
        build.file_names(),
        ["Makefile", ".env"],
        "a file name is kept as written, and once whatever its case"
    );
    assert!(!build.is_all_files());

    let every = FileFilter::all_files("All files")
        .with_extension("png")
        .with_file_name("Makefile")
        .with_mime_type("image/*");
    assert!(every.is_all_files());
    assert!(
        every.extensions().is_empty()
            && every.file_names().is_empty()
            && every.mime_types().is_empty(),
        "a filter of every file holds no pattern"
    );
    assert_ne!(
        every,
        FileFilter::all_files("All files"),
        "that it was given patterns is kept, and a request offering it is refused for them"
    );
    assert_ne!(
        FileFilter::all_files("Files"),
        FileFilter::new("Files"),
        "every file is not the filter nothing was added to"
    );
}

#[test]
fn a_request_carries_its_wish_for_hidden_files_to_the_dialog() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    assert!(!OpenRequest::files().show_hidden);
    assert!(!SaveRequest::new().show_hidden);

    dialogs.answer_next_open(Ok(None));
    let opened = land(handle.open(OpenRequest::folders().show_hidden()));
    turn(&mut host);
    dialogs.answer_next_save(Ok(None));
    let saved = land(handle.save(SaveRequest::new().show_hidden()));
    turn(&mut host);
    assert_eq!(*opened.borrow(), Some(Ok(Ok(None))));
    assert_eq!(*saved.borrow(), Some(Ok(Ok(None))));

    let shown = dialogs
        .requests()
        .iter()
        .map(|asked| match asked {
            DialogRequest::Open { request, .. } => request.show_hidden,
            DialogRequest::Save { request, .. } => request.show_hidden,
            other => panic!("an open or a save dialog: {other:?}"),
        })
        .collect::<Vec<bool>>();
    assert_eq!(shown, [true, true], "each dialog was asked to show them");
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
        // A pattern given to a filter of every file would be dropped
        // unseen: the filter would not mean what its author wrote.
        (
            OpenRequest::files().filter(FileFilter::all_files("All files").with_extension("png")),
            MalformedRequest::PatternOnAllFiles,
        ),
        (
            OpenRequest::files()
                .filter(FileFilter::all_files("All files").with_file_name("Makefile")),
            MalformedRequest::PatternOnAllFiles,
        ),
        (
            OpenRequest::files()
                .filter(FileFilter::all_files("All files").with_mime_type("image/*")),
            MalformedRequest::PatternOnAllFiles,
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
        // A file name is matched whole, so it holds no wildcard, separator
        // or space, is not a folder's own name, and ends in no dot, which
        // Windows drops.
        (
            OpenRequest::files().filter(FileFilter::new("Glob").with_file_name("Make*")),
            MalformedRequest::FileName,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Path").with_file_name("src/Makefile")),
            MalformedRequest::FileName,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("List").with_file_name("a;b")),
            MalformedRequest::FileName,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Space").with_file_name("READ ME")),
            MalformedRequest::FileName,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("None").with_file_name("")),
            MalformedRequest::FileName,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Here").with_file_name(".")),
            MalformedRequest::FileName,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Up").with_file_name("..")),
            MalformedRequest::FileName,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Dot").with_file_name("notes.")),
            MalformedRequest::FileName,
        ),
        (
            OpenRequest::files().filter(FileFilter::new("Long").with_file_name(too_long.as_str())),
            MalformedRequest::FileName,
        ),
        (
            OpenRequest::files().filter((0..FileFilter::MAX_PATTERNS).fold(
                FileFilter::new("Many").with_file_name("Makefile"),
                |filter, index| filter.with_extension(format!("e{index}")),
            )),
            MalformedRequest::TooManyPatterns,
        ),
        (
            OpenRequest::files().filter(FileFilter::all_files(" ")),
            MalformedRequest::FilterName,
        ),
        (
            OpenRequest::folders().filter(FileFilter::all_files("All files")),
            MalformedRequest::FiltersOnFolders,
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

    // A filter of every file needs no pattern, and a file name may start
    // with a dot, hold several and be as long as a pattern may be.
    dialogs.answer_next_open(Ok(None));
    let named = land(
        handle.open(
            OpenRequest::files()
                .filter(FileFilter::all_files("All files"))
                .filter(
                    FileFilter::new("Named")
                        .with_file_name("Makefile")
                        .with_file_name(".env.local")
                        .with_file_name("CMakeLists.txt")
                        .with_file_name("my_file-2+")
                        .with_file_name(longest.as_str()),
                ),
        ),
    );
    turn(&mut host);
    assert_eq!(*named.borrow(), Some(Ok(Ok(None))));

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

/// The file at `path` under a handed-over folder, holding `contents`.
fn folder_file(path: &str, contents: &'static [u8]) -> FolderFile {
    FolderFile::new(
        FolderPath::try_new(path).expect("a path under a folder"),
        Arc::from(contents),
    )
}

#[test]
fn a_folder_handed_over_without_a_path_carries_its_name_and_its_files_where_they_are() {
    // In no order, as a browser lists a folder.
    let folder = PickedFile::try_from_folder(
        "Project",
        vec![
            folder_file("src/main.rs", b"fn main() {}"),
            folder_file("src.txt", b"beside the folder"),
            folder_file("README.md", b"private words"),
            folder_file("src/lib.rs", b""),
        ],
    )
    .expect("a folder");

    assert_eq!(folder.path(), None, "a browser hands over no path");
    assert_eq!(folder.contents(), None, "a folder's files hold theirs");
    assert_eq!(folder.file_name(), Some(OsStr::new("Project")));
    let files = folder.files().expect("a folder has files");
    assert_eq!(
        folder.form(),
        PickedForm::Folder {
            name: "Project",
            files
        }
    );
    let read: Vec<(&str, &[u8])> = files
        .iter()
        .map(|file| (file.path().as_str(), file.contents()))
        .collect();
    assert_eq!(
        read,
        [
            ("README.md", &b"private words"[..]),
            ("src/lib.rs", &b""[..]),
            ("src/main.rs", &b"fn main() {}"[..]),
            ("src.txt", &b"beside the folder"[..]),
        ],
        "name by name from the folder down, whatever order they were listed in: \
         a folder's files come before the file named after it"
    );
    let shown = format!("{folder:?} {files:?}");
    assert!(
        shown.contains("Project") && shown.contains("README.md") && !shown.contains("private"),
        "a folder shows its name and its files' paths, never what they hold: {shown}"
    );

    for name in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
        assert_eq!(
            PickedFile::try_from_folder(name, Vec::new()),
            None,
            "{name:?} names no folder"
        );
    }
    assert_eq!(
        PickedFile::try_from_folder(
            "Project",
            vec![folder_file("a.txt", b"1"), folder_file("a.txt", b"2")]
        ),
        None,
        "two files at one path"
    );
    assert_eq!(
        PickedFile::try_from_folder("Empty", Vec::new())
            .expect("a folder")
            .files(),
        Some(&[][..]),
        "a folder may hold no file"
    );
    assert_eq!(
        file("Project").files(),
        None,
        "a folder at a path is read by its application"
    );
}

#[test]
fn a_path_under_a_folder_is_its_names_and_none_climbs_out() {
    let path = FolderPath::try_new("src/web/mod.rs").expect("a path");
    assert_eq!(path.as_str(), "src/web/mod.rs");
    assert_eq!(
        path.components().collect::<Vec<&str>>(),
        ["src", "web", "mod.rs"]
    );
    assert_eq!(path.file_name(), "mod.rs");

    // As a walk builds it: an entry of the folder, then entries under it.
    let walked = FolderPath::try_from_name("src")
        .and_then(|folder| folder.try_child("web"))
        .and_then(|folder| folder.try_child("mod.rs"));
    assert_eq!(walked, Some(path));
    assert_eq!(
        FolderPath::try_from_name("README.md").map(|path| path.file_name().to_owned()),
        Some("README.md".to_owned())
    );

    // The same rule on every host: no empty name, none that stays or climbs,
    // no separator of any host, no NUL.
    for path in [
        "", "/a", "a/", "a//b", ".", "a/./b", "..", "a/../b", "a\\b", "a\0b",
    ] {
        assert_eq!(FolderPath::try_new(path), None, "{path:?} is not a path");
    }
    let folder = FolderPath::try_from_name("src").expect("a name");
    for name in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
        assert_eq!(FolderPath::try_from_name(name), None, "{name:?}");
        assert_eq!(folder.try_child(name), None, "{name:?}");
    }
    // A folder holds what its file system allows, and a name is kept as it
    // is on every host, whatever one of them would read into it.
    for path in [
        " ",
        "docs/Icon\r",
        ".git/config",
        "NUL",
        "C:",
        "a.",
        "aux.c",
    ] {
        assert_eq!(
            FolderPath::try_new(path).as_ref().map(FolderPath::as_str),
            Some(path),
            "{path:?} is a path under a folder"
        );
    }
}

#[test]
fn what_a_system_hands_over_is_held_to_the_requests_bounds_on_every_host() {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    let three_files = || {
        PickedFile::try_from_folder(
            "Project",
            vec![
                folder_file("a.txt", b"12"),
                folder_file("src/b.txt", b"345"),
                folder_file("src/c.txt", b"6789"),
            ],
        )
        .expect("a folder")
    };
    let handed = |name: &str, contents: &'static [u8]| {
        PickedFile::try_from_contents(name, Arc::from(contents)).expect("a file name")
    };

    let cases: [(OpenRequest, Vec<PickedFile>, Result<(), DialogError>); 8] = [
        (
            OpenRequest::folders().max_entries(2),
            vec![three_files()],
            Err(DialogError::TooManyEntries { limit: 2 }),
        ),
        (
            OpenRequest::folders().max_entries(3),
            vec![three_files()],
            Ok(()),
        ),
        (
            OpenRequest::folders().max_bytes(8),
            vec![three_files()],
            Err(DialogError::TooLarge {
                actual: 9,
                limit: 8,
            }),
        ),
        (
            OpenRequest::folders().max_bytes(9),
            vec![three_files()],
            Ok(()),
        ),
        (
            OpenRequest::files().allow_multiple().max_bytes(8),
            vec![handed("a.txt", b"12345"), handed("b.txt", b"6789")],
            Err(DialogError::TooLarge {
                actual: 9,
                limit: 8,
            }),
        ),
        (
            OpenRequest::files().allow_multiple().max_bytes(9),
            vec![handed("a.txt", b"12345"), handed("b.txt", b"6789")],
            Ok(()),
        ),
        // A pick at a path is read by its application: nothing bounds it.
        (
            OpenRequest::folders().max_entries(0).max_bytes(0),
            vec![file("Project")],
            Ok(()),
        ),
        (
            OpenRequest::folders().max_entries(0).max_bytes(0),
            vec![PickedFile::try_from_folder("Empty", Vec::new()).expect("a folder")],
            Ok(()),
        ),
    ];
    for (request, picks, admitted) in cases {
        let picked = Picks::try_new(picks).expect("the case names a pick");
        dialogs.answer_next_open(Ok(Some(picked.clone())));
        let shown = format!("{request:?} answered {picked:?}");
        let answered = land(handle.open(request));
        turn(&mut host);
        turn(&mut host);
        assert_eq!(
            *answered.borrow(),
            Some(Ok(admitted.map(|()| Some(picked)))),
            "{shown}"
        );
    }
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

    let build = FileFilter::new("Build files")
        .with_file_name("Makefile")
        .with_file_name(".env");
    let named: [(&str, Option<&str>, bool); 8] = [
        ("Makefile", None, true),
        ("MAKEFILE", None, true),
        (".env", None, true),
        (".ENV", Some("text/plain"), true),
        ("Makefile.txt", None, false),
        ("aMakefile", None, false),
        ("env", None, false),
        ("Makefil", Some("text/x-makefile"), false),
    ];
    for (name, mime_type, matches) in named {
        assert_eq!(
            build.matches(name, mime_type),
            matches,
            "{name:?} of {mime_type:?}"
        );
    }

    let every = FileFilter::all_files("All files");
    for (name, mime_type) in [
        ("README", None),
        (".hidden", None),
        ("run.exe", Some("application/x-msdownload")),
        ("", None),
    ] {
        assert!(every.matches(name, mime_type), "{name:?} of {mime_type:?}");
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

/// Save `contents` through a scripted dialog whose user chose `chosen`, and
/// answer what the save answered once the write ended.
#[cfg(any(unix, windows))]
fn save_to(chosen: &std::path::Path, contents: &[u8]) -> SaveResult {
    let (dialogs, plugin) = scripted();
    let (mut host, handle) = acquire::<FileDialogCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides dialogs");
    dialogs.answer_next_save(Ok(Some(SaveLocation::new(
        PickedFile::try_new(chosen).expect("an absolute path"),
        None,
    ))));
    let answered = land(handle.save(SaveRequest::new().contents(contents.to_vec())));
    turn_until_landed(&mut host, &answered);
    let outcome = answered.borrow_mut().take();
    outcome
        .expect("the loop above saw it land")
        .expect("the host is not retired")
}

/// The names in `folder`, sorted.
#[cfg(any(unix, windows))]
fn names_in(folder: &std::path::Path) -> Vec<std::ffi::OsString> {
    let mut names: Vec<std::ffi::OsString> = std::fs::read_dir(folder)
        .expect("the folder")
        .map(|entry| entry.expect("an entry").file_name())
        .collect();
    names.sort();
    names
}

#[test]
#[cfg(unix)]
fn a_save_to_a_symlink_updates_its_target() {
    let folder = tempfile::tempdir().expect("a test folder");
    let elsewhere = folder.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("a folder for the target");
    let target = elsewhere.join("notes.txt");
    std::fs::write(&target, b"old").expect("the linked file");
    // Relative, as a link beside its target's folder is written.
    let link = folder.path().join("link.txt");
    std::os::unix::fs::symlink("elsewhere/notes.txt", &link).expect("a link");

    let saved = save_to(&link, b"new");

    assert!(matches!(saved, Ok(Some(_))), "{saved:?}");
    assert_eq!(std::fs::read(&target).expect("the target"), b"new");
    assert!(
        std::fs::symlink_metadata(&link)
            .expect("the link")
            .file_type()
            .is_symlink(),
        "the link the user chose is still a link, not a file in its place"
    );
    assert_eq!(names_in(folder.path()), ["elsewhere", "link.txt"]);
    assert_eq!(
        names_in(&elsewhere),
        ["notes.txt"],
        "no temporary file is left"
    );

    // A link whose file is not there yet names where it is made.
    let dangling = folder.path().join("later.txt");
    std::os::unix::fs::symlink(elsewhere.join("made.txt"), &dangling).expect("a link");
    let saved = save_to(&dangling, b"made");
    assert!(matches!(saved, Ok(Some(_))), "{saved:?}");
    assert_eq!(
        std::fs::read(elsewhere.join("made.txt")).expect("the file"),
        b"made"
    );
    assert!(
        std::fs::symlink_metadata(&dangling)
            .expect("the link")
            .file_type()
            .is_symlink()
    );
}

#[test]
#[cfg(unix)]
fn a_save_keeps_the_mode_of_the_file_it_replaces() {
    use std::os::unix::fs::PermissionsExt as _;

    let folder = tempfile::tempdir().expect("a test folder");
    let mode_of = |path: &std::path::Path| {
        std::fs::metadata(path)
            .expect("the file")
            .permissions()
            .mode()
            & 0o7777
    };
    // Two modes, so at most one is what a new file gets by default.
    for mode in [0o600, 0o664] {
        let target = folder.path().join(format!("notes-{mode:o}.txt"));
        std::fs::write(&target, b"old").expect("a file to replace");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode)).expect("its mode");
        // A second name for the old file: the save replaced it, so the mode
        // is the new file's, given to it, not the old file's kept by a
        // write in place.
        let old = folder.path().join(format!("old-{mode:o}"));
        std::fs::hard_link(&target, &old).expect("a hard link");

        let saved = save_to(&target, b"new");

        assert!(matches!(saved, Ok(Some(_))), "{saved:?}");
        assert_eq!(std::fs::read(&target).expect("the file"), b"new");
        assert_eq!(std::fs::read(&old).expect("the old file"), b"old");
        assert_eq!(mode_of(&target), mode, "{mode:o}");
    }
}

/// A name within the file system's limit whose sibling's name is past it:
/// no new file can be made beside it, whoever runs the test.
#[test]
#[cfg(unix)]
fn a_save_where_no_file_can_be_made_beside_the_chosen_one_writes_into_it() {
    let folder = tempfile::tempdir().expect("a test folder");
    let target = folder.path().join("n".repeat(250));
    std::fs::write(&target, b"the old, longer contents").expect("a file of that name");
    // A second name for the file: one written into changes under it.
    let same = folder.path().join("same");
    std::fs::hard_link(&target, &same).expect("a hard link");

    let saved = save_to(&target, b"new");

    assert!(matches!(saved, Ok(Some(_))), "{saved:?}");
    assert_eq!(std::fs::read(&target).expect("the file"), b"new");
    assert_eq!(
        std::fs::read(&same).expect("the same file"),
        b"new",
        "the chosen file itself was written, cut to the new length"
    );
    assert_eq!(names_in(folder.path()).len(), 2, "nothing else was made");
}

/// A file another process holds open without letting it be deleted (what a
/// program keeping a document open does on Windows) cannot be replaced: the
/// save answers the system's failure and the file keeps every byte, where
/// writing into it would empty it first.
#[test]
#[cfg(windows)]
fn a_save_over_a_file_held_open_fails_and_leaves_it_whole() {
    use std::os::windows::fs::OpenOptionsExt as _;

    /// `FILE_SHARE_READ | FILE_SHARE_WRITE`: no `FILE_SHARE_DELETE`.
    const SHARE_READ_WRITE: u32 = 0x1 | 0x2;

    let folder = tempfile::tempdir().expect("a test folder");
    let target = folder.path().join("notes.txt");
    std::fs::write(&target, b"the old, longer contents").expect("a file to replace");
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ_WRITE)
        .open(&target)
        .expect("the file held open");

    let saved = save_to(&target, b"new");

    assert!(
        matches!(saved, Err(DialogError::Failed(SystemFailure::Os(_)))),
        "{saved:?}"
    );
    drop(held);
    assert_eq!(
        std::fs::read(&target).expect("the file"),
        b"the old, longer contents",
        "the file was neither replaced nor written into"
    );
    assert_eq!(
        names_in(folder.path()),
        ["notes.txt"],
        "the new file beside it was removed"
    );
}

/// The new file a save writes beside the chosen one is never readable by
/// more users than the file it replaces, at any moment of the write: it is
/// made with that file's mode, not given it once the contents are in.
#[test]
#[cfg(unix)]
fn a_saves_new_file_is_never_more_readable_than_the_file_it_replaces() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::atomic::{AtomicBool, Ordering};

    let folder = tempfile::tempdir().expect("a test folder");
    let target = folder.path().join("secret.txt");
    std::fs::write(&target, b"old").expect("a file to replace");
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).expect("its mode");

    // Large enough that the write and its sync outlast many reads of the
    // folder.
    let contents = vec![b'x'; 256 << 20];
    let done = Arc::new(AtomicBool::new(false));
    let watching = Arc::clone(&done);
    let watched = folder.path().to_path_buf();
    let watcher = std::thread::spawn(move || {
        let mut modes = Vec::new();
        while !watching.load(Ordering::Acquire) {
            let Ok(entries) = std::fs::read_dir(&watched) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                if name.to_string_lossy().ends_with(".flui-save")
                    && let Ok(metadata) = entry.metadata()
                {
                    modes.push(metadata.permissions().mode() & 0o7777);
                }
            }
        }
        modes
    });

    let saved = save_to(&target, &contents);
    done.store(true, Ordering::Release);
    let modes = watcher.join().expect("the watcher ran to its end");

    assert!(matches!(saved, Ok(Some(_))), "{saved:?}");
    assert!(
        !modes.is_empty(),
        "the new file was seen while the contents were written"
    );
    assert!(
        modes.iter().all(|mode| mode & !0o600 == 0),
        "the new file was readable beyond its owner: {:?}",
        modes
            .iter()
            .map(|mode| format!("{mode:o}"))
            .collect::<Vec<String>>()
    );
}
