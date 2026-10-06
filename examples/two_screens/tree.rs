//! Shared production tree for a sole-facade consumer and later acceptance tests.
//! Uses the existing Router, form, Material actions and Scrollable viewport.
//! Retained application data lives above Router; this does not claim that an
//! offstage page's element is retained. The loader is deterministic, not HTTP.
use std::{future::poll_fn, rc::Rc, task::Poll};

use flui::foundation::ConnectionState;
use flui::geometry::{EdgeInsets, Size};
use flui::interaction::FocusNode;
use flui::material::{ButtonStyle, InputDecoration, TextButton, TextFormField, Theme, ThemeData};
use flui::prelude::*;
use flui::widgets::{
    BoxedResultFuture, Form, FormHandle, FutureBuilder, SafeArea, Scrollable, WidgetStateProperty,
    column,
};

/// The application's readiness operation; a real service can replace the local
/// offline fixture while the same loading, retry and cancellation UI is used.
pub type NotesLoader = Rc<dyn Fn(u32) -> BoxedResultFuture<(), &'static str>>;

const NOTE_COUNT: usize = 10_000;
const ROW_HEIGHT: f64 = 48.0;

#[derive(Routable, Clone, Debug, PartialEq)]
enum Route {
    #[route("/")]
    Home,
    #[route("/note/:id")]
    Note { id: usize },
    #[route("/settings")]
    Settings,
}

#[derive(Clone)]
struct Shared {
    titles: StateHandle<Vec<String>>,
    selected: StateHandle<Option<usize>>,
    draft: TextEditingController,
    scroll: ScrollController,
    attempt: StateHandle<u32>,
    compact: StateHandle<bool>,
    status: StateHandle<String>,
    loader: NotesLoader,
}

impl Shared {
    fn new(loader: NotesLoader) -> Self {
        Self {
            titles: StateHandle::new((0..NOTE_COUNT).map(|id| format!("Note {id}")).collect()),
            selected: StateHandle::new(None),
            draft: TextEditingController::new(),
            scroll: ScrollController::new(),
            attempt: StateHandle::new(0),
            compact: StateHandle::new(false),
            status: StateHandle::new(String::new()),
            loader,
        }
    }

    fn bind(&self, cx: &dyn LifecycleContext) {
        self.titles.bind(cx);
        self.selected.bind(cx);
        self.attempt.bind(cx);
        self.compact.bind(cx);
        self.status.bind(cx);
    }
}

#[derive(Clone, StatelessView)]
pub struct NotesApp {
    loader: NotesLoader,
}

impl NotesApp {
    /// Configure a readiness service. Repeated Reload/Retry starts a new keyed
    /// operation; the FutureBuilder owns cancellation when replaced or unmounted.
    pub fn with_loader(loader: NotesLoader) -> Self {
        Self { loader }
    }
}

impl Default for NotesApp {
    fn default() -> Self {
        Self::with_loader(Rc::new(|attempt| {
            let mut waited = false;
            Box::pin(poll_fn(move |cx| {
                if !waited {
                    waited = true;
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Poll::Ready(if attempt == 0 {
                    Err("Offline fixture: first load failed")
                } else {
                    Ok(())
                })
            }))
        }))
    }
}

impl StatelessView for NotesApp {
    fn build(&self, _cx: &dyn BuildContext) -> impl IntoView {
        NotesRoot {
            loader: self.loader.clone(),
        }
    }
}

#[derive(Clone, StatefulView)]
struct NotesRoot {
    loader: NotesLoader,
}

pub struct NotesState {
    shared: Shared,
}

impl StatefulView for NotesRoot {
    type State = NotesState;
    fn create_state(&self) -> Self::State {
        NotesState {
            shared: Shared::new(self.loader.clone()),
        }
    }
}

impl ViewState<NotesRoot> for NotesState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.shared.bind(cx);
    }

    fn build(&self, _view: &NotesRoot, _cx: &dyn BuildContext) -> impl IntoView {
        let shared = self.shared.clone();
        Theme::new(
            ThemeData::light(),
            WidgetsApp::router(Router::new(Route::Home, move |route: &Route, _cx| {
                Screen {
                    route: route.clone(),
                    shared: shared.clone(),
                }
                .boxed()
            })),
        )
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
}

impl StatefulView for Screen {
    type State = ScreenState;
    fn create_state(&self) -> Self::State {
        ScreenState {
            router: None,
            form: FormHandle::new(),
            focus: FocusNode::new(),
        }
    }
}

