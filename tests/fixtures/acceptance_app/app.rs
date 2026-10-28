//! The acceptance application: a small notes app built only on the `flui`
//! facade, the way an external consumer writes one.
//!
//! Three screens under one [`Router`]: Home lists the notes and toggles compact
//! rows, Note edits a title through a [`TextFormField`] and keeps its draft
//! after Back, Settings sits over either. Readiness is a keyed
//! [`FutureBuilder`] above the router, with Loading, an error and Retry.
//! Everything is held in memory by the root's state, so a restart starts over.
//!
//! [`Faults`] is the constructor's seam for injected failures: a panic in the
//! Note screen's `build`, in the Save handler, or in the Note screen's
//! `dispose`.

use std::fmt;
use std::future::poll_fn;
use std::num::ParseIntError;
use std::rc::Rc;
use std::str::FromStr;
use std::task::Poll;

use flui::foundation::ConnectionState;
use flui::geometry::{EdgeInsets, Size};
use flui::interaction::FocusNode;
use flui::material::{ButtonStyle, InputDecoration, TextButton, TextFormField, Theme, ThemeData};
use flui::prelude::*;
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

/// Failures injected into the application, for the rows that check what a
/// panic leaves behind. [`Faults::default`] injects none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Faults {
    /// The Note screen's `build` panics.
    pub note_build: bool,
    /// Pressing "Save note" panics before anything is saved.
    pub save_pressed: bool,
    /// The Note screen's state panics when it is disposed.
    pub note_dispose: bool,
}

/// The readiness operation, called with the attempt number: 0 for the first
/// load, one more for each Retry, wrapping at `u32::MAX`.
pub type Loader = Rc<dyn Fn(u32) -> BoxedResultFuture<(), String>>;

/// The acceptance application's root view.
#[derive(Clone, StatelessView)]
pub struct AcceptanceApp {
    faults: Faults,
    loader: Loader,
}

impl AcceptanceApp {
    /// The application with `faults` injected and a loader that succeeds on
    /// its second poll, so the first frame shows Loading.
    #[must_use]
    pub fn new(faults: Faults) -> Self {
        Self {
            faults,
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
}

impl StatelessView for AcceptanceApp {
    fn build(&self, _cx: &dyn BuildContext) -> impl IntoView {
        Root {
            faults: self.faults,
            loader: Rc::clone(&self.loader),
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
}

#[derive(Clone, StatefulView)]
struct Root {
    faults: Faults,
    loader: Loader,
}

struct RootState {
    shared: Option<Shared>,
    attempt: Signal<u32>,
    faults: Faults,
}

impl StatefulView for Root {
    type State = RootState;

    fn create_state(&self) -> Self::State {
        RootState {
            shared: None,
            attempt: Signal::default(),
            faults: self.faults,
        }
    }
}

impl ViewState<Root> for RootState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.attempt = cx.signal(0);
        self.shared = Some(Shared {
            notebook: cx.signal(Notebook::seeded()),
            editing: cx.signal(None),
            compact: cx.signal(false),
            status: cx.signal(String::new()),
            draft: TextEditingController::new(),
            scroll: ScrollController::new(),
            faults: self.faults,
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
}

struct ScreenState {
    router: Option<RouterHandle<Route>>,
    form: FormHandle,
    focus: Rc<FocusNode>,
    panic_on_dispose: bool,
}

impl StatefulView for Screen {
    type State = ScreenState;

    fn create_state(&self) -> Self::State {
        ScreenState {
            router: None,
            form: FormHandle::new(),
            focus: FocusNode::new(),
            panic_on_dispose: self.shared.faults.note_dispose
                && matches!(self.route, Route::Note { .. }),
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
            Route::Note { id } => {
                assert!(
                    !shared.faults.note_build,
                    "injected fault: the Note screen's build panics"
                );
                editor(cx, id, shared, &router, &self.form, &self.focus)
            }
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
                let notes = Rc::clone(&notes);
                let shared = shared.clone();
                let router = router.clone();
                ListView::builder(notes.len(), extent, move |index| {
                    let note = notes.get(index)?;
                    let id = note.id;
                    let shared = shared.clone();
                    Some(
                        SizedBox::height(extent)
                            .child(
                                on_current_page(
                                    &note.title,
                                    &router,
                                    &Route::Home,
                                    move |cx, router| {
                                        open_note(cx, &shared, router, id);
                                    },
                                )
                                .style(flat_row()),
                            )
                            .boxed(),
                    )
                })
                .position(position)
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
                    !save.faults.save_pressed,
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
