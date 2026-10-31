//! The acceptance application: a small notes app built only on the `flui`
//! facade, the way an external consumer writes one.
//!
//! Three screens under one [`Router`]: Home lists the notes and toggles compact
//! rows, Note edits a title through a [`TextFormField`] and keeps its draft
//! after Back, Settings sits over either. Two [`Persisted`] documents, encoded
//! as JSON, hold what a restart finds. [`Saved`] is the data: the notes and
//! the compact flag, written by saving a title, adding or deleting a note and
//! toggling compact rows. [`Session`] is where the user was: the route stack,
//! the list's offset and the unsaved draft, written whenever one of them
//! changes and restored on the first frame after the data loads. Readiness is
//! a keyed [`FutureBuilder`] above the router that waits for the [`Loader`]
//! gate, then loads the data, then the session, with Loading, an error and
//! Retry.
//!
//! [`Faults`] is the constructor's seam for injected failures, a set of
//! [`Fault`]s: a panic in the Note editor's `build`, in the Save handler, or in
//! the Note screen's `dispose`. [`Planter`] hands it a value to keep in each
//! state, page and listener it makes, which a test turns into a drop sentinel.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::future::poll_fn;
use std::num::ParseIntError;
use std::rc::{Rc, Weak};
use std::str::FromStr;
use std::task::Poll;

use flui::foundation::{
    ChangeNotifier, ConnectionState, Listenable, ListenerId, ValueKey, ViewKey,
};
use flui::geometry::{EdgeInsets, Size};
use flui::interaction::FocusNode;
use flui::material::{ButtonStyle, InputDecoration, TextButton, TextFormField, Theme, ThemeData};
use flui::platform::StorageName;
use flui::prelude::*;
use flui::view::element::ElementKind;
use flui::view::persist::{DecodeError, Document, PersistError, Persisted, Revision, SaveStatus};
use flui::view::{AsyncDriver, CloseGuard, StaleClose, TaskToken, View};
use flui::widgets::{
    AnimatedBuilder, BoxedResultFuture, Form, FormHandle, FutureBuilder, SafeArea, Scrollable,
    WidgetStateProperty, column,
};
use serde::{Deserialize, Serialize};

/// How many notes a fresh start opens with: enough to scroll.
const SEEDED_NOTES: u64 = 40;
const ROW_HEIGHT: f64 = 48.0;
const COMPACT_ROW_HEIGHT: f64 = 32.0;

/// A note's identity. Ids are handed out in increasing order and never
/// reused, so a stale id can only name nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NoteId(pub u64);

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for NoteId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.parse().map(Self)
    }
}

/// The application's locations.
#[derive(Routable, Clone, Debug, PartialEq)]
pub enum Route {
    #[route("/")]
    Home,
    #[route("/note/:id")]
    Note { id: NoteId },
    #[route("/settings")]
    Settings,
}

/// A failure injected into the application, for the rows and native runs that
/// check what a panic leaves behind. Each has one name, which [`Fault::name`]
/// gives and [`FromStr`] reads: the `acceptance_app_faults` example takes it as
/// its argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// `build-panic`: the Note screen's editor panics in its `build`; the
    /// screen's header, with Back, stays.
    Build,
    /// `save-panic`: pressing "Save note" panics before anything is saved.
    Save,
    /// `dispose-panic`: the Note screen's state panics when it is disposed.
    Dispose,
}

impl Fault {
    /// Every fault, in the order their names are listed.
    pub const ALL: [Self; 3] = [Self::Build, Self::Save, Self::Dispose];

    /// The fault's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Build => "build-panic",
            Self::Save => "save-panic",
            Self::Dispose => "dispose-panic",
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A name that is not a [`Fault`]'s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownFault(pub String);

impl fmt::Display for UnknownFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown fault `{}`; the faults are", self.0)?;
        for fault in Fault::ALL {
            write!(f, " `{fault}`")?;
        }
        Ok(())
    }
}

impl std::error::Error for UnknownFault {}

impl FromStr for Fault {
    type Err = UnknownFault;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|fault| fault.name() == name)
            .ok_or_else(|| UnknownFault(name.to_owned()))
    }
}

/// The set of faults the application injects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Faults(u8);

impl Faults {
    /// No fault.
    pub const NONE: Self = Self(0);

    /// This set and `fault`.
    #[must_use]
    pub const fn with(self, fault: Fault) -> Self {
        Self(self.0 | fault.bit())
    }

    /// Whether `fault` is in the set.
    #[must_use]
    pub const fn contains(self, fault: Fault) -> bool {
        self.0 & fault.bit() != 0
    }
}

impl From<Fault> for Faults {
    fn from(fault: Fault) -> Self {
        Self::NONE.with(fault)
    }
}

/// A kind of value the application keeps, for a test that plants a drop
/// sentinel in each one to prove the value was dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    /// A screen's or the root's state.
    State,
    /// A page the router keeps for a route.
    Page,
    /// A listener on the draft's controller.
    Listener,
}

/// Hands the application a value to keep in each value of a [`Kept`] kind it
/// makes. The application only holds what it is given and drops it with the
/// value it is kept in.
pub type Planter = Rc<dyn Fn(Kept) -> Box<dyn Any>>;

