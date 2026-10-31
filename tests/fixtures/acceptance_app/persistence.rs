//! Persistence rows: what a restart over the same storage finds.

use std::cell::{Cell, RefCell};
use std::future::poll_fn;
use std::io::ErrorKind;
use std::rc::Rc;
use std::task::{Poll, Waker};

use flui::platform::StorageError;
use flui::platform::keyboard::NamedKey;
use flui::testing::storage::MemoryStorage;
use flui::view::persist::Document;
use serde_json::{Value, json};

use crate::app::{AcceptanceApp, Faults, Loader, Saved, Session};
use crate::testing::AppRun;

/// More frames than a load over memory storage takes.
const LOAD_FRAMES: usize = 16;

fn app() -> AcceptanceApp {
    AcceptanceApp::new(Faults::NONE)
}

#[test]
fn saved_title_survives_restart_and_others_stay() {
    let mut run = AppRun::start(app());
    run.tap("Note 2");
    run.replace_title("Groceries");
    run.tap("Save note");
    assert!(run.shows("Saved note 2"));
    run.tap("Back");

    let mut run = run.restart();
    assert!(run.shows("Groceries"), "the saved title is read back");
    assert!(!run.shows("Note 2"));
    assert!(run.shows("Note 1"), "the other notes are unchanged");
    assert!(run.shows("Note 3"));

    run.tap("Groceries");
    assert_eq!(
        run.title(),
        "Groceries",
        "the editor opens on the stored title"
    );
}

#[test]
fn compact_rows_survive_restart() {
    let mut run = AppRun::start(app());
    run.tap("Compact rows: off");
    assert!(run.shows("Compact rows: on"));

    let mut run = run.restart();
    assert!(run.shows("Compact rows: on"), "the flag is read back");
    run.tap("Settings");
    assert!(run.shows("Compact rows are on"));
}

