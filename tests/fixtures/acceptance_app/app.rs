//! The acceptance application: a small notes app built only on the `flui`
//! facade, the way an external consumer writes one.
//!
//! Three screens under one [`Router`]: Home lists the notes and toggles compact
//! rows, Note edits a title through a [`TextFormField`] and keeps its draft
//! after Back, Settings sits over either. Readiness is a keyed
//! [`FutureBuilder`] above the router, with Loading, an error and Retry.
//! Everything is held in memory by the root's state, so a restart starts over.
//!
//! [`Faults`] is the constructor's seam for injected failures, a set of
//! [`Fault`]s: a panic in the Note editor's `build`, in the Save handler, or in
//! the Note screen's `dispose`. [`Planter`] hands it a value to keep in each
//! state, page and listener it makes, which a test turns into a drop sentinel.

use std::any::Any;
use std::fmt;
use std::future::poll_fn;
use std::num::ParseIntError;
use std::rc::Rc;
use std::str::FromStr;
use std::task::Poll;

use flui::foundation::{ConnectionState, Listenable, ValueKey, ViewKey};
use flui::geometry::{EdgeInsets, Size};
use flui::interaction::FocusNode;
use flui::material::{ButtonStyle, InputDecoration, TextButton, TextFormField, Theme, ThemeData};
use flui::prelude::*;
use flui::view::View;
use flui::view::element::ElementKind;
use flui::widgets::{
    BoxedResultFuture, Form, FormHandle, FutureBuilder, SafeArea, Scrollable, WidgetStateProperty,
    column,
};

/// How many notes a fresh start opens with: enough to scroll.
const SEEDED_NOTES: u64 = 40;
const ROW_HEIGHT: f64 = 48.0;
const COMPACT_ROW_HEIGHT: f64 = 32.0;

/// A note's identity. Ids are handed out in increasing order and never
/// reused, so a stale id can only name nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

/// The readiness operation, called with the attempt number.
///
/// The attempt number is a key, not a count: Retry moves it to the next value,
/// wrapping from `u32::MAX` to 0, so it always differs from the last one and
/// always starts a new load. A [`NoteId`] is an identity and never wraps.
pub type Loader = Rc<dyn Fn(u32) -> BoxedResultFuture<(), String>>;

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

#[derive(Clone, Debug, PartialEq)]
struct Note {
    id: NoteId,
    title: String,
}

#[derive(Clone, Debug, PartialEq)]
struct Notebook {
    notes: Vec<Note>,
    /// The id the next new note takes.
    next_id: NoteId,
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

/// The application state every screen shares. Signals are handles, and the
/// two controllers share their storage between clones.
#[derive(Clone)]
struct Shared {
    notebook: Signal<Notebook>,
    /// The note the draft belongs to.
    editing: Signal<Option<NoteId>>,
    compact: Signal<bool>,
    status: Signal<String>,
    draft: TextEditingController,
    scroll: ScrollController,
    faults: Faults,
    planter: Option<Planter>,
}

#[derive(Clone, StatefulView)]
struct Root {
    faults: Faults,
    loader: Loader,
    first_attempt: u32,
    planter: Option<Planter>,
}

struct RootState {
    shared: Option<Shared>,
    attempt: Signal<u32>,
    faults: Faults,
    first_attempt: u32,
    planter: Option<Planter>,
    _kept: Option<Rc<dyn Any>>,
}

impl StatefulView for Root {
    type State = RootState;

    fn create_state(&self) -> Self::State {
        RootState {
            shared: None,
            attempt: Signal::default(),
            faults: self.faults,
            first_attempt: self.first_attempt,
            planter: self.planter.clone(),
            _kept: plant(self.planter.as_ref(), Kept::State),
        }
    }
}

impl ViewState<Root> for RootState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.attempt = cx.signal(self.first_attempt);
        let draft = TextEditingController::new();
        if let Some(kept) = plant(self.planter.as_ref(), Kept::Listener) {
            draft.add_listener(Rc::new(move || {
                let _kept = &kept;
            }));
        }
        self.shared = Some(Shared {
            notebook: cx.signal(Notebook::seeded()),
            editing: cx.signal(None),
            compact: cx.signal(false),
            status: cx.signal(String::new()),
            draft,
            scroll: ScrollController::new(),
            faults: self.faults,
            planter: self.planter.clone(),
        });
    }

    fn build(&self, view: &Root, cx: &dyn BuildContext) -> impl IntoView {
        let shared = self.shared.clone().expect("BUG: init_state precedes build");
        let attempt = self.attempt;
        let current = attempt.get(cx);
        let loader = Rc::clone(&view.loader);
        let ready = FutureBuilder::keyed(
            Some(current),
            Rc::new(move || loader(current)),
            Rc::new(move |_cx, snapshot| {
                if snapshot.connection_state() != ConnectionState::Done {
                    return Text::new("Loading notes").boxed();
                }
                if let Some(error) = snapshot.error() {
                    return Column::new(column![
                        Text::new(format!("Load failed: {error}")),
                        TextButton::new(Text::new("Retry")).on_pressed(move |cx| {
                            attempt.update(cx, |attempt| *attempt = attempt.wrapping_add(1))
                        }),
                    ])
                    .boxed();
                }
                let shared = shared.clone();
                WidgetsApp::router(Router::new(Route::Home, move |route: &Route, _cx| {
                    Screen {
                        route: route.clone(),
                        shared: shared.clone(),
                        _kept: plant(shared.planter.as_ref(), Kept::Page),
                    }
                    .boxed()
                }))
                .boxed()
            }),
        );
        Theme::new(ThemeData::light(), ready)
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
            _kept: plant(self.shared.planter.as_ref(), Kept::State),
        }
    }
}

impl ViewState<Screen> for ScreenState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.router = Some(Router::<Route>::handle(cx).expect("BUG: a screen is under the Router"));
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
            Expanded::new(content),
        ]))
    }

    fn dispose(&mut self) {
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
    let editing = shared
        .editing
        .peek(cx, |editing| *editing)
        .expect("BUG: the root's signals outlive its screens");
    if editing != Some(id) {
        let title = shared
            .notebook
            .peek(cx, |notebook| notebook.title(id).map(str::to_owned))
            .expect("BUG: the root's signals outlive its screens");
        shared.draft.set_text(title.unwrap_or_default());
        shared
            .editing
            .set(cx, Some(id))
            .expect("BUG: the root's signals outlive its screens");
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
    let toggle = shared.compact;
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
                        .update(cx, |compact| *compact = !*compact)
                        .expect("BUG: the root's signals outlive its screens");
                },
            )
            .boxed(),
            on_current_page("New note", router, &Route::Home, move |cx, router| {
                let id = add
                    .notebook
                    .update(cx, Notebook::add)
                    .expect("BUG: the root's signals outlive its screens");
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
                        .expect("BUG: the root's signals outlive its screens");
                    format!("Saved note {id}")
                } else {
                    "Fix the title".to_owned()
                };
                save.status
                    .set(cx, message)
                    .expect("BUG: the root's signals outlive its screens");
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