/// What `planter` gives for `kind`, when there is a planter.
fn plant(planter: Option<&Planter>, kind: Kept) -> Option<Rc<dyn Any>> {
    planter.map(|planter| Rc::<dyn Any>::from(planter(kind)))
}

/// The readiness gate, called with the attempt number: the stored document
/// is loaded once it succeeds, and its error is the load's.
///
/// The attempt number is a key, not a count: Retry moves it to the next value,
/// wrapping from `u32::MAX` to 0, so it always differs from the last one and
/// always starts a new load. A [`NoteId`] is an identity and never wraps.
pub type Loader = Rc<dyn Fn(u32) -> BoxedResultFuture<(), String>>;

/// The directory the native launchers keep the application's data in.
///
/// A launcher run with `cargo run --example` has no identifier from
/// `flui build`, and without one a desktop host gives widgets no storage:
/// the notes would never load.
pub const DATA_DIR: &str = "dev.flui.acceptance-app";

/// How the native launchers open the application: with its own storage
/// directory, so the notes document loads on every desktop host.
#[must_use]
pub fn launch() -> flui::AppLaunch {
    let dir = flui::platform::AppDataDir::try_new(DATA_DIR)
        .expect("BUG: the acceptance application's data directory name is valid");
    flui::AppLaunch::new().config(flui::AppConfig::new().with_storage_dir(dir))
}

/// The acceptance application's root view.
#[derive(Clone, StatelessView)]
pub struct AcceptanceApp {
    faults: Faults,
    loader: Loader,
    first_attempt: u32,
    planter: Option<Planter>,
}

impl AcceptanceApp {
    /// The application with `faults` injected, a first attempt of 0 and a
    /// loader that succeeds on its second poll, so the first frame shows
    /// Loading.
    #[must_use]
    pub fn new(faults: Faults) -> Self {
        Self {
            faults,
            first_attempt: 0,
            planter: None,
            loader: Rc::new(|_attempt| {
                let mut polled = false;
                Box::pin(poll_fn(move |cx| {
                    if polled {
                        return Poll::Ready(Ok(()));
                    }
                    polled = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }))
            }),
        }
    }

    /// Replace the readiness operation.
    #[must_use]
    pub fn with_loader(mut self, loader: Loader) -> Self {
        self.loader = loader;
        self
    }

    /// Start the attempt number at `attempt` instead of 0.
    #[must_use]
    pub fn first_attempt(mut self, attempt: u32) -> Self {
        self.first_attempt = attempt;
        self
    }

    /// Keep what `planter` gives in each value of a [`Kept`] kind.
    #[must_use]
    pub fn with_planter(mut self, planter: Planter) -> Self {
        self.planter = Some(planter);
        self
    }
}

