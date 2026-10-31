//! `Persisted<D>` through the public surface: a document opened in
//! `init_state`, loaded and written through the UI runtime's storage, with a
//! `MemoryStorage` standing in for the disk.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use flui_objects::RenderSizedBox;
use flui_platform::StorageName;
use flui_platform::{StorageError, UnsupportedReason};
use flui_rendering::protocol::BoxProtocol;
use flui_testing::storage::MemoryStorage;
use flui_testing::widgets::{
    LaidOut, lay_out, lay_out_with_provided_storage, lay_out_with_storage, tight,
};
use flui_view::persist::{
    CodecStep, DecodeError, Document, PersistError, Persisted, ReadOnlyReason, Revision, SaveStatus,
};
use flui_view::prelude::*;
use flui_view::{CloseGuard, CloseReason};

/// The document under test: one line of text.
#[derive(Debug, PartialEq)]
struct Note(String);

impl Document for Note {
    const NAME: StorageName = StorageName::from_static("notes");
    const VERSION: u32 = 2;
    // Small, so a test can store a file over it.
    const MAX_BYTES: u64 = 64;

    fn initial() -> Self {
        let text = NOTE_INITIAL.with(Cell::get);
        if text == "initial panics" {
            fail_the_codec();
        }
        Self(text.to_owned())
    }

    fn encode(&self) -> Vec<u8> {
        if self.0 == "panic" {
            fail_the_codec();
        }
        self.0.clone().into_bytes()
    }

    fn decode(version: u32, body: &[u8]) -> Result<Self, DecodeError> {
        if body == b"panic" {
            fail_the_codec();
        }
        let text = String::from_utf8(body.to_vec())
            .map_err(|error| DecodeError::new(error.to_string()))?;
        // An older format migrates by tagging the body with its version.
        Ok(Self(if version == Self::VERSION {
            text
        } else {
            format!("v{version}:{text}")
        }))
    }
}

thread_local! {
    /// The text of `Note::initial`; a test that needs an initial value whose
    /// encode panics sets it.
    static NOTE_INITIAL: Cell<&'static str> = const { Cell::new("") };

    /// What a dropped `Note` runs: a destructor that reaches back into its
    /// document, set by the test that needs one.
    static ON_NOTE_DROP: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
}

impl Drop for Note {
    fn drop(&mut self) {
        ON_NOTE_DROP.with(|hook| {
            if let Some(hook) = hook.borrow().as_ref() {
                hook();
            }
        });
    }
}

/// A codec that panics, out of line so the codec's return types stay its own.
fn fail_the_codec() {
    panic!("the codec broke");
}
// The document, its codec and its status stay on the owner thread.
static_assertions::assert_not_impl_any!(Persisted<Note>: Send, Sync);

/// A leaf that renders nothing, so the build chain bottoms out.
#[derive(Clone)]
struct Leaf;

impl RenderView for Leaf {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }
}

impl View for Leaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// What `init_state` hands the test: the document and the presentation's close
/// guard.
struct Opened {
    document: Persisted<Note>,
    guard: Option<CloseGuard>,
}

/// Opens the document in `init_state` and hands it to the test.
#[derive(Clone, StatefulView)]
struct NoteOpener {
    opened: Rc<RefCell<Option<Opened>>>,
}

struct NoteOpenerState {
    opened: Rc<RefCell<Option<Opened>>>,
}

impl StatefulView for NoteOpener {
    type State = NoteOpenerState;

    fn create_state(&self) -> Self::State {
        NoteOpenerState {
            opened: Rc::clone(&self.opened),
        }
    }
}

impl ViewState<NoteOpener> for NoteOpenerState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        *self.opened.borrow_mut() = Some(Opened {
            document: Persisted::open(cx),
            guard: cx.close_guard(),
        });
    }

    fn build(&self, _view: &NoteOpener, _cx: &dyn BuildContext) -> impl IntoView {
        Leaf
    }
}

/// Mount a tree over `storage` and take the document it opened: a fresh
/// mount over the same storage is a restart.
fn mount(storage: &MemoryStorage) -> (LaidOut, Persisted<Note>) {
    let (laid, document, _guard) = mount_guarded(storage);
    (laid, document)
}

/// [`mount`], with the presentation's close guard the document holds.
fn mount_guarded(storage: &MemoryStorage) -> (LaidOut, Persisted<Note>, CloseGuard) {
    let opened = Rc::new(RefCell::new(None));
    let laid = lay_out_with_storage(
        NoteOpener {
            opened: Rc::clone(&opened),
        },
        tight(10.0, 10.0),
        storage.clone(),
    );
    let Opened { document, guard } = opened
        .borrow_mut()
        .take()
        .expect("init_state opened the document");
    (
        laid,
        document,
        guard.expect("a hosted presentation has a close guard"),
    )
}

/// Poll `future` between frames until it completes.
fn drive<F: Future>(laid: &mut LaidOut, future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..20 {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        laid.pump();
    }
    panic!("the future did not complete within 20 frames");
}

