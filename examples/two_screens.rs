//! Two screens — navigation as a value (ADR-0093). The app's locations are
//! a `#[derive(Routable)]` enum, one `#[route("…")]` pattern per variant, and
//! `WidgetsApp::router` roots the app in a `Router` of that type, which keeps
//! the stack of route values whose top is the current location.
//!
//! Each page takes a `RouterHandle` in `init_state` — the nearest `Router` of
//! its route type — and its button pushes or pops a route value. Each page
//! also prints its own location, `route.to_path()`, where a browser would
//! show its address bar.
//!
//! Run with: cargo run --example two_screens
//! (`tests/two_screens_example.rs` mounts this file's tree headless.)

use flui::prelude::*;
use flui::widgets::column;

/// Every location of the app.
#[derive(Routable, Clone, Debug, PartialEq)]
enum AppRoute {
    #[route("/")]
    Home,
    #[route("/note/:id")]
    Note { id: u32 },
}

/// The app: a `WidgetsApp` whose routing subtree is a `Router<AppRoute>`.
#[derive(Clone, Debug, StatelessView)]
pub struct TwoScreensApp;

impl StatelessView for TwoScreensApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        WidgetsApp::router(Router::new(
            AppRoute::Home,
            |route: &AppRoute, _cx| match route {
                AppRoute::Home => HomePage.boxed(),
                AppRoute::Note { id } => NotePage { id: *id }.boxed(),
            },
        ))
    }
}

/// A page's content: its location, a title and one button.
fn page(route: &AppRoute, title: String, button: RawButton) -> impl IntoView + use<> {
    Center::new().child(
        Column::new(column![
            Text::new(route.to_path().to_string()),
            SizedBox::height(16.0),
            Text::new(title),
            SizedBox::height(16.0),
            button,
        ])
        .main_axis_alignment(MainAxisAlignment::Center),
    )
}

/// The handle a page's state acquires in `init_state`.
fn router_handle(cx: &dyn LifecycleContext) -> RouterHandle<AppRoute> {
    // A page is only ever built under its Router, so `NoRouter` here is a bug.
    Router::<AppRoute>::handle(cx).expect("BUG: a page is built under its Router")
}

#[derive(Clone, StatefulView)]
struct HomePage;

struct HomeState {
    router: Option<RouterHandle<AppRoute>>,
}

impl StatefulView for HomePage {
    type State = HomeState;

    fn create_state(&self) -> Self::State {
        HomeState { router: None }
    }
}

impl ViewState<HomePage> for HomeState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.router = Some(router_handle(cx));
    }

    fn build(&self, _view: &HomePage, _ctx: &dyn BuildContext) -> impl IntoView {
        let router = self
            .router
            .clone()
            .expect("BUG: init_state runs before build");
        page(
            &AppRoute::Home,
            "Home".to_owned(),
            RawButton::new(Text::new("Open")).on_press(move |_cx| {
                router
                    .push(AppRoute::Note { id: 1 })
                    .expect("BUG: a page's button fires only while its Router is mounted");
            }),
        )
    }
}

#[derive(Clone, StatefulView)]
struct NotePage {
    id: u32,
}

struct NoteState {
    router: Option<RouterHandle<AppRoute>>,
}

impl StatefulView for NotePage {
    type State = NoteState;

    fn create_state(&self) -> Self::State {
        NoteState { router: None }
    }
}

impl ViewState<NotePage> for NoteState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.router = Some(router_handle(cx));
    }

    fn build(&self, view: &NotePage, _ctx: &dyn BuildContext) -> impl IntoView {
        let router = self
            .router
            .clone()
            .expect("BUG: init_state runs before build");
        page(
            &AppRoute::Note { id: view.id },
            format!("Note {}", view.id),
            RawButton::new(Text::new("Back")).on_press(move |_cx| {
                router
                    .pop()
                    .expect("BUG: a page's button fires only while its Router is mounted");
            }),
        )
    }
}

fn main() {
    run_app(TwoScreensApp);
}