impl StatelessView for AcceptanceApp {
    fn build(&self, _cx: &dyn BuildContext) -> impl IntoView {
        Root {
            faults: self.faults,
            loader: Rc::clone(&self.loader),
            first_attempt: self.first_attempt,
            planter: self.planter.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Note {
    id: NoteId,
    title: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Notebook {
    notes: Vec<Note>,
    /// The id the next new note takes.
    next_id: NoteId,
}

/// The application's data: what follows the user to another device.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    notebook: Notebook,
    compact: bool,
}

impl Document for Saved {
    const NAME: StorageName = StorageName::from_static("notes");
    const VERSION: u32 = 1;

    fn initial() -> Self {
        Self {
            notebook: Notebook::seeded(),
            compact: false,
        }
    }

    fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("BUG: the notes serialize to JSON")
    }

    fn decode(_version: u32, body: &[u8]) -> Result<Self, DecodeError> {
        serde_json::from_slice::<Self>(body).map_err(|error| DecodeError::new(error.to_string()))
    }
}

impl Notebook {
    fn seeded() -> Self {
        Self {
            notes: (1..=SEEDED_NOTES)
                .map(|id| Note {
                    id: NoteId(id),
                    title: format!("Note {id}"),
                })
                .collect(),
            next_id: NoteId(SEEDED_NOTES + 1),
        }
    }

    fn title(&self, id: NoteId) -> Option<&str> {
        self.notes
            .iter()
            .find(|note| note.id == id)
            .map(|note| note.title.as_str())
    }

    fn add(&mut self) -> NoteId {
        let id = self.next_id;
        self.next_id = NoteId(
            id.0.checked_add(1)
                .expect("BUG: fewer than u64::MAX notes are ever created"),
        );
        self.notes.push(Note {
            id,
            title: format!("Note {id}"),
        });
        id
    }
}

/// Where the user was when the application last ran: what a restart restores
/// over the data. It is machine-local, so it stays on this device when the
/// data follows the user to another one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// The router's stack, bottom to top, as the routes' paths.
    stack: Vec<String>,
    /// The notes list's scroll offset.
    offset: f64,
    /// The title being edited and the note it belongs to.
    draft: Option<Draft>,
    /// The data revision the draft was taken against; `None` while the data
    /// in memory was not stored.
    data_revision: Option<Revision>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Draft {
    note: NoteId,
    title: String,
}

impl Document for Session {
    const NAME: StorageName = StorageName::machine_local("session");
    const VERSION: u32 = 1;

    fn initial() -> Self {
        Self::default()
    }

    fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("BUG: the session serializes to JSON")
    }

    fn decode(_version: u32, body: &[u8]) -> Result<Self, DecodeError> {
        serde_json::from_slice::<Self>(body).map_err(|error| DecodeError::new(error.to_string()))
    }
}

/// What a stored session restores over the loaded data.
struct Restored {
    stack: Vec<Route>,
    offset: f64,
    draft: Option<Draft>,
}

impl Session {
    /// What of this session still fits `notebook`, stored as `revision`.
    ///
    /// The stack is kept only if every path parses, it starts at Home and
    /// every note it opens exists; otherwise it is Home alone. The draft is
    /// kept only if it was taken against this very revision, its note exists
    /// and no other note is open above it; an open note without its draft
    /// edits its stored title. The offset is kept if it is finite, and the
    /// list's layout clamps it to the extent it has now.
    fn restore(&self, notebook: &Notebook, revision: Option<Revision>) -> Restored {
        let stack = self
            .stack
            .iter()
            .map(|path| Route::parse(path))
            .collect::<Result<Vec<Route>, _>>()
            .ok()
            .filter(|stack| {
                stack.first() == Some(&Route::Home)
                    && stack.iter().all(|route| match route {
                        Route::Note { id } => notebook.title(*id).is_some(),
                        Route::Home | Route::Settings => true,
                    })
            })
            .unwrap_or_else(|| vec![Route::Home]);
        let open = stack.iter().rev().find_map(|route| match route {
            Route::Note { id } => Some(*id),
            Route::Home | Route::Settings => None,
        });
        let draft = self
            .draft
            .clone()
            .filter(|draft| {
                self.data_revision.is_some()
                    && self.data_revision == revision
                    && notebook.title(draft.note).is_some()
                    && open.is_none_or(|open| open == draft.note)
            })
            .or_else(|| {
                let note = open?;
                Some(Draft {
                    note,
                    title: notebook.title(note)?.to_owned(),
                })
            });
        Restored {
            stack,
            offset: if self.offset.is_finite() {
                self.offset
            } else {
                0.0
            },
            draft,
        }
    }
}

/// Writes the session whenever where the user is changes: a route change, a
/// scroll of the list, an edit of the draft, or a data write the draft is now
/// taken against; and once the session settles, so a write it refused while a
/// fresh start ran is made.
///
/// Its listeners hold it weakly, and it holds the router only while Home,
/// the bottom page, is mounted, so it keeps nothing alive past the tree.
struct Place {
    session: Persisted<Session>,
    data: Persisted<Saved>,
    draft: TextEditingController,
    editing: Rc<Cell<Option<NoteId>>>,
    scroll: ScrollController,
    router: RefCell<Option<(RouterHandle<Route>, ListenerId)>>,
    /// What it listens to while the loaded screens are up.
    listening: RefCell<Option<Listening>>,
    /// The session as last read or written, so a write that changes
    /// nothing is not made.
    stored: RefCell<Session>,
}

/// The listeners a [`Place`] keeps.
struct Listening {
    data: ListenerId,
    draft: ListenerId,
    /// The scroll position's notifier and the listener on it.
    scroll: Rc<dyn Listenable>,
    offset: ListenerId,
    session: ListenerId,
}

impl Place {
    /// A listener that writes the session, if the place still exists.
    fn listener(self: &Rc<Self>) -> Rc<dyn Fn()> {
        let place = Rc::downgrade(self);
        Rc::new(move || {
            if let Some(place) = place.upgrade() {
                place.store();
            }
        })
    }

    /// Listen to the data, the draft, the list's offset and the session
    /// itself: a write refused while a fresh start was running is made once
    /// the session settles.
    fn listen(self: &Rc<Self>) {
        let scroll = self.scroll.listenable();
        let ids = Listening {
            data: self.data.add_listener(self.listener()),
            draft: self.draft.add_listener(self.listener()),
            offset: scroll.add_listener(self.listener()),
            session: self.session.add_listener(self.listener()),
            scroll,
        };
        *self.listening.borrow_mut() = Some(ids);
    }

    /// Stop listening to anything.
    fn stop(&self) {
        if let Some(listening) = self.listening.take() {
            self.data.remove_listener(listening.data);
            self.draft.remove_listener(listening.draft);
            listening.scroll.remove_listener(listening.offset);
            self.session.remove_listener(listening.session);
        }
        self.detach();
    }

    /// Follow the stack of the router Home's page sits under.
    fn attach(self: &Rc<Self>, router: RouterHandle<Route>) {
        let id = router.add_listener(self.listener());
        if let Some((old, id)) = self.router.replace(Some((router, id))) {
            old.remove_listener(id);
        }
    }

    /// Stop following the router: Home's page is gone.
    fn detach(&self) {
        if let Some((router, id)) = self.router.take() {
            router.remove_listener(id);
        }
    }