/// A stored file as an earlier run wrote it.
fn stored(revision: u64, body: &str) -> Vec<u8> {
    format!("flui-document notes 2 {revision}\n{body}").into_bytes()
}

/// A document loads from storage, a set value is written under the next
/// revision, and a restart over the same storage loads that value.
#[test]
fn a_loaded_document_round_trips_through_memory_storage() {
    let storage = MemoryStorage::new();
    storage.put(&Note::NAME, stored(7, "first"));

    let (mut laid, notes) = mount(&storage);
    let loaded = drive(&mut laid, notes.load());
    assert_eq!(
        loaded.as_deref().map(|note| note.0.as_str()),
        Ok("first"),
        "the stored body is decoded"
    );
    assert_eq!(notes.status(), SaveStatus::Clean);

    let revision = notes.set(Note("second".into()));
    assert_eq!(
        revision.as_ref().map(|revision| revision.get()),
        Ok(8),
        "a set value takes the stored revision's successor"
    );
    laid.pump();
    assert_eq!(storage.contents(&Note::NAME), Some(stored(8, "second")));
    assert_eq!(notes.committed(), revision.ok());

    drop((laid, notes));
    let (mut laid, notes) = mount(&storage);
    let reloaded = drive(&mut laid, notes.load());
    assert_eq!(
        reloaded.as_deref().map(|note| note.0.as_str()),
        Ok("second"),
        "a restart loads the written value"
    );
}

/// "Saved" is reported only once the storage commits the write: until then
/// the status is `Saving` and nothing is committed.
#[test]
fn saved_status_appears_only_after_commit() {
    let storage = MemoryStorage::new();
    storage.put(&Note::NAME, stored(1, "first"));
    let (mut laid, notes) = mount(&storage);
    let loaded = drive(&mut laid, notes.load());
    assert!(loaded.is_ok(), "the stored document loads: {loaded:?}");

    let barrier = storage.hold_commits();
    let revision = notes.set(Note("second".into()));
    assert!(
        revision.is_ok(),
        "a loaded document takes a value: {revision:?}"
    );
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Saving, "written, not committed");
    assert!(
        notes.committed() < revision.as_ref().ok().copied(),
        "the new revision is not committed yet"
    );
    assert_eq!(storage.contents(&Note::NAME), Some(stored(1, "first")));

    barrier.commit();
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Clean);
    assert_eq!(notes.committed(), revision.ok());
}

/// Mount a tree with no storage at all, as on a platform without files.
fn mount_without_storage() -> (LaidOut, Persisted<Note>) {
    let opened = Rc::new(RefCell::new(None));
    let laid = lay_out(
        NoteOpener {
            opened: Rc::clone(&opened),
        },
        tight(10.0, 10.0),
    );
    let document = opened
        .borrow_mut()
        .take()
        .expect("init_state opened the document")
        .document;
    (laid, document)
}

/// One stored state a load meets: its name, the stored bytes, the error a
/// read fails with, and the load's outcome.
struct LoadCase(
    &'static str,
    Option<Vec<u8>>,
    Option<StorageError>,
    Result<String, PersistError>,
);

/// What a load makes of each stored state: the value, or the error and the
/// status it leaves. An inaccessible file is its own error, apart from a
/// corrupt one (R18); a file over the limit is refused unread (R30); an
/// older format decodes and a newer one is read-only, with nothing written
/// either way (R31); a decode panic is contained (R29).
#[test]
fn a_load_reports_each_stored_state() {
    let note = |text: &str| Ok(text.to_owned());
    let cases = [
        LoadCase("nothing stored", None, None, note("")),
        LoadCase(
            "a stored document",
            Some(stored(3, "kept")),
            None,
            note("kept"),
        ),
        LoadCase(
            "an older format",
            Some(b"flui-document notes 1 4\nold".to_vec()),
            None,
            note("v1:old"),
        ),
        LoadCase(
            "no header",
            Some(b"kept".to_vec()),
            None,
            Err(PersistError::Corrupt {
                detail: "no header line".into(),
            }),
        ),
        LoadCase(
            "another document's header",
            Some(b"flui-document drafts 2 1\nkept".to_vec()),
            None,
            Err(PersistError::Corrupt {
                detail: "the header names `drafts`, not `notes`".into(),
            }),
        ),
        LoadCase(
            "a newer format",
            Some(b"flui-document notes 3 1\nkept".to_vec()),
            None,
            Err(PersistError::NewerVersion {
                found: 3,
                supported: 2,
            }),
        ),
        LoadCase(
            "an undecodable body",
            Some(stored(1, "\u{0}").into_iter().chain([0xff]).collect()),
            None,
            Err(PersistError::Decode(DecodeError::new(
                "invalid utf-8 sequence of 1 bytes from index 1",
            ))),
        ),
        LoadCase(
            "a decode that panics",
            Some(stored(1, "panic")),
            None,
            Err(PersistError::Panicked {
                during: CodecStep::Decode,
                message: "the codec broke".into(),
            }),
        ),
        LoadCase(
            "an inaccessible file",
            Some(stored(1, "kept")),
            Some(StorageError::Inaccessible {
                kind: std::io::ErrorKind::PermissionDenied,
            }),
            Err(PersistError::Storage(StorageError::Inaccessible {
                kind: std::io::ErrorKind::PermissionDenied,
            })),
        ),
        LoadCase(
            "a file over the limit",
            Some(stored(1, &"x".repeat(64))),
            None,
            Err(PersistError::Storage(StorageError::TooLarge {
                len: 88,
                limit: 64,
            })),
        ),
    ];
    for LoadCase(case, bytes, read_error, expected) in cases {
        let storage = MemoryStorage::new();
        if let Some(bytes) = &bytes {
            storage.put(&Note::NAME, bytes.clone());
        }
        storage.fail_reads(read_error);
        let (mut laid, notes) = mount(&storage);
        let loaded = drive(&mut laid, notes.load());
        assert_eq!(
            loaded.map(|note| note.0.clone()),
            expected,
            "{case}: the load's outcome"
        );
        let status = match &expected {
            Ok(_) => SaveStatus::Clean,
            Err(PersistError::NewerVersion { found, supported }) => {
                SaveStatus::ReadOnly(ReadOnlyReason::NewerVersion {
                    found: *found,
                    supported: *supported,
                })
            }
            Err(error) => SaveStatus::NotLoaded(Some(error.clone())),
        };
        assert_eq!(notes.status(), status, "{case}: the status");
        assert_eq!(
            storage.contents(&Note::NAME),
            bytes,
            "{case}: a load writes nothing"
        );
    }
}

