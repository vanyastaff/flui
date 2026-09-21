//! Minimal multi-window demo: primary window from `run_app`, secondary opened
//! by clicking a button through `flui::app::open_window`.

use std::{cell::Cell, rc::Rc};

use flui::prelude::*;
use flui::view::{RebuildHandle, RebuildReason};

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
    opened: Rc<Cell<bool>>,
    rebuild: Option<RebuildHandle>,
}

impl StatefulView for Root {
    type State = RootState;
    fn create_state(&self) -> Self::State {
        RootState {
            opened: Rc::new(Cell::new(false)),
            rebuild: None,
        }
    }
}

impl ViewState<Root> for RootState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.rebuild = Some(ctx.rebuild_handle());
    }

    fn build(&self, _view: &Root, _ctx: &dyn BuildContext) -> impl IntoView {
        let opened = self.opened.clone();
        let rebuild = self.rebuild.clone().expect("BUG: init_state runs first");
        let label = if opened.get() { "Secondary opened" } else { "Open secondary" };

        Center::new().child(
            Column::new(flui::widgets::column![
                Text::new("Primary window"),
                ElevatedButton::new(Text::new(label)).on_pressed(move || {
                    if opened.get() {
                        return;
                    }
                    let result = flui::app::open_window(
                        flui::AppConfig::new().with_title("Secondary"),
                        flui::WindowPolicy::SeparateRealms,
                        Secondary,
                    );
                    match result {
                        Ok(()) => {
                            opened.set(true);
                            rebuild.schedule(RebuildReason::StateChange);
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

fn main() {
    flui::run_app(App);
}