    /// Write the session as it stands. Nothing is written before Home
    /// mounts, nor when nothing changed.
    fn store(&self) {
        let Some(stack) = self
            .router
            .borrow()
            .as_ref()
            .map(|(router, _id)| router.stack())
        else {
            return;
        };
        let session = Session {
            stack: stack
                .iter()
                .map(|route| route.to_path().to_string())
                .collect::<Vec<String>>(),
            offset: self.scroll.pixels(),
            draft: self.editing.get().map(|note| Draft {
                note,
                title: self.draft.text(),
            }),
            data_revision: self.data.revision(),
        };
        if *self.stored.borrow() == session {
            return;
        }
        // A write that fails is reported by the session's status; the place
        // is written again on the next change or when the session settles.
        if self.session.set(session.clone()).is_ok() {
            *self.stored.borrow_mut() = session;
        }
    }
}

/// The application state every screen shares. Signals are handles, and the
/// two controllers share their storage between clones.
#[derive(Clone)]
struct Shared {
    notebook: Signal<Notebook>,
    /// The note the draft belongs to. A cell, not a signal: the session
    /// reads it from listeners, which have no context.
    editing: Rc<Cell<Option<NoteId>>>,
    compact: Signal<bool>,
    status: Signal<String>,
    draft: TextEditingController,
    scroll: ScrollController,
    /// Where the notebook and the compact flag are stored.
    data: Persisted<Saved>,
    /// What writes the session; weak, since the router's pages hold this
    /// state and the place holds the router.
    place: Weak<Place>,
    /// The window's close as the screens show it; `None` on a host that
    /// hands out no close guard.
    close: Option<CloseWatch>,
    faults: Faults,
    planter: Option<Planter>,
}

/// A presentation's close guard and a notifier told each time what it has
/// recorded changes, for the screens to show a close waiting on the user.
#[derive(Clone)]
struct CloseWatch {
    guard: CloseGuard,
    changed: Rc<ChangeNotifier>,
    /// The task that follows the guard; it ends with the last clone.
    _following: Rc<TaskToken>,
}

impl CloseWatch {
    fn start(guard: CloseGuard, driver: &AsyncDriver) -> Self {
        let changed = Rc::new(ChangeNotifier::new());
        let told = Rc::downgrade(&changed);
        let followed = guard.clone();
        let following = driver.spawn_local(Box::pin(async move {
            loop {
                // The next future is taken in the same turn the change is
                // told, so a later change always wakes it.
                followed.changed().await;
                let Some(told) = told.upgrade() else {
                    return;
                };
                told.notify_listeners();
            }
        }));
        Self {
            guard,
            changed,
            _following: Rc::new(following),
        }
    }
}

impl Shared {
    /// Write the notebook and the compact flag as they stand.
    fn store(&self, cx: &EventCx<'_>) {
        let saved = Saved {
            notebook: self
                .notebook
                .peek(cx, Clone::clone)
                .expect("BUG: the loaded state's signals outlive its screens"),
            compact: self
                .compact
                .peek(cx, |compact| *compact)
                .expect("BUG: the loaded state's signals outlive its screens"),
        };
        // A write that fails is reported by the document's status and kept
        // in memory; the screens go on with it.
        let _revision = self.data.set(saved);
    }
}

#[derive(Clone, StatefulView)]
struct Root {
    faults: Faults,
    loader: Loader,
    first_attempt: u32,
    planter: Option<Planter>,
}

struct RootState {
    documents: Option<Documents>,
    attempt: Signal<u32>,
    first_attempt: u32,
    /// Set by Start over: the next load starts from the initial notes, which
    /// the data document is storing over the unreadable ones.
    fresh: Rc<Cell<bool>>,
    _kept: Option<Rc<dyn Any>>,
}

/// Why the notes did not load.
#[derive(Clone, Debug)]
enum LoadFailure {
    /// The stored notes cannot be read back: Start over keeps them aside and
    /// begins again.
    Unreadable(String),
    /// Anything else, which Retry may get past.
    Other(String),
}

impl From<PersistError> for LoadFailure {
    fn from(error: PersistError) -> Self {
        match error {
            PersistError::Corrupt { .. } | PersistError::Decode(_) => {
                Self::Unreadable(error.to_string())
            }
            other => Self::Other(other.to_string()),
        }
    }
}

impl fmt::Display for LoadFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(message) | Self::Other(message) => f.write_str(message),
        }
    }
}

/// The two stored documents.
#[derive(Clone)]
struct Documents {
    data: Persisted<Saved>,
    session: Persisted<Session>,
}

/// What a load reads: the data, then the session over it.
type Read = (Rc<Saved>, Rc<Session>);

impl StatefulView for Root {
    type State = RootState;

    fn create_state(&self) -> Self::State {
        RootState {
            documents: None,
            attempt: Signal::default(),
            first_attempt: self.first_attempt,
            fresh: Rc::new(Cell::new(false)),
            _kept: plant(self.planter.as_ref(), Kept::State),
        }
    }
}