/// Of several loads in flight, only the latest is applied, whichever
/// finishes last (R25, R28).
#[test]
fn only_the_latest_of_several_loads_is_applied() {
    for (case, older, newer, applied) in [
        (
            "an older failure finishing last",
            b"kept".to_vec(),
            stored(1, "fixed"),
            SaveStatus::Clean,
        ),
        (
            "an older success finishing last",
            stored(1, "first"),
            b"broken".to_vec(),
            SaveStatus::NotLoaded(Some(PersistError::Corrupt {
                detail: "no header line".into(),
            })),
        ),
    ] {
        let storage = MemoryStorage::new();
        storage.put(&Note::NAME, older);
        let (mut laid, notes) = mount(&storage);
        let first = notes.load();
        storage.put(&Note::NAME, newer);
        let second = notes.load();
        drop(drive(&mut laid, second));
        drop(drive(&mut laid, first));
        assert_eq!(notes.status(), applied, "{case}");
    }
}

/// A load dropped before it finishes releases its storage request and
/// applies nothing, the tree gone or not (R27).
#[test]
fn unmount_during_load_releases_requests() {
    let storage = MemoryStorage::new();
    storage.put(&Note::NAME, stored(1, "kept"));
    let (laid, notes) = mount(&storage);
    let load = notes.load();
    assert_eq!(storage.live_requests(), 1, "the load reads");
    drop(laid);
    drop(load);
    assert_eq!(
        storage.live_requests(),
        0,
        "a dropped load releases its read"
    );
    assert_eq!(notes.status(), SaveStatus::NotLoaded(None));
}

/// Without storage the document lives in memory: its status says so and a
/// load is refused as not registered, without a panic (R33).
#[test]
fn unavailable_storage_runs_in_memory_with_status() {
    let (mut laid, notes) = mount_without_storage();
    assert_eq!(notes.status(), SaveStatus::Unavailable);
    let loaded = drive(&mut laid, notes.load());
    match loaded {
        Err(PersistError::Storage(StorageError::Unsupported(refusal))) => {
            assert_eq!(refusal.reason, UnsupportedReason::NotRegistered);
        }
        other => panic!("expected a refusal as not registered, got {other:?}"),
    }
    assert_eq!(notes.status(), SaveStatus::Unavailable, "the status stays");
}

/// A load that replaces or clears the loaded value drops it with no borrow
/// of the document held, so a destructor that reads the document finds it
/// settled.
#[test]
fn a_value_a_load_replaces_is_dropped_after_the_document_settles() {
    let storage = MemoryStorage::new();
    storage.put(&Note::NAME, stored(1, "one"));
    let (mut laid, notes) = mount(&storage);
    drop(drive(&mut laid, notes.load()));

    let seen = Rc::new(RefCell::new(Vec::new()));
    let (reader, record) = (notes.clone(), Rc::clone(&seen));
    ON_NOTE_DROP.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || record.borrow_mut().push(reader.status())));
    });
    storage.put(&Note::NAME, stored(2, "two"));
    drop(drive(&mut laid, notes.load()));
    storage.put(&Note::NAME, b"no header".to_vec());
    let failed = drive(&mut laid, notes.load());
    ON_NOTE_DROP.with(|hook| hook.borrow_mut().take());

    assert!(failed.is_err());
    assert_eq!(
        seen.borrow()[..2],
        [
            SaveStatus::Clean,
            SaveStatus::NotLoaded(Some(PersistError::Corrupt {
                detail: "no header line".into(),
            })),
        ],
        "each replaced value saw the status its load left"
    );
}