impl ViewState<Screen> for ScreenState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.router = Some(Router::<Route>::handle(cx).expect("BUG: screen is under Router"));
    }

    fn build(&self, view: &Screen, _cx: &dyn BuildContext) -> impl IntoView {
        let router = self.router.clone().expect("BUG: init_state precedes build");
        let shared = view.shared.clone();
        let content = match view.route {
            Route::Home => home(shared.clone(), router.clone()),
            Route::Note { id } => editor(
                id,
                shared.clone(),
                router.clone(),
                self.form.clone(),
                self.focus.clone(),
            ),
            Route::Settings => {
                let preference = shared.compact.clone();
                let settings = router.clone();
                Column::new(column![
                    Text::new(format!(
                        "Compact rows: {}",
                        shared.compact.with(|value| *value)
                    )),
                    TextButton::new(Text::new("Toggle compact rows")).on_pressed(move |_cx| {
                        // The departing Settings page stays actionable during
                        // its exit; only the current Settings may toggle.
                        if settings.current() == Route::Settings {
                            preference.update(|value| *value = !*value);
                        }
                    }),
                ])
                .boxed()
            }
        };
        let reload = shared.attempt.clone();
        let mut header = vec![Text::new("FLUI Notes").boxed()];
        // Settings is offered only where it leads somewhere: pushing it over
        // itself would stack an identical page that Back appears not to leave.
        if view.route != Route::Settings {
            let settings = router.clone();
            header.push(
                TextButton::new(Text::new("Settings"))
                    .on_pressed(move |_cx| {
                        // The outgoing page stays actionable during the
                        // entrance, so a second activation can arrive after
                        // Settings is already the current route.
                        if settings.current() != Route::Settings {
                            settings.push(Route::Settings).expect("BUG: mounted Router");
                        }
                    })
                    .boxed(),
            );
        }
        // Home is the router's root page, where there is nothing to go back to.
        if view.route != Route::Home {
            let source = view.route.clone();
            header.push(
                TextButton::new(Text::new("Back"))
                    .on_pressed(move |_cx| {
                        // A departing page stays actionable during its exit;
                        // only the page that is still current may pop, so a
                        // second activation cannot also pop the page beneath.
                        if router.current() == source {
                            router.pop().expect("BUG: mounted Router");
                        }
                    })
                    .boxed(),
            );
        }
        header.push(
            TextButton::new(Text::new("Reload notes"))
                .on_pressed(move |_cx| {
                    reload.update(|attempt| *attempt = attempt.saturating_add(1));
                })
                .boxed(),
        );
        SafeArea::new().child(Column::new(column![
            Row::new(header),
            Text::new(shared.status.with(Clone::clone)),
            Expanded::new(content),
        ]))
    }
}

fn home(shared: Shared, router: RouterHandle<Route>) -> BoxedView {
    let attempt = shared.attempt.with(|value| *value);
    let loader = shared.loader.clone();
    FutureBuilder::keyed(
        Some(attempt),
        Rc::new(move || loader(attempt)),
        Rc::new(move |_cx, snapshot| {
            if snapshot.connection_state() != ConnectionState::Done {
                return Text::new("Loading notes").boxed();
            }
            if snapshot.error().is_some() {
                let retry = shared.attempt.clone();
                return Column::new(column![
                    Text::new("Load failed"),
                    TextButton::new(Text::new("Retry")).on_pressed(move |_cx| {
                        retry.update(|value| *value = value.saturating_add(1));
                    }),
                ])
                .boxed();
            }
            if snapshot.data().is_none() {
                return Text::new("Loading notes").boxed();
            }
            let titles = shared.titles.with(Clone::clone);
            let count = titles.len();
            let rows = shared.clone();
            let navigate = router.clone();
            let extent = if shared.compact.with(|value| *value) {
                32.0
            } else {
                ROW_HEIGHT
            };
            Scrollable::new()
                .controller(shared.scroll.clone())
                .viewport_builder(Rc::new(move |position| {
                    let titles = titles.clone();
                    let rows = rows.clone();
                    let navigate = navigate.clone();
                    ListView::builder(count, extent, move |id| {
                        let title = titles.get(id)?.clone();
                        let open = rows.clone();
                        let navigate = navigate.clone();
                        Some(
                            SizedBox::height(extent)
                                .child(
                                    TextButton::new(Text::new(title))
                                        .style(ButtonStyle {
                                            minimum_size: Some(WidgetStateProperty::all(Some(
                                                Size::ZERO,
                                            ))),
                                            padding: Some(WidgetStateProperty::all(Some(
                                                EdgeInsets::ZERO,
                                            ))),
                                            ..ButtonStyle::default()
                                        })
                                        .on_pressed(move |_cx| {
                                            // Home stays actionable while a Note
                                            // animates in; only the current Home
                                            // may open a note.
                                            if navigate.current() != Route::Home {
                                                return;
                                            }
                                            if open.selected.with(|selected| *selected) != Some(id)
                                            {
                                                open.draft.set_text(
                                                    open.titles.with(|titles| titles[id].clone()),
                                                );
                                                open.selected
                                                    .update(|selected| *selected = Some(id));
                                            }
                                            navigate
                                                .push(Route::Note { id })
                                                .expect("BUG: mounted Router");
                                        }),
                                )
                                .boxed(),
                        )
                    })
                    .position(position)
                    .boxed()
                }))
                .boxed()
        }),
    )
    .boxed()
}

fn editor(
    id: usize,
    shared: Shared,
    router: RouterHandle<Route>,
    form: FormHandle,
    focus: Rc<FocusNode>,
) -> BoxedView {
    let save = shared.clone();
    let validate = form.clone();
    Form::new(Column::new(column![
        Text::new(format!("Editing note {id}")),
        TextFormField::new(shared.draft)
            .focus_node(focus)
            .decoration(InputDecoration {
                label_text: Some("Title".to_owned()),
                ..InputDecoration::default()
            })
            .validator(|value| value.trim().is_empty().then(|| "Enter a title".to_owned())),
        TextButton::new(Text::new("Save note")).on_pressed(move |_cx| {
            // A departing editor stays actionable during its exit; saving
            // from it would write an abandoned draft into Home.
            if router.current() != (Route::Note { id }) {
                return;
            }
            if validate.validate() {
                let title = save.draft.text();
                save.titles.update(|titles| {
                    if let Some(saved) = titles.get_mut(id) {
                        *saved = title;
                    }
                });
                save.status
                    .update(|status| *status = format!("Saved note {id}"));
            } else {
                save.status
                    .update(|status| "Fix the title".clone_into(status));
            }
        }),
    ]))
    .handle(form)
    .boxed()
}