impl ViewState<Root> for RootState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.attempt = cx.signal(self.first_attempt);
        self.documents = Some(Documents {
            data: Persisted::<Saved>::open(cx),
            session: Persisted::<Session>::open(cx),
        });
    }

    fn build(&self, view: &Root, cx: &dyn BuildContext) -> impl IntoView {
        let documents = self
            .documents
            .clone()
            .expect("BUG: init_state precedes build");
        let attempt = self.attempt;
        let current = attempt.get(cx);
        let loader = Rc::clone(&view.loader);
        let load = documents.clone();
        let faults = view.faults;
        let planter = view.planter.clone();
        let fresh = Rc::clone(&self.fresh);
        let starting_over = Rc::clone(&self.fresh);
        let ready = FutureBuilder::keyed(
            Some(current),
            Rc::new(move || {
                let gate = loader(current);
                let Documents { data, session } = load.clone();
                let fresh = Rc::clone(&fresh);
                Box::pin(async move {
                    gate.await.map_err(LoadFailure::Other)?;
                    // The session is read only over loaded data: a failed
                    // data load leaves it unread and unwritten.
                    let saved = if fresh.get() {
                        // The notes are shown once the fresh start is
                        // stored, so no edit is refused while the old ones
                        // are kept aside.
                        started_over(&data).await.map_err(LoadFailure::Other)?;
                        Rc::new(Saved::initial())
                    } else {
                        match data.load().await {
                            Ok(saved) => saved,
                            // Without storage the notes live in memory; the
                            // banner says so.
                            Err(_) if matches!(data.status(), SaveStatus::Unavailable) => {
                                Rc::new(Saved::initial())
                            }
                            Err(error) => return Err(LoadFailure::from(error)),
                        }
                    };
                    // A session that cannot be read loses the place, never the
                    // data: an unreadable one is kept aside and started over.
                    let place = match session.load().await {
                        Ok(place) => place,
                        Err(_unreadable) => {
                            session.start_over();
                            Rc::new(Session::initial())
                        }
                    };
                    // A place taken in notes given up is never restored over
                    // fresh ones, which can take their revision and note ids.
                    // The session is still loaded, so the new place is
                    // written over it once the fresh notes' revision is told.
                    let place = if fresh.replace(false) {
                        Rc::new(Session::initial())
                    } else {
                        place
                    };
                    Ok((saved, place))
                }) as BoxedResultFuture<Read, LoadFailure>
            }),
            Rc::new(move |_cx, snapshot| {
                if snapshot.connection_state() != ConnectionState::Done {
                    // Reload replaces the load under way: its future is
                    // dropped, so nothing it read is applied.
                    let reload = TextButton::new(Text::new("Reload")).on_pressed(move |cx| {
                        attempt.update(cx, |attempt| *attempt = attempt.wrapping_add(1))
                    });
                    return Column::new(column![Text::new("Loading notes"), reload]).boxed();
                }
                if let Some(error) = snapshot.error() {
                    let resume = documents.data.clone();
                    let resuming = Rc::clone(&starting_over);
                    let retry = TextButton::new(Text::new("Retry")).on_pressed(move |cx| {
                        // A fresh start that failed is taken up where it
                        // stopped; the next attempt waits for it.
                        if resuming.get() {
                            resume.retry();
                        }
                        attempt.update(cx, |attempt| *attempt = attempt.wrapping_add(1))
                    });
                    let message = Text::new(format!("Load failed: {error}"));
                    if !matches!(error, LoadFailure::Unreadable(_)) {
                        return Column::new(column![message, retry]).boxed();
                    }
                    let data = documents.data.clone();
                    let starting_over = Rc::clone(&starting_over);
                    let start_over =
                        TextButton::new(Text::new("Start over")).on_pressed(move |cx| {
                            // The unreadable notes are kept aside, then the
                            // initial ones are stored and shown.
                            data.start_over();
                            starting_over.set(true);
                            attempt.update(cx, |attempt| *attempt = attempt.wrapping_add(1))
                        });
                    return Column::new(column![message, retry, start_over]).boxed();
                }
                let read = snapshot
                    .data()
                    .cloned()
                    .expect("BUG: a load done without an error has its value");
                Loaded {
                    read,
                    documents: documents.clone(),
                    faults,
                    planter: planter.clone(),
                }
                .boxed()
            }),
        );
        Theme::new(ThemeData::light(), ready)
    }
}

/// The screens over the loaded documents: their state starts from them.
#[derive(Clone, StatefulView)]
struct Loaded {
    read: Read,
    documents: Documents,
    faults: Faults,
    planter: Option<Planter>,
}

struct LoadedState {
    /// What `init_state` starts the shared state from.
    start: Option<(Read, Documents)>,
    shared: Option<Shared>,
    /// The stack the router opens on.
    stack: Vec<Route>,
    place: Option<Rc<Place>>,
    faults: Faults,
    planter: Option<Planter>,
    _kept: Option<Rc<dyn Any>>,
}

impl StatefulView for Loaded {
    type State = LoadedState;

    fn create_state(&self) -> Self::State {
        LoadedState {
            start: Some((self.read.clone(), self.documents.clone())),
            shared: None,
            stack: Vec::new(),
            place: None,
            faults: self.faults,
            planter: self.planter.clone(),
            _kept: plant(self.planter.as_ref(), Kept::State),
        }
    }
}