/// Mount over `storage` holding `body` at revision 1 and load it.
fn loaded(storage: &MemoryStorage, body: &str) -> (LaidOut, Persisted<Note>, CloseGuard) {
    storage.put(&Note::NAME, stored(1, body));
    let (mut laid, notes, guard) = mount_guarded(storage);
    let loaded = drive(&mut laid, notes.load());
    assert!(loaded.is_ok(), "the stored document loads: {loaded:?}");
    (laid, notes, guard)
}

/// A set has nothing to write over unless a load produced a writable value:
/// before a load, after a failed one, over a newer format, at the last
/// revision and without storage it is refused, the status stays and nothing
/// is written (R24, R31).
#[test]
fn no_write_without_a_writable_base() {
    let cases: [(&str, Option<Vec<u8>>, bool, PersistError); 5] = [
        (
            "not loaded",
            Some(stored(1, "kept")),
            false,
            PersistError::NotLoaded,
        ),
        (
            "a failed load",
            Some(b"kept".to_vec()),
            true,
            PersistError::NotLoaded,
        ),
        (
            "a newer format",
            Some(b"flui-document notes 3 1\nkept".to_vec()),
            true,
            PersistError::NewerVersion {
                found: 3,
                supported: 2,
            },
        ),
        (
            "the last revision",
            Some(stored(u64::MAX, "kept")),
            true,
            PersistError::Exhausted,
        ),
        (
            "nothing stored, not loaded",
            None,
            false,
            PersistError::NotLoaded,
        ),
    ];
    for (case, bytes, load, expected) in cases {
        let storage = MemoryStorage::new();
        if let Some(bytes) = &bytes {
            storage.put(&Note::NAME, bytes.clone());
        }
        let (mut laid, notes) = mount(&storage);
        if load {
            drop(drive(&mut laid, notes.load()));
        }
        let before = notes.status();
        assert_eq!(notes.set(Note("new".into())), Err(expected), "{case}");
        laid.pump();
        assert_eq!(notes.status(), before, "{case}: the status stays");
        assert_eq!(
            storage.contents(&Note::NAME),
            bytes,
            "{case}: nothing written"
        );
        assert_eq!(notes.committed(), None, "{case}");
    }

    let (mut laid, notes) = mount_without_storage();
    match notes.set(Note("new".into())) {
        Err(PersistError::Storage(StorageError::Unsupported(refusal))) => {
            assert_eq!(refusal.reason, UnsupportedReason::NotRegistered);
        }
        other => panic!("without storage a set is refused, got {other:?}"),
    }
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Unavailable);
}

/// A full disk fails the write: the status says so, the stored file keeps
/// the old value, and a load is refused over the unsaved edit (R30); a retry
/// then writes the latest value (R20).
#[test]
fn retry_after_failure_publishes_the_latest_value() {
    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    storage.fail_next(StorageError::Full);
    let second = notes.set(Note("second".into()));
    laid.pump();
    let full = PersistError::Storage(StorageError::Full);
    assert_eq!(notes.status(), SaveStatus::Failed(full.clone()));
    assert_eq!(storage.contents(&Note::NAME), Some(stored(1, "first")));
    assert_eq!(notes.committed(), None);
    assert_eq!(
        drive(&mut laid, notes.load()).map(|_| ()),
        Err(PersistError::UnsavedChanges),
        "a load would drop the edit"
    );

    let third = notes.set(Note("third".into()));
    storage.fail_next(StorageError::Full);
    laid.pump();
    assert_eq!(
        notes.status(),
        SaveStatus::Failed(full),
        "the later edit failed too"
    );
    assert_eq!(second.map(Revision::get), Ok(2));
    assert_eq!(third.as_ref().map(|revision| revision.get()), Ok(3));

    notes.retry();
    assert_eq!(notes.status(), SaveStatus::Saving);
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Clean);
    assert_eq!(storage.contents(&Note::NAME), Some(stored(3, "third")));
    assert_eq!(notes.committed(), third.ok());
}

/// Edits made while a write is in flight replace each other: only the
/// latest is written after it, under its own revision (R28).
#[test]
fn a_later_edit_supersedes_an_unstarted_one() {
    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    let barrier = storage.hold_commits();
    let revisions: Vec<_> = ["a", "b", "c"]
        .into_iter()
        .map(|text| notes.set(Note(text.into())))
        .collect::<Result<Vec<_>, _>>()
        .expect("a loaded document takes every value");
    assert_eq!(
        revisions
            .iter()
            .map(|revision| revision.get())
            .collect::<Vec<_>>(),
        [2, 3, 4]
    );
    laid.pump();
    barrier.commit();
    for _ in 0..3 {
        laid.pump();
    }
    assert_eq!(storage.contents(&Note::NAME), Some(stored(4, "c")));
    assert_eq!(notes.committed(), revisions.last().copied());
    assert_eq!(notes.status(), SaveStatus::Clean);
}

