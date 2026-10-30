//! `Persisted<D>` through the public surface: a document opened in
//! `init_state`, loaded and written through the UI runtime's storage, with a
//! `MemoryStorage` standing in for the disk.

use std::cell::RefCell;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use flui_objects::RenderSizedBox;
use flui_platform::StorageName;
use flui_platform::{StorageError, UnsupportedReason};
use flui_rendering::protocol::BoxProtocol;
use flui_testing::storage::MemoryStorage;
use flui_testing::widgets::{LaidOut, lay_out, lay_out_with_storage, tight};
use flui_view::persist::{
    CodecStep, DecodeError, Document, PersistError, Persisted, ReadOnlyReason, SaveStatus,
};
use flui_view::prelude::*;

/// The document under test: one line of text.
#[derive(Debug, PartialEq)]
struct Note(String);

impl Document for Note {
    const NAME: StorageName = StorageName::from_static("notes");
    const VERSION: u32 = 2;
    // Small, so a test can store a file over it.
    const MAX_BYTES: u64 = 64;

    fn initial() -> Self {
        Self(String::new())
    }

    fn encode(&self) -> Vec<u8> {
        self.0.clone().into_bytes()
    }

    fn decode(version: u32, body: &[u8]) -> Result<Self, DecodeError> {
        if body == b"panic" {
            fail_the_decode();
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

/// A decode that panics, out of line so the codec's return type stays its own.
fn fail_the_decode() {
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

/// Opens the document in `init_state` and hands it to the test.
#[derive(Clone, StatefulView)]
struct NoteOpener {
    opened: Rc<RefCell<Option<Persisted<Note>>>>,
}

struct NoteOpenerState {
    opened: Rc<RefCell<Option<Persisted<Note>>>>,
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
        *self.opened.borrow_mut() = Some(Persisted::open(cx));
    }

    fn build(&self, _view: &NoteOpener, _cx: &dyn BuildContext) -> impl IntoView {
        Leaf
    }
}

/// Mount a tree over `storage` and take the document it opened: a fresh
/// mount over the same storage is a restart.
fn mount(storage: &MemoryStorage) -> (LaidOut, Persisted<Note>) {
    let opened = Rc::new(RefCell::new(None));
    let laid = lay_out_with_storage(
        NoteOpener {
            opened: Rc::clone(&opened),
        },
        tight(10.0, 10.0),
        storage.clone(),
    );
    let document = opened
        .borrow_mut()
        .take()
        .expect("init_state opened the document");
    (laid, document)
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
#[ignore = "contract: Persisted loads and writes through the ui_runtime's storage"]
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
#[ignore = "contract: Persisted reports a write as saved only after the storage commits it"]
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
        .expect("init_state opened the document");
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