impl ViewState<Loaded> for LoadedState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        let (
            (saved, session),
            Documents {
                data,
                session: stored,
            },
        ) = self.start.take().expect("BUG: init_state runs once");
        // Everything restored is in place before the first layout, so the
        // first loaded frame already shows it.
        let restored = session.restore(&saved.notebook, data.revision());
        let draft = TextEditingController::new();
        if let Some(Draft { title, .. }) = &restored.draft {
            draft.set_text(title.clone());
        }
        if let Some(kept) = plant(self.planter.as_ref(), Kept::Listener) {
            draft.add_listener(Rc::new(move || {
                let _kept = &kept;
            }));
        }
        let editing = Rc::new(Cell::new(restored.draft.map(|draft| draft.note)));
        let scroll = ScrollController::with_initial_scroll_offset(restored.offset);
        let place = Rc::new(Place {
            session: stored,
            data: data.clone(),
            draft: draft.clone(),
            editing: Rc::clone(&editing),
            scroll: scroll.clone(),
            router: RefCell::new(None),
            listening: RefCell::new(None),
            stored: RefCell::new(Session::clone(&session)),
        });
        place.listen();
        self.shared = Some(Shared {
            notebook: cx.signal(saved.notebook.clone()),
            editing,
            compact: cx.signal(saved.compact),
            status: cx.signal(String::new()),
            draft,
            scroll,
            data,
            place: Rc::downgrade(&place),
            close: cx
                .close_guard()
                .zip(cx.async_driver())
                .map(|(guard, driver)| CloseWatch::start(guard, &driver)),
            faults: self.faults,
            planter: self.planter.clone(),
        });
        self.stack = restored.stack;
        self.place = Some(place);
    }

    fn build(&self, _view: &Loaded, _cx: &dyn BuildContext) -> impl IntoView {
        let shared = self.shared.clone().expect("BUG: init_state precedes build");
        let router = Router::from_stack(self.stack.clone(), move |route: &Route, _cx| {
            Screen {
                route: route.clone(),
                shared: shared.clone(),
                _kept: plant(shared.planter.as_ref(), Kept::Page),
            }
            .boxed()
        })
        .expect("BUG: a restored stack holds Home at least");
        WidgetsApp::router(router)
    }

    fn dispose(&mut self) {
        if let Some(place) = self.place.take() {
            place.stop();
        }
    }
}

#[derive(Clone, StatefulView)]
struct Screen {
    route: Route,
    shared: Shared,
    _kept: Option<Rc<dyn Any>>,
}

struct ScreenState {
    router: Option<RouterHandle<Route>>,
    form: FormHandle,
    focus: Rc<FocusNode>,
    panic_on_dispose: bool,
    /// On Home's page, the bottom one: what writes the session, which
    /// follows the router while the page is mounted.
    place: Option<Weak<Place>>,
    _kept: Option<Rc<dyn Any>>,
}

impl StatefulView for Screen {
    type State = ScreenState;

    fn create_state(&self) -> Self::State {
        ScreenState {
            router: None,
            form: FormHandle::new(),
            focus: FocusNode::new(),
            panic_on_dispose: self.shared.faults.contains(Fault::Dispose)
                && matches!(self.route, Route::Note { .. }),
            place: (self.route == Route::Home).then(|| Weak::clone(&self.shared.place)),
            _kept: plant(self.shared.planter.as_ref(), Kept::State),
        }
    }
}

impl ViewState<Screen> for ScreenState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        let router = Router::<Route>::handle(cx).expect("BUG: a screen is under the Router");
        if let Some(place) = self.place.as_ref().and_then(Weak::upgrade) {
            place.attach(router.clone());
        }
        self.router = Some(router);
    }

    fn build(&self, view: &Screen, cx: &dyn BuildContext) -> impl IntoView {
        let router = self.router.clone().expect("BUG: init_state precedes build");
        let shared = &view.shared;
        let content = match view.route {
            Route::Home => home(cx, shared, &router),
            Route::Note { .. } if shared.faults.contains(Fault::Build) => FailingEditor {
                faults: shared.faults,
            }
            .boxed(),
            Route::Note { id } => editor(cx, id, shared, &router, &self.form, &self.focus),
            Route::Settings => settings(cx, shared),
        };
        SafeArea::new().child(Column::new(column![
            Row::new(header(&view.route, &router)),
            Text::new(shared.status.get(cx)),
            storage_banner(&shared.data),
            close_decision(shared.close.as_ref()),
            Expanded::new(content),
        ]))
    }

    fn dispose(&mut self) {
        if let Some(place) = self.place.take().as_ref().and_then(Weak::upgrade) {
            place.detach();
        }
        assert!(
            !self.panic_on_dispose,
            "injected fault: the Note screen's dispose panics"
        );
    }
}

/// The Note screen's editor with [`Fault::Build`] injected: its `build`
/// panics.
#[derive(Clone, StatelessView)]
struct FailingEditor {
    faults: Faults,
}