/// An encode panic publishes nothing and fails that save; the next set
/// works. A failure is kept until a write succeeds, the first of several
/// winning (R29).
#[test]
fn codec_panic_is_contained_first_failure_kept() {
    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    let panicked = PersistError::Panicked {
        during: CodecStep::Encode,
        message: "the codec broke".into(),
    };
    assert_eq!(notes.set(Note("panic".into())), Err(panicked.clone()));
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Failed(panicked.clone()));
    assert_eq!(storage.contents(&Note::NAME), Some(stored(1, "first")));

    let second = notes.set(Note("second".into()));
    assert_eq!(second.map(Revision::get), Ok(2));
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Clean, "a write succeeded");

    storage.fail_next(StorageError::Full);
    assert!(notes.set(Note("third".into())).is_ok());
    laid.pump();
    let full = PersistError::Storage(StorageError::Full);
    assert_eq!(notes.status(), SaveStatus::Failed(full.clone()));
    assert_eq!(notes.set(Note("panic".into())), Err(panicked));
    assert_eq!(
        notes.status(),
        SaveStatus::Failed(full),
        "the first failure is kept"
    );
    assert_eq!(storage.contents(&Note::NAME), Some(stored(2, "second")));
}

/// A write in flight holds the user's and the program's close and lets a
/// session end through; the held close is carried out once the write is
/// stored (R12, R14). A failed write requires a decision, even of a session
/// end.
#[test]
fn a_saving_document_holds_user_close_only() {
    let storage = MemoryStorage::new();
    let (mut laid, notes, guard) = loaded(&storage, "first");
    let barrier = storage.hold_commits();
    assert!(notes.set(Note("second".into())).is_ok());
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Saving);
    assert!(!laid.request_close(CloseReason::User), "the user waits");
    assert!(!laid.request_close(CloseReason::Program));
    assert_eq!(
        guard.pending().map(|pending| pending.reason()),
        Some(CloseReason::User)
    );

    barrier.commit();
    // No status read in between: the document follows the write by itself.
    laid.pump();
    laid.pump();
    assert_eq!(storage.contents(&Note::NAME), Some(stored(2, "second")));
    assert!(
        guard.pending().is_none(),
        "the held closes were carried out"
    );
    assert_eq!(notes.status(), SaveStatus::Clean);

    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    let _barrier = storage.hold_commits();
    assert!(notes.set(Note("second".into())).is_ok());
    laid.pump();
    assert!(
        laid.request_close(CloseReason::SessionEnd),
        "a session end is not held by a write in flight"
    );

    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    storage.fail_next(StorageError::Full);
    assert!(notes.set(Note("second".into())).is_ok());
    laid.pump();
    assert!(matches!(notes.status(), SaveStatus::Failed(_)));
    assert!(
        !laid.request_close(CloseReason::SessionEnd),
        "a failed write requires a decision"
    );
}

/// Two UI runtimes keep their own documents of one name, each in its own
/// storage (R36).
#[test]
fn two_runtimes_keep_independent_documents() {
    let (left, right) = (MemoryStorage::new(), MemoryStorage::new());
    let (mut left_laid, left_notes, _left_guard) = loaded(&left, "left");
    let (mut right_laid, right_notes, _right_guard) = loaded(&right, "right");
    assert!(left_notes.set(Note("left 2".into())).is_ok());
    left_laid.pump();
    right_laid.pump();
    assert_eq!(left.contents(&Note::NAME), Some(stored(2, "left 2")));
    assert_eq!(right.contents(&Note::NAME), Some(stored(1, "right")));
    assert_eq!(right_notes.status(), SaveStatus::Clean);
    assert_eq!(right_notes.committed(), None);
}

/// An edit the replaced value's destructor makes is published after the
/// edit that replaced it, so it is the one stored and loaded.
#[test]
fn an_edit_from_a_replaced_values_drop_wins() {
    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    let (editor, nested) = (notes.clone(), Rc::new(RefCell::new(None)));
    let (record, fired) = (Rc::clone(&nested), std::cell::Cell::new(false));
    ON_NOTE_DROP.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            // Once: the nested edit replaces a value too.
            if !fired.replace(true) {
                let set = editor.set(Note("nested".into()));
                *record.borrow_mut() = Some(set);
            }
        }));
    });
    let outer = notes.set(Note("outer".into()));
    ON_NOTE_DROP.with(|hook| hook.borrow_mut().take());
    laid.pump();
    laid.pump();

    assert_eq!(outer.map(Revision::get), Ok(2));
    assert_eq!(
        nested.borrow_mut().take().map(|set| set.map(Revision::get)),
        Some(Ok(3))
    );
    assert_eq!(notes.status(), SaveStatus::Clean);
    assert_eq!(storage.contents(&Note::NAME), Some(stored(3, "nested")));
    assert_eq!(notes.committed().map(Revision::get), Some(3));
}