#[test]
fn first_launch_starts_from_initial_state_without_an_error() {
    let mut run = AppRun::start(app());
    assert!(run.shows("Note 1"), "Home lists the initial notes");
    assert!(run.shows("Compact rows: off"));
    assert!(
        !run.shows("Retry"),
        "an empty storage is a first launch, not a failure"
    );
    assert_eq!(
        run.storage().contents(&Saved::NAME),
        None,
        "reading the initial state writes nothing"
    );

    run.tap("Compact rows: off");
    let stored = run
        .storage()
        .contents(&Saved::NAME)
        .expect("the first edit writes the document");
    let stored = String::from_utf8_lossy(&stored);
    assert!(
        stored.contains(r#""title":"Note 40""#) && stored.contains(r#""compact":true"#),
        "the first write is the initial state with the edit: {stored}"
    );
}

#[test]
fn a_note_id_is_never_reused_across_a_restart() {
    let mut run = AppRun::start(app());
    run.tap("New note");
    assert!(run.shows("Editing note 41"));
    run.tap("Back");

    let mut run = run.restart();
    run.tap("New note");
    assert!(
        run.shows("Editing note 42"),
        "an id is never handed out twice, across runs too"
    );
}

/// A loader the test opens: until then the load waits on the gate.
#[derive(Default)]
struct Gate {
    open: Cell<bool>,
    waiting: RefCell<Option<Waker>>,
}

impl Gate {
    fn set_open(&self, open: bool) {
        self.open.set(open);
        if let Some(waker) = self.waiting.take() {
            waker.wake();
        }
    }
}

fn gated() -> (AcceptanceApp, Rc<Gate>) {
    let gate = Rc::new(Gate::default());
    let held = Rc::clone(&gate);
    let loader: Loader = Rc::new(move |_attempt| {
        let gate = Rc::clone(&held);
        Box::pin(poll_fn(move |cx| {
            if gate.open.get() {
                return Poll::Ready(Ok(()));
            }
            *gate.waiting.borrow_mut() = Some(cx.waker().clone());
            Poll::Pending
        }))
    });
    (app().with_loader(loader), gate)
}

/// Rewrite the stored session's body as JSON with `edit`, keeping its header.
fn edit_session(storage: &MemoryStorage, edit: impl FnOnce(&mut Value)) {
    let stored = storage
        .contents(&Session::NAME)
        .expect("the session was written");
    let split = stored
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("a stored document starts with a header line")
        + 1;
    let (header, body) = stored.split_at(split);
    let mut session: Value = serde_json::from_slice(body).expect("the session is JSON");
    edit(&mut session);
    let mut edited = header.to_vec();
    edited.extend(serde_json::to_vec(&session).expect("JSON serializes"));
    storage.put(&Session::NAME, edited);
}

/// A copy of what `storage` holds, for a launch that must not touch it.
fn copy_of(storage: &MemoryStorage) -> MemoryStorage {
    let copy = MemoryStorage::new();
    for name in [Saved::NAME, Session::NAME] {
        if let Some(bytes) = storage.contents(&name) {
            copy.put(&name, bytes);
        }
    }
    copy
}

#[test]
fn unsaved_draft_reopens_its_note_without_changing_the_list() {
    let mut run = AppRun::start(app());
    run.tap("Note 2");
    run.replace_title("Groceries");

    let mut run = run.restart();
    assert!(run.shows("Editing note 2"), "the editor is open again");
    assert_eq!(run.title(), "Groceries", "with the unsaved draft");
    run.tap("Back");
    assert!(
        run.shows("Note 2") && !run.shows("Groceries"),
        "the list keeps the saved title until Save"
    );
}

#[test]
fn settings_over_editor_restores_and_backs_out_twice() {
    let mut run = AppRun::start(app());
    run.tap("Note 5");
    run.tap("Settings");

    let mut run = run.restart();
    assert!(
        run.shows("Compact rows are off"),
        "Settings is on top again"
    );
    run.tap("Back");
    assert!(
        run.shows("Editing note 5"),
        "the first Back leads to the note"
    );
    run.tap("Back");
    assert!(run.shows("Note 1") && !run.shows("Editing note 5"));
    assert!(
        !run.shows("Back"),
        "the second leads to Home, the bottom page"
    );
}

#[test]
fn list_offset_and_route_are_restored_on_the_first_loaded_frame() {
    let (app, gate) = gated();
    gate.set_open(true);
    let mut run = AppRun::start(app);
    run.shift_tab();
    run.key(NamedKey::End);
    let offset = run.list_offset();
    assert!(offset > 0.0, "End scrolls the list");
    run.tap("Note 40");
    run.replace_title("Last");

    gate.set_open(false);
    let mut run = run.restart();
    assert!(run.shows("Loading notes"));
    gate.set_open(true);
    for _ in 0..LOAD_FRAMES {
        if !run.shows("Loading notes") {
            break;
        }
        run.laid().pump();
    }
    assert!(!run.shows("Loading notes"), "the documents load");
    assert!(
        run.shows("Editing note 40"),
        "the first loaded frame shows the restored route"
    );
    assert_eq!(run.title(), "Last", "and the restored draft");
    let restored = run.list_offset();
    assert!(
        (restored - offset).abs() <= 1.0,
        "and the restored offset: {restored} for {offset}"
    );

    run.tap("Back");
    assert!(run.shows("Note 40") && !run.shows("Note 1"));
}

#[test]
fn failed_data_load_leaves_session_unread_and_unwritten() {
    let mut run = AppRun::start(app());
    run.tap("Note 2");
    run.replace_title("Groceries");
    let session = run.storage().contents(&Session::NAME);
    assert!(session.is_some(), "the draft writes the session");

    run.storage().fail_reads(Some(StorageError::Inaccessible {
        kind: ErrorKind::PermissionDenied,
    }));
    let mut run = run.restart();
    assert!(run.shows("Retry"), "a failed data load shows Retry");
    assert!(!run.shows("Editing note 2"), "and applies no session");
    assert_eq!(
        run.storage().contents(&Session::NAME),
        session,
        "nor writes one"
    );

    run.storage().fail_reads(None);
    run.tap("Retry");
    assert!(
        run.shows("Editing note 2"),
        "a load that succeeds applies it"
    );
    assert_eq!(run.title(), "Groceries");
}

#[test]
fn deleted_note_then_crash_drops_the_draft_and_trims_the_stack() {
    let mut run = AppRun::start(app());
    run.tap("Note 3");
    run.replace_title("Groceries");
    let before = run
        .storage()
        .contents(&Session::NAME)
        .expect("the draft writes the session");
    run.tap("Delete note");
    assert!(run.shows("Deleted note 3") && !run.shows("Note 3"));

    // The data with note 3 deleted is stored; the session written after it
    // is lost with the process.
    run.storage().put(&Session::NAME, before);
    let run = run.restart();
    assert!(!run.shows("Load failed: "), "no load error");
    assert!(
        run.shows("Note 1") && !run.shows("Note 3 does not exist"),
        "a stack opening a deleted note starts at Home"
    );
    assert!(!run.shows("Note 3"), "the note stays deleted");
}

/// A stored session whose values no longer fit falls back value by value,
/// without a panic or a load error.
#[test]
fn restored_values_that_no_longer_fit_fall_back() {
    struct Case {
        name: &'static str,
        edit: fn(&mut Value),
        check: fn(&mut AppRun),
    }
    let cases = [
        Case {
            name: "an offset past the extent is clamped to its end",
            edit: |session| {
                session["stack"] = json!(["/"]);
                session["offset"] = json!(1.0e9);
            },
            check: |run| {
                assert!(run.shows("Note 40") && !run.shows("Note 1"));
                let clamped = run.list_offset();
                run.shift_tab();
                run.key(NamedKey::End);
                assert_eq!(run.list_offset(), clamped, "the end of the list");
            },
        },
        Case {
            name: "an offset before the start is clamped to it",
            edit: |session| {
                session["stack"] = json!(["/"]);
                session["offset"] = json!(-500.0);
            },
            check: |run| {
                assert!(run.shows("Note 1"));
                assert_eq!(run.list_offset(), 0.0);
            },
        },
        Case {
            name: "a path that names no route starts at Home",
            edit: |session| session["stack"] = json!(["/", "/nowhere"]),
            check: |run| assert!(run.shows("Note 1") && !run.shows("Back")),
        },
        Case {
            name: "a stack that does not start at Home is Home alone",
            edit: |session| session["stack"] = json!(["/settings"]),
            check: |run| assert!(run.shows("Note 1") && !run.shows("Back")),
        },
        Case {
            name: "a draft under another open note is dropped",
            edit: |session| session["stack"] = json!(["/", "/note/5"]),
            check: |run| {
                assert!(run.shows("Editing note 5"));
                assert_eq!(
                    run.title(),
                    "Note 5",
                    "the open note's title, not the draft"
                );
            },
        },
        Case {
            name: "a draft taken against other data is dropped",
            edit: |session| session["data_revision"] = json!(99),
            check: |run| {
                assert!(run.shows("Editing note 2"), "the route still fits");
                assert_eq!(run.title(), "Note 2", "the stored title, not the draft");
            },
        },
    ];

    let mut run = AppRun::start(app());
    run.tap("Note 2");
    run.replace_title("Groceries");
    let stored = run.storage().clone();
    for case in cases {
        let storage = copy_of(&stored);
        edit_session(&storage, case.edit);
        let mut run = AppRun::start_over(app(), storage);
        assert!(!run.shows("Load failed: "), "{}: no load error", case.name);
        (case.check)(&mut run);
    }
}

#[test]
fn unreadable_session_starts_home_at_zero() {
    let mut run = AppRun::start(app());
    run.shift_tab();
    run.key(NamedKey::End);
    run.tap("Note 40");
    run.storage().put(&Session::NAME, "not a session");

    let run = run.restart();
    assert!(!run.shows("Load failed: "), "the data loads");
    assert!(
        run.shows("Note 1") && !run.shows("Back"),
        "Home, at the top"
    );
    assert_eq!(run.list_offset(), 0.0);
    assert_eq!(
        run.storage().kept_aside(&Session::NAME),
        [b"not a session".to_vec()],
        "the unreadable session is kept aside"
    );
}