impl StatelessView for FailingEditor {
    fn build(&self, _cx: &dyn BuildContext) -> impl IntoView {
        assert!(
            !self.faults.contains(Fault::Build),
            "injected fault: the Note screen's editor panics in build"
        );
        Text::new("")
    }
}
/// A button that acts only while `route` is the current page: a page stays
/// actionable while it animates in or out, and a departing page must not act
/// for the page that replaced it.
fn on_current_page(
    label: &str,
    router: &RouterHandle<Route>,
    route: &Route,
    action: impl Fn(&mut EventCx<'_>, &RouterHandle<Route>) + 'static,
) -> TextButton {
    let router = router.clone();
    let route = route.clone();
    TextButton::new(Text::new(label)).on_pressed(move |cx| {
        if router.current() == route {
            action(cx, &router);
        }
    })
}

fn header(route: &Route, router: &RouterHandle<Route>) -> Vec<BoxedView> {
    let mut header = vec![Text::new("Notes").boxed()];
    if *route != Route::Settings {
        header.push(
            on_current_page("Settings", router, route, |_cx, router| {
                router.push(Route::Settings).expect("BUG: mounted Router");
            })
            .boxed(),
        );
    }
    if *route != Route::Home {
        header.push(
            on_current_page("Back", router, route, |_cx, router| {
                router.pop().expect("BUG: mounted Router");
            })
            .boxed(),
        );
    }
    header
}

fn open_note(cx: &mut EventCx<'_>, shared: &Shared, router: &RouterHandle<Route>, id: NoteId) {
    if shared.editing.get() != Some(id) {
        let title = shared
            .notebook
            .peek(cx, |notebook| notebook.title(id).map(str::to_owned))
            .expect("BUG: the loaded state's signals outlive its screens");
        // The draft names its note before its text changes, so the session
        // written for the new text names the right note.
        shared.editing.set(Some(id));
        shared.draft.set_text(title.unwrap_or_default());
    }
    router
        .push(Route::Note { id })
        .expect("BUG: mounted Router");
}

fn home(cx: &dyn BuildContext, shared: &Shared, router: &RouterHandle<Route>) -> BoxedView {
    let notes: Rc<[Note]> = shared
        .notebook
        .with(cx, |notebook| notebook.notes.clone().into());
    let compact = shared.compact.get(cx);
    let extent = if compact {
        COMPACT_ROW_HEIGHT
    } else {
        ROW_HEIGHT
    };
    let rows = {
        let shared = shared.clone();
        let router = router.clone();
        Scrollable::new()
            .controller(shared.scroll.clone())
            .viewport_builder(Rc::new(move |position| {
                let built = Rc::clone(&notes);
                let found = Rc::clone(&notes);
                let shared = shared.clone();
                let router = router.clone();
                ListView::builder(notes.len(), extent, move |index| {
                    let note = built.get(index)?;
                    Some(
                        NoteRow {
                            key: ValueKey::new(note.id),
                            title: note.title.clone(),
                            extent,
                            shared: shared.clone(),
                            router: router.clone(),
                        }
                        .boxed(),
                    )
                })
                .find_index_by_key(move |key| {
                    let id = key.as_any().downcast_ref::<ValueKey<NoteId>>()?.value();
                    found.iter().position(|note| note.id == *id)
                })
                .position(position)
                .focusable(true)
                .semantics_label("Notes")
                .empty_label("No notes")
                .boxed()
            }))
    };
    let toggle = shared.clone();
    let add = shared.clone();
    Column::new(column![
        Row::new(vec![
            on_current_page(
                if compact {
                    "Compact rows: on"
                } else {
                    "Compact rows: off"
                },
                router,
                &Route::Home,
                move |cx, _router| {
                    toggle
                        .compact
                        .update(cx, |compact| *compact = !*compact)
                        .expect("BUG: the loaded state's signals outlive its screens");
                    toggle.store(cx);
                },
            )
            .boxed(),
            on_current_page("New note", router, &Route::Home, move |cx, router| {
                let id = add
                    .notebook
                    .update(cx, Notebook::add)
                    .expect("BUG: the loaded state's signals outlive its screens");
                add.store(cx);
                open_note(cx, &add, router, id);
            })
            .boxed(),
        ]),
        Expanded::new(rows),
    ])
    .boxed()
}

/// One note's row, keyed by the note's id: the row of the same note keeps
/// its element, and so its focus, across a rebuild that moves or resizes it.
#[derive(Clone)]
struct NoteRow {
    key: ValueKey<NoteId>,
    title: String,
    extent: f64,
    shared: Shared,
    router: RouterHandle<Route>,
}

impl StatelessView for NoteRow {
    fn build(&self, _cx: &dyn BuildContext) -> impl IntoView {
        let id = *self.key.value();
        let shared = self.shared.clone();
        SizedBox::height(self.extent).child(
            on_current_page(
                &self.title,
                &self.router,
                &Route::Home,
                move |cx, router| {
                    open_note(cx, &shared, router, id);
                },
            )
            .style(flat_row()),
        )
    }
}

impl View for NoteRow {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

fn flat_row() -> ButtonStyle {
    ButtonStyle {
        minimum_size: Some(WidgetStateProperty::all(Some(Size::ZERO))),
        padding: Some(WidgetStateProperty::all(Some(EdgeInsets::ZERO))),
        ..ButtonStyle::default()
    }
}

fn editor(
    cx: &dyn BuildContext,
    id: NoteId,
    shared: &Shared,
    router: &RouterHandle<Route>,
    form: &FormHandle,
    focus: &Rc<FocusNode>,
) -> BoxedView {
    let exists = shared
        .notebook
        .with(cx, |notebook| notebook.title(id).is_some());
    if !exists {
        return Text::new(format!("Note {id} does not exist")).boxed();
    }
    let save = shared.clone();
    let delete = shared.clone();
    let validate = form.clone();
    Form::new(Column::new(column![
        Text::new(format!("Editing note {id}")),
        TextFormField::new(shared.draft.clone())
            .focus_node(Rc::clone(focus))
            .autofocus(true)
            .decoration(InputDecoration {
                label_text: Some("Title".to_owned()),
                ..InputDecoration::default()
            })
            .validator(|value| value.trim().is_empty().then(|| "Enter a title".to_owned())),
        on_current_page(
            "Save note",
            router,
            &Route::Note { id },
            move |cx, _router| {
                assert!(
                    !save.faults.contains(Fault::Save),
                    "injected fault: pressing Save note panics"
                );
                let message = if validate.validate() {
                    let title = save.draft.text();
                    save.notebook
                        .update(cx, |notebook| {
                            if let Some(note) = notebook.notes.iter_mut().find(|note| note.id == id)
                            {
                                note.title.clone_from(&title);
                            }
                        })
                        .expect("BUG: the loaded state's signals outlive its screens");
                    save.store(cx);
                    format!("Saved note {id}")
                } else {
                    "Fix the title".to_owned()
                };
                save.status
                    .set(cx, message)
                    .expect("BUG: the loaded state's signals outlive its screens");
            }
        ),
        on_current_page(
            "Delete note",
            router,
            &Route::Note { id },
            move |cx, router| {
                delete
                    .notebook
                    .update(cx, |notebook| notebook.notes.retain(|note| note.id != id))
                    .expect("BUG: the loaded state's signals outlive its screens");
                if delete.editing.get() == Some(id) {
                    delete.editing.set(None);
                }
                delete.store(cx);
                delete
                    .status
                    .set(cx, format!("Deleted note {id}"))
                    .expect("BUG: the loaded state's signals outlive its screens");
                router.pop().expect("BUG: mounted Router");
            }
        ),
    ]))
    .handle(form.clone())
    .boxed()
}

fn settings(cx: &dyn BuildContext, shared: &Shared) -> BoxedView {
    let compact = if shared.compact.get(cx) { "on" } else { "off" };
    Text::new(format!("Compact rows are {compact}")).boxed()
}

/// What the notes' storage cannot do now, if anything: no storage at all, a
/// file this build must not write, or a save that failed, with Retry.
fn storage_banner(data: &Persisted<Saved>) -> BoxedView {
    let watched = data.clone();
    let listenable: Rc<dyn Listenable> = Rc::new(data.clone());
    AnimatedBuilder::new(listenable, move || match watched.status() {
        SaveStatus::Unavailable => {
            Text::new("Storage is unavailable: the notes are kept in memory only").boxed()
        }
        SaveStatus::ReadOnly(_) => {
            Text::new("Read only: changes to the notes are not saved").boxed()
        }
        SaveStatus::Failed(error) => {
            let data = watched.clone();
            Row::new(vec![
                Text::new(format!("Saving failed: {error}")).boxed(),
                TextButton::new(Text::new("Retry saving"))
                    .on_pressed(move |_cx| data.retry())
                    .boxed(),
            ])
            .boxed()
        }
        _ => SizedBox::shrink().boxed(),
    })
    .boxed()
}

/// Wait until the fresh start of `data` is stored: its value is loaded and
/// takes edits from then on. A failed step ends the wait with its error.
async fn started_over(data: &Persisted<Saved>) -> Result<(), String> {
    /// Removes the waiting listener when the wait ends or is dropped.
    struct Watch {
        data: Persisted<Saved>,
        id: Option<ListenerId>,
    }
    impl Drop for Watch {
        fn drop(&mut self) {
            if let Some(id) = self.id.take() {
                self.data.remove_listener(id);
            }
        }
    }
    let waker = Rc::new(RefCell::new(None::<std::task::Waker>));
    let mut watch = Watch {
        data: data.clone(),
        id: None,
    };
    poll_fn(move |cx| {
        if let SaveStatus::Failed(error) = watch.data.status() {
            return Poll::Ready(Err(error.to_string()));
        }
        if watch.data.revision().is_some() {
            return Poll::Ready(Ok(()));
        }
        *waker.borrow_mut() = Some(cx.waker().clone());
        if watch.id.is_none() {
            let waker = Rc::clone(&waker);
            watch.id = Some(watch.data.add_listener(Rc::new(move || {
                // Taken before waking: the waker's wake may poll again.
                let waker = waker.borrow_mut().take();
                if let Some(waker) = waker {
                    waker.wake();
                }
            })));
        }
        Poll::Pending
    })
    .await
}

/// A close the window holds, with the user's two ways out: close without
/// saving what the holds protect, or keep the window open. Retry saving is
/// on the storage banner; a save that succeeds then carries the close out.
fn close_decision(close: Option<&CloseWatch>) -> BoxedView {
    let Some(close) = close else {
        return SizedBox::shrink().boxed();
    };
    let listenable: Rc<dyn Listenable> = Rc::clone(&close.changed) as Rc<dyn Listenable>;
    let guard = close.guard.clone();
    AnimatedBuilder::new(listenable, move || {
        if guard.pending().is_none() {
            return SizedBox::shrink().boxed();
        }
        // Each button acts on the close recorded when it is pressed: one
        // shown earlier may have been carried out or withdrawn since.
        let discard = guard.clone();
        let keep = guard.clone();
        let buttons = Row::new(vec![
            TextButton::new(Text::new("Close without saving"))
                .on_pressed(move |_cx| {
                    if let Some(pending) = discard.pending() {
                        let _gone: Result<(), StaleClose> = pending.discard_and_close();
                    }
                })
                .boxed(),
            TextButton::new(Text::new("Keep open"))
                .on_pressed(move |_cx| {
                    if let Some(pending) = keep.pending() {
                        let _gone: Result<(), StaleClose> = pending.stay_open();
                    }
                })
                .boxed(),
        ]);
        Column::new(column![
            Text::new("Closing: unsaved changes to the notes would be lost"),
            buttons
        ])
        .boxed()
    })
    .boxed()
}