/// A load that read the file before an edit is not applied over it: the
/// edit stays the value, and the next edit takes the next revision.
#[test]
fn a_load_in_flight_is_not_applied_over_a_later_edit() {
    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    let stale = notes.load();
    assert_eq!(notes.set(Note("second".into())).map(Revision::get), Ok(2));
    let read = drive(&mut laid, stale);
    assert_eq!(read.as_deref(), Ok(&Note("first".into())), "returned");
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Clean);
    assert_eq!(notes.set(Note("third".into())).map(Revision::get), Ok(3));
    laid.pump();
    assert_eq!(storage.contents(&Note::NAME), Some(stored(3, "third")));
}

/// A reload that fails after a save reports the load's failure, not the
/// earlier save's `Clean`.
#[test]
fn a_failed_reload_after_a_save_reports_the_load() {
    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    assert!(notes.set(Note("second".into())).is_ok());
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Clean);
    storage.put(&Note::NAME, b"no header".to_vec());
    assert!(drive(&mut laid, notes.load()).is_err());
    assert_eq!(
        notes.status(),
        SaveStatus::NotLoaded(Some(PersistError::Corrupt {
            detail: "no header line".into(),
        }))
    );
}

/// A document whose application provides its own storage reads and writes
/// that storage: the host's own storage receives nothing.
#[test]
fn a_provided_storage_receives_the_documents_writes() {
    let (host, provided) = (MemoryStorage::new(), MemoryStorage::new());
    provided.put(&Note::NAME, stored(1, "first"));
    let opened = Rc::new(RefCell::new(None));
    let mut laid = lay_out_with_provided_storage(
        NoteOpener {
            opened: Rc::clone(&opened),
        },
        tight(10.0, 10.0),
        host.clone(),
        provided.clone(),
    );
    let Opened {
        document: notes, ..
    } = opened
        .borrow_mut()
        .take()
        .expect("init_state opened the document");

    let loaded = drive(&mut laid, notes.load());
    assert_eq!(loaded.as_deref(), Ok(&Note("first".into())));
    assert!(notes.set(Note("second".into())).is_ok());
    laid.pump();

    assert_eq!(notes.status(), SaveStatus::Clean);
    assert_eq!(provided.contents(&Note::NAME), Some(stored(2, "second")));
    assert_eq!(
        host.contents(&Note::NAME),
        None,
        "the host's storage is untouched"
    );
}

/// A busy store is tried again on the UI runtime's clock: 50 ms after the
/// first refusal, not 49, the status `Saving` meanwhile; after the 800 ms
/// retry a further refusal fails the write (R16).
#[test]
fn busy_retry_follows_the_runtime_clock() {
    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    storage.fail_next(StorageError::Busy);
    assert!(notes.set(Note("second".into())).is_ok());
    laid.pump();
    assert_eq!(
        notes.status(),
        SaveStatus::Saving,
        "a busy store is retried"
    );
    laid.pump_for(Duration::from_millis(49));
    assert_eq!(storage.contents(&Note::NAME), Some(stored(1, "first")));
    laid.pump_for(Duration::from_millis(1));
    laid.pump();
    assert_eq!(storage.contents(&Note::NAME), Some(stored(2, "second")));
    assert_eq!(notes.status(), SaveStatus::Clean);

    for _ in 0..6 {
        storage.fail_next(StorageError::Busy);
    }
    assert!(notes.set(Note("third".into())).is_ok());
    laid.pump();
    for (backoff, status) in [
        (50, SaveStatus::Saving),
        (100, SaveStatus::Saving),
        (200, SaveStatus::Saving),
        (400, SaveStatus::Saving),
        (
            800,
            SaveStatus::Failed(PersistError::Storage(StorageError::Busy)),
        ),
    ] {
        laid.pump_for(Duration::from_millis(backoff));
        laid.pump();
        assert_eq!(notes.status(), status, "after the {backoff} ms retry");
    }
    assert_eq!(storage.contents(&Note::NAME), Some(stored(2, "second")));
}

/// An edit made while an older edit waits out a long back-off starts its own
/// retries: refused as busy, it is tried again 50 ms later, not when the
/// older wait ends.
#[test]
fn a_later_edit_refused_as_busy_starts_its_own_back_off() {
    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    for _ in 0..3 {
        storage.fail_next(StorageError::Busy);
    }
    assert!(notes.set(Note("second".into())).is_ok());
    laid.pump();
    for backoff in [50, 100] {
        laid.pump_for(Duration::from_millis(backoff));
        laid.pump();
    }
    assert_eq!(notes.status(), SaveStatus::Saving, "waiting 200 ms");

    storage.fail_next(StorageError::Busy);
    assert!(notes.set(Note("third".into())).is_ok());
    laid.pump();
    laid.pump_for(Duration::from_millis(49));
    assert_eq!(storage.contents(&Note::NAME), Some(stored(1, "first")));
    laid.pump_for(Duration::from_millis(1));
    laid.pump();
    assert_eq!(
        storage.contents(&Note::NAME),
        Some(stored(3, "third")),
        "retried after its own first back-off"
    );
    assert_eq!(notes.status(), SaveStatus::Clean);
}

