//! `Persisted<D>` through the public surface: a document opened in
//! `init_state`, loaded and written through the UI runtime's storage, with a
//! `MemoryStorage` standing in for the disk.

use std::cell::RefCell;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use flui_objects::RenderSizedBox;
use flui_platform_api::StorageName;
use flui_rendering::protocol::BoxProtocol;
use flui_testing::storage::MemoryStorage;
use flui_testing::widgets::{LaidOut, lay_out_with_storage, tight};
use flui_view::persist::{DecodeError, Document, Persisted, SaveStatus};
use flui_view::prelude::*;

/// The document under test: one line of text.
#[derive(Debug, PartialEq)]
struct Note(String);

impl Document for Note {
    const NAME: StorageName = StorageName::from_static("notes");
    const VERSION: u32 = 1;

    fn initial() -> Self {
        Self(String::new())
    }

    fn encode(&self) -> Vec<u8> {
        self.0.clone().into_bytes()
    }

    fn decode(_version: u32, body: &[u8]) -> Result<Self, DecodeError> {
        String::from_utf8(body.to_vec())
            .map(Self)
            .map_err(|error| DecodeError::new(error.to_string()))
    }
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
    format!("flui-document notes 1 {revision}\n{body}").into_bytes()
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
