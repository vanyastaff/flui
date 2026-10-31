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
    /// The attempt numbers the loader was called with, in order.
    attempts: RefCell<Vec<u32>>,
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
    let loader: Loader = Rc::new(move |attempt| {
        held.attempts.borrow_mut().push(attempt);
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
    assert!(!run.shows_starting("Load failed: "), "no load error");
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
        assert!(
            !run.shows_starting("Load failed: "),
            "{}: no load error",
            case.name
        );
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
    assert!(!run.shows_starting("Load failed: "), "the data loads");
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

#[test]
fn a_list_scrolled_and_left_reopens_at_its_offset() {
    let mut run = AppRun::start(app());
    run.shift_tab();
    run.key(NamedKey::End);
    let offset = run.list_offset();
    assert!(offset > 0.0, "End scrolls the list");

    let run = run.restart();
    let restored = run.list_offset();
    assert!(
        (restored - offset).abs() <= 1.0,
        "a scroll alone writes the session: {restored} for {offset}"
    );
}

#[test]
fn a_place_taken_while_an_unreadable_session_is_kept_aside_is_stored() {
    let run = AppRun::start(app());
    run.storage().put(&Session::NAME, "not a session");
    let copying = run.storage().hold_copies();

    let mut run = run.restart();
    assert!(run.shows("Note 1"), "the data loads under a slow copy");
    run.tap("Note 3");
    run.replace_title("Groceries");
    drop(copying);
    run.settle();
    assert_eq!(
        run.storage().kept_aside(&Session::NAME),
        [b"not a session".to_vec()],
        "the unreadable session is kept aside"
    );

    let run = run.restart();
    assert!(
        run.shows("Editing note 3"),
        "the place taken during the fresh start is stored"
    );
    assert_eq!(run.title(), "Groceries");
}

/// Each way a load of the notes fails shows Retry; only notes that cannot be
/// read back offer Start over, and nothing is written until the user picks.
#[test]
fn each_load_failure_offers_its_way_on() {
    struct Case {
        name: &'static str,
        stored: &'static str,
        fail_reads: bool,
        offers_start_over: bool,
    }
    let cases = [
        Case {
            name: "corrupt bytes",
            stored: "not notes",
            fail_reads: false,
            offers_start_over: true,
        },
        Case {
            name: "a body the notes cannot decode",
            stored: "flui-document notes 1 1\n\"nope\"",
            fail_reads: false,
            offers_start_over: true,
        },
        Case {
            name: "a newer format",
            stored: "flui-document notes 99 1\n{}",
            fail_reads: false,
            offers_start_over: false,
        },
        Case {
            name: "an inaccessible file",
            stored: "not read",
            fail_reads: true,
            offers_start_over: false,
        },
    ];
    for case in cases {
        let storage = MemoryStorage::new();
        storage.put(&Saved::NAME, case.stored);
        if case.fail_reads {
            storage.fail_reads(Some(StorageError::Inaccessible {
                kind: ErrorKind::PermissionDenied,
            }));
        }
        let run = AppRun::start_over(app(), storage);
        assert!(
            run.shows_starting("Load failed: "),
            "{}: the failure is shown",
            case.name
        );
        assert!(run.shows("Retry"), "{}: with Retry", case.name);
        assert_eq!(
            run.shows("Start over"),
            case.offers_start_over,
            "{}: Start over is offered only for unreadable notes",
            case.name
        );
        assert_eq!(
            run.storage().contents(&Saved::NAME).as_deref(),
            Some(case.stored.as_bytes()),
            "{}: nothing is written",
            case.name
        );
    }
}

#[test]
fn start_over_keeps_the_unreadable_notes_aside_and_stores_fresh_ones() {
    let storage = MemoryStorage::new();
    storage.put(&Saved::NAME, "not notes");
    let mut run = AppRun::start_over(app(), storage);
    run.tap("Start over");
    assert!(
        run.shows("Note 1") && !run.shows_starting("Load failed: "),
        "the initial notes are shown"
    );
    assert_eq!(
        run.storage().kept_aside(&Saved::NAME),
        [b"not notes".to_vec()],
        "the unreadable notes are kept aside"
    );

    run.tap("Note 2");
    run.replace_title("Groceries");
    run.tap("Save note");
    run.tap("Back");
    let run = run.restart();
    assert!(
        run.shows("Groceries") && !run.shows_starting("Load failed: "),
        "the fresh notes are stored and take later saves"
    );
}

#[test]
fn without_storage_the_notes_run_in_memory_with_a_banner() {
    let mut run = AppRun::start_without_storage(app());
    assert!(
        !run.shows_starting("Load failed: "),
        "no storage is not a load failure"
    );
    assert!(run.shows("Note 1"), "the initial notes are shown");
    assert!(
        run.shows("Storage is unavailable: the notes are kept in memory only"),
        "and a banner says they are not stored"
    );
    run.tap("Note 2");
    run.replace_title("Groceries");
    run.tap("Save note");
    run.tap("Back");
    assert!(run.shows("Groceries"), "edits work in memory");
}

#[test]
fn a_failed_save_shows_a_banner_whose_retry_stores_the_notes() {
    let mut run = AppRun::start(app());
    run.tap("Note 2");
    run.replace_title("Groceries");
    run.storage().fail_next(StorageError::Full);
    run.tap("Save note");
    assert!(
        run.shows_starting("Saving failed: "),
        "the failed save is shown"
    );
    assert!(run.storage().contents(&Saved::NAME).is_none());

    run.tap("Retry saving");
    assert!(
        !run.shows_starting("Saving failed: "),
        "the banner goes once it is stored"
    );
    run.tap("Back");
    let run = run.restart();
    assert!(run.shows("Groceries"), "the retried save is read back");
}

#[test]
fn reload_replaces_a_pending_load() {
    let (app, gate) = gated();
    let mut run = AppRun::start(app);
    assert!(run.shows("Loading notes"));
    let waiting = Rc::strong_count(&gate);
    assert_eq!(gate.attempts.borrow().last(), Some(&0));
    run.tap("Reload");
    assert_eq!(
        gate.attempts.borrow().last(),
        Some(&1),
        "Reload starts the next attempt"
    );
    assert!(run.shows("Loading notes"), "the new load waits too");
    assert_eq!(
        Rc::strong_count(&gate),
        waiting,
        "the replaced load's future is dropped, not kept beside the new one"
    );

    gate.set_open(true);
    run.settle();
    assert!(run.shows("Note 1"), "the new load is applied");
    assert_eq!(run.storage().live_requests(), 0, "and no request is left");
}

#[test]
fn a_held_write_keeps_typing_and_scrolling_live() {
    let mut run = AppRun::start(app());
    let held = run.storage().hold_writes();
    run.tap("Note 2");
    run.replace_title("Groceries");
    run.tap("Save note");
    assert!(
        run.storage().contents(&Saved::NAME).is_none(),
        "the save is held"
    );
    run.replace_title("Groceries and milk");
    assert_eq!(run.title(), "Groceries and milk", "typing works");
    run.tap("Back");
    run.shift_tab();
    run.key(NamedKey::End);
    assert!(run.list_offset() > 0.0, "scrolling works");

    drop(held);
    run.settle();
    let stored = run
        .storage()
        .contents(&Saved::NAME)
        .expect("the held save lands once released");
    assert!(
        String::from_utf8_lossy(&stored).contains("\"Groceries\""),
        "with the title saved"
    );
}

#[test]
fn start_over_shows_the_notes_once_they_take_edits() {
    let storage = MemoryStorage::new();
    storage.put(&Saved::NAME, "not notes");
    let mut run = AppRun::start_over(app(), storage);
    let held = run.storage().hold_copies();
    run.tap("Start over");
    assert!(
        run.shows("Loading notes") && !run.shows("Note 1"),
        "no editable notes while the old ones are being kept aside"
    );
    drop(held);
    run.settle();
    assert_eq!(
        run.storage().kept_aside(&Saved::NAME),
        [b"not notes".to_vec()],
        "the unreadable notes are kept aside"
    );

    run.tap("Note 2");
    run.replace_title("Groceries");
    run.tap("Save note");
    run.tap("Back");
    let run = run.restart();
    assert!(
        run.shows("Groceries"),
        "the first edit after the fresh start is stored"
    );
}

#[test]
fn a_start_over_whose_copy_fails_offers_retry() {
    let storage = MemoryStorage::new();
    storage.put(&Saved::NAME, "not notes");
    let mut run = AppRun::start_over(app(), storage);
    run.storage().fail_copies(Some(StorageError::Full));
    run.tap("Start over");
    assert!(
        run.shows_starting("Load failed: ") && run.shows("Retry"),
        "the failed copy is shown with Retry"
    );
    assert_eq!(
        run.storage().contents(&Saved::NAME).as_deref(),
        Some(&b"not notes"[..]),
        "the unreadable notes are not written over"
    );

    run.storage().fail_copies(None);
    run.tap("Retry");
    assert!(run.shows("Note 1"), "Retry takes up the fresh start");
    assert_eq!(
        run.storage().kept_aside(&Saved::NAME),
        [b"not notes".to_vec()]
    );
}

#[test]
fn start_over_drops_the_place_taken_in_the_notes_given_up() {
    let mut run = AppRun::start(app());
    run.tap("Note 2");
    run.replace_title("Groceries");
    run.tap("Save note");
    run.tap("Back");
    run.tap("Note 3");
    run.replace_title("Old draft");
    let run = run.restart();
    assert!(run.shows("Editing note 3"), "the draft is restored");

    run.storage().put(&Saved::NAME, "not notes");
    let mut run = run.restart();
    run.tap("Start over");
    assert!(
        !run.shows("Editing note 3") && !run.shows("Back"),
        "the fresh notes open at Home, without the old draft"
    );
    let run = run.restart();
    assert!(
        !run.shows("Editing note 3") && run.shows("Note 3"),
        "nor does a later launch restore it over the fresh notes"
    );
}

/// A close while the notes could not be saved holds the window and offers
/// the user's ways out: Keep open, Retry saving (which then closes), or
/// Close without saving.
#[test]
fn close_with_a_failed_write_requires_a_decision() {
    let failed_save = || {
        let mut run = AppRun::start(app());
        run.tap("Note 2");
        run.replace_title("Groceries");
        run.storage().fail_next(StorageError::Full);
        run.tap("Save note");
        assert!(run.shows_starting("Saving failed: "));
        run.request_close();
        assert!(!run.is_closed(), "the failed save holds the window");
        assert!(
            run.shows("Close without saving") && run.shows("Keep open"),
            "and the decision is shown"
        );
        run
    };

    let mut run = failed_save();
    run.tap("Keep open");
    assert!(!run.is_closed() && !run.shows("Keep open"), "kept open");
    run.request_close();
    assert!(!run.is_closed(), "a later close is held again");
    run.tap("Retry saving");
    assert!(
        run.is_closed(),
        "a save that succeeds carries the close out"
    );
    assert!(
        run.storage()
            .contents(&Saved::NAME)
            .is_some_and(|stored| String::from_utf8_lossy(&stored).contains("\"Groceries\"")),
        "with the notes stored"
    );

    let mut run = failed_save();
    run.tap("Close without saving");
    assert!(run.is_closed(), "the user's choice closes the window");
    assert!(
        run.storage().contents(&Saved::NAME).is_none(),
        "and nothing is written"
    );
}

/// A close during a save waits for it, asks nothing, and closes once the
/// notes are stored.
#[test]
fn close_during_a_save_waits_for_it() {
    let mut run = AppRun::start(app());
    run.tap("Note 2");
    run.replace_title("Groceries");
    let held = run.storage().hold_writes();
    run.tap("Save note");
    run.request_close();
    assert!(!run.is_closed(), "the save in flight holds the window");
    drop(held);
    run.settle();
    assert!(
        run.is_closed(),
        "the close is carried out once it is stored"
    );
    assert!(run.storage().contents(&Saved::NAME).is_some());
}

/// Hiding the window leaves the place stored; showing it again reads
/// nothing back over what is in memory.
#[test]
fn hidden_writes_the_session_and_resume_does_not_reread() {
    let mut run = AppRun::start(app());
    run.tap("Note 2");
    run.replace_title("Groceries");
    run.laid().set_window_visible(false);
    run.settle();
    let stored = run
        .storage()
        .contents(&Session::NAME)
        .expect("the session is stored while hidden");
    assert!(
        String::from_utf8_lossy(&stored).contains("\"Groceries\""),
        "with the draft"
    );

    edit_session(run.storage(), |session| {
        session["draft"]["title"] = json!("Changed elsewhere");
    });
    run.storage().put(&Saved::NAME, "not notes");
    run.laid().set_window_visible(true);
    run.settle();
    assert_eq!(run.title(), "Groceries", "the draft in memory stays");
    assert!(
        !run.shows_starting("Load failed: "),
        "and nothing is read back"
    );
}