/// Hiding the application while nothing is retried hurries nothing later: a
/// write refused as busy after the application is shown again waits its
/// first back-off.
#[test]
fn a_hide_while_clean_does_not_hurry_a_later_busy_retry() {
    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    laid.set_window_visible(false);
    laid.pump();
    laid.set_window_visible(true);
    laid.pump();

    storage.fail_next(StorageError::Busy);
    assert!(notes.set(Note("second".into())).is_ok());
    laid.pump();
    laid.pump();
    assert_eq!(
        storage.contents(&Note::NAME),
        Some(stored(1, "first")),
        "the earlier hide is not carried into this write"
    );
    laid.pump_for(Duration::from_millis(50));
    laid.pump();
    assert_eq!(storage.contents(&Note::NAME), Some(stored(2, "second")));
}

/// A case of a retry hurried: its name and what hurries it.
struct HurryCase(&'static str, fn(&LaidOut));

/// A retry waiting out its back-off runs at once when the application is
/// hidden or a close is asked for, not when the back-off ends.
#[test]
fn a_hidden_application_or_a_close_retries_a_busy_write_at_once() {
    let cases: [HurryCase; 2] = [
        HurryCase("hidden", |laid| laid.set_window_visible(false)),
        HurryCase("closed", |laid| {
            assert!(!laid.request_close(CloseReason::User), "the write holds it");
        }),
    ];
    for HurryCase(case, hurry) in cases {
        let storage = MemoryStorage::new();
        let (mut laid, notes, _guard) = loaded(&storage, "first");
        storage.fail_next(StorageError::Busy);
        assert!(notes.set(Note("second".into())).is_ok());
        laid.pump();
        hurry(&laid);
        laid.pump();
        laid.pump();
        assert_eq!(
            storage.contents(&Note::NAME),
            Some(stored(2, "second")),
            "{case}: written before the 50 ms back-off"
        );
    }
}

/// A stored file the document cannot read is never written over until the
/// application starts over, and starting over keeps its bytes first (R17):
/// a file without a header, a body `decode` refuses and a decode that panics
/// are each kept under a free name and replaced by the initial value; a
/// reload that finds the file corrupted after a save reports it rather than
/// the earlier save; a failed copy leaves the file and resumes on retry.
#[test]
fn corrupt_bytes_are_kept_until_start_over() {
    let unreadable: [(&str, Vec<u8>); 3] = [
        ("no header", b"not a document".to_vec()),
        (
            "refused body",
            stored(1, "\u{fffd}").into_iter().chain([0xff]).collect(),
        ),
        ("decode panic", stored(1, "panic")),
    ];
    for (case, bytes) in unreadable {
        let storage = MemoryStorage::new();
        storage.put(&Note::NAME, bytes.clone());
        let (mut laid, notes, _guard) = mount_guarded(&storage);
        let refused = drive(&mut laid, notes.load()).map(|_| ());
        assert!(refused.is_err(), "{case}: the load fails");
        assert_eq!(
            notes.set(Note("edit".into())),
            Err(PersistError::NotLoaded),
            "{case}"
        );
        laid.pump();
        assert_eq!(
            storage.contents(&Note::NAME),
            Some(bytes.clone()),
            "{case}: untouched"
        );

        notes.start_over();
        assert_eq!(notes.status(), SaveStatus::Saving, "{case}");
        laid.pump();
        laid.pump();
        assert_eq!(notes.status(), SaveStatus::Clean, "{case}");
        assert_eq!(storage.kept_aside(&Note::NAME), vec![bytes], "{case}: kept");
        assert_eq!(storage.contents(&Note::NAME), Some(stored(1, "")), "{case}");
        assert_eq!(notes.committed().map(Revision::get), Some(1), "{case}");
        assert_eq!(
            notes.set(Note("fresh".into())).map(Revision::get),
            Ok(2),
            "{case}: the fresh value is loaded"
        );
        laid.pump();
        assert_eq!(
            storage.contents(&Note::NAME),
            Some(stored(2, "fresh")),
            "{case}"
        );
    }

    // A file corrupted after a save: the reload's failure, not the save's
    // success, is the status, and starting over keeps that file.
    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    assert!(notes.set(Note("second".into())).is_ok());
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Clean);
    storage.put(&Note::NAME, b"overwritten".to_vec());
    assert!(drive(&mut laid, notes.load()).is_err());
    assert!(
        matches!(
            notes.status(),
            SaveStatus::NotLoaded(Some(PersistError::Corrupt { .. }))
        ),
        "{:?}",
        notes.status()
    );
    notes.start_over();
    laid.pump();
    laid.pump();
    assert_eq!(
        storage.kept_aside(&Note::NAME),
        vec![b"overwritten".to_vec()]
    );

    // A copy that fails writes nothing; a retry makes it.
    let storage = MemoryStorage::new();
    storage.put(&Note::NAME, b"not a document".to_vec());
    let (mut laid, notes, _guard) = mount_guarded(&storage);
    assert!(drive(&mut laid, notes.load()).is_err());
    storage.fail_copies(Some(StorageError::Full));
    notes.start_over();
    laid.pump();
    assert_eq!(
        notes.status(),
        SaveStatus::Failed(PersistError::Storage(StorageError::Full))
    );
    assert_eq!(
        storage.contents(&Note::NAME),
        Some(b"not a document".to_vec())
    );
    storage.fail_copies(None);
    notes.retry();
    laid.pump();
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Clean);
    assert_eq!(storage.kept_aside(&Note::NAME).len(), 1);
    assert_eq!(storage.contents(&Note::NAME), Some(stored(1, "")));

    // A readable document never starts over.
    let storage = MemoryStorage::new();
    let (mut laid, notes, _guard) = loaded(&storage, "first");
    notes.start_over();
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Clean);
    assert_eq!(storage.kept_aside(&Note::NAME), Vec::<Vec<u8>>::new());
    assert_eq!(storage.contents(&Note::NAME), Some(stored(1, "first")));
}

