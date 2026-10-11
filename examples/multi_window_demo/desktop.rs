//! Minimal multi-window demo: primary window from `run_app`, secondary opened
//! by clicking a button through `flui::app::open_window`.

use flui::app::AppWindowError;
use flui::prelude::*;
use flui::view::StateCell;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, StatelessView)]
struct App;

impl StatelessView for App {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Theme::new(ThemeData::light(), Root)
    }
}

#[derive(Clone, StatefulView)]
struct Root;

struct RootState {
    opened: StateCell<bool>,
    opening: StateCell<bool>,
    driver: Option<flui::view::AsyncDriver>,
    request_task: Rc<RefCell<Option<flui::view::TaskToken>>>,
}

impl StatefulView for Root {
    type State = RootState;
    fn create_state(&self) -> Self::State {
        RootState {
            opened: StateCell::new(false),
            opening: StateCell::new(false),
            driver: None,
            request_task: Rc::new(RefCell::new(None)),
        }
    }
}

impl ViewState<Root> for RootState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.opened.bind(ctx);
        self.opening.bind(ctx);
        self.driver = ctx.async_driver();
    }

    fn build(&self, _view: &Root, _ctx: &dyn BuildContext) -> impl IntoView {
        let opened = self.opened.clone();
        let opening = self.opening.clone();
        let driver = self
            .driver
            .clone()
            .expect("BUG: mounted root has an async driver");
        let request_task = Rc::clone(&self.request_task);
        let label = if opened.get() {
            "Secondary opened"
        } else if opening.get() {
            "Opening secondary"
        } else {
            "Open secondary"
        };

        Center::new().child(
            Column::new(flui::widgets::column![
                Text::new("Primary window"),
                ElevatedButton::new(Text::new(label)).on_pressed(move |_cx| {
                    if opened.get() || opening.get() {
                        return;
                    }
                    let result = flui::app::open_window(
                        flui::AppConfig::new().with_title("Secondary"),
                        flui::WindowPolicy::Isolated,
                        Secondary,
                    );
                    match result {
                        Ok(request) => {
                            opening.set(true);
                            let opened = opened.clone();
                            let opening = opening.clone();
                            let task = driver.spawn_local(Box::pin(async move {
                                match request.await {
                                    Ok(_) => opened.set(true),
                                    Err(error) => {
                                        eprintln!("secondary installation failed: {error}");
                                    }
                                }
                                opening.set(false);
                            }));
                            let outgoing = request_task.borrow_mut().replace(task);
                            drop(outgoing);
                        }
                        Err(AppWindowError::AdmissionClosed) => {
                            eprintln!("the application is quitting; no new window");
                        }
                        Err(error) => eprintln!("open_window failed: {error}"),
                    }
                }),
            ])
            .main_axis_alignment(MainAxisAlignment::Center),
        )
    }
}

#[derive(Clone, StatelessView)]
struct Secondary;

impl StatelessView for Secondary {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // A real content window with its own pipeline and frame pump.
        // This is the whole point of `open_window`: a secondary native
        // window with a fully mounted widget tree and its own GPU surface.
        Center::new().child(Text::new("Secondary window says hello"))
    }
}

pub(super) fn run() {
    flui::run_app(App);
}