/// A fresh start writes over the file the failed load read and no other: a
/// file another writer stored since, here one in a newer format, is kept
/// and the write is a conflict.
#[test]
fn start_over_never_writes_over_a_file_replaced_since_the_load() {
    let storage = MemoryStorage::new();
    storage.put(&Note::NAME, b"not a document".to_vec());
    let (mut laid, notes, _guard) = mount_guarded(&storage);
    assert!(drive(&mut laid, notes.load()).is_err(), "the load fails");
    let newer = b"flui-document notes 3 1\nfrom a newer build".to_vec();
    storage.put(&Note::NAME, newer.clone());

    notes.start_over();
    laid.pump();
    laid.pump();
    assert_eq!(
        notes.status(),
        SaveStatus::Failed(PersistError::Storage(StorageError::Conflict))
    );
    assert_eq!(storage.contents(&Note::NAME), Some(newer));
}

/// An initial value whose encode panics fails the fresh start even when its
/// destructor panics too: the first failure is recorded before the
/// destructor runs, so the start is not left `Saving` with nothing to finish
/// it.
#[test]
fn an_initial_value_failing_encode_and_drop_fails_the_fresh_start() {
    let storage = MemoryStorage::new();
    storage.put(&Note::NAME, b"not a document".to_vec());
    let (mut laid, notes, _guard) = mount_guarded(&storage);
    assert!(drive(&mut laid, notes.load()).is_err(), "the load fails");

    NOTE_INITIAL.with(|initial| initial.set("panic"));
    let fired = Rc::new(Cell::new(false));
    let once = Rc::clone(&fired);
    ON_NOTE_DROP.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            assert!(once.replace(true), "the destructor broke");
        }));
    });
    notes.start_over();
    let pumped = catch_unwind(AssertUnwindSafe(|| {
        laid.pump();
        laid.pump();
    }));
    ON_NOTE_DROP.with(|hook| hook.borrow_mut().take());
    NOTE_INITIAL.with(|initial| initial.set(""));
    assert!(fired.get(), "the destructor ran");
    drop(pumped);

    assert!(
        matches!(
            notes.status(),
            SaveStatus::Failed(PersistError::Panicked {
                during: CodecStep::Encode,
                ..
            })
        ),
        "{:?}",
        notes.status()
    );
    notes.retry();
    laid.pump();
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Clean, "a retry finishes it");
    assert_eq!(storage.contents(&Note::NAME), Some(stored(1, "")));
}

/// A `Document::initial` that panics fails the fresh start instead of
/// leaving it `Saving` with nothing to finish it; a retry once it works
/// finishes the start.
#[test]
fn an_initial_value_that_panics_fails_the_fresh_start() {
    let storage = MemoryStorage::new();
    storage.put(&Note::NAME, b"not a document".to_vec());
    let (mut laid, notes, _guard) = mount_guarded(&storage);
    assert!(drive(&mut laid, notes.load()).is_err(), "the load fails");

    NOTE_INITIAL.with(|initial| initial.set("initial panics"));
    notes.start_over();
    laid.pump();
    laid.pump();
    NOTE_INITIAL.with(|initial| initial.set(""));
    assert!(
        matches!(
            notes.status(),
            SaveStatus::Failed(PersistError::Panicked {
                during: CodecStep::Initial,
                ..
            })
        ),
        "{:?}",
        notes.status()
    );
    notes.retry();
    laid.pump();
    laid.pump();
    assert_eq!(notes.status(), SaveStatus::Clean, "a retry finishes it");
    assert_eq!(storage.contents(&Note::NAME), Some(stored(1, "")));
}
