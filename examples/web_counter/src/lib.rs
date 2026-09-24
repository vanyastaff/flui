//! The counter template, in a browser.
//!
//! This is `flui create --template counter`'s widget tree behind a
//! `#[wasm_bindgen(start)]` entry point instead of `fn main`: `run_app`
//! dispatches to the web runner on `wasm32`, which mounts the tree into the
//! page's `#flui-canvas` (or appends one) and renders through WebGPU. It is
//! the Web row's executable evidence in `docs/BETA.md`.
//!
//! Build with
//! `cargo build -p flui-web-counter --release --target wasm32-unknown-unknown`,
//! then run `wasm-bindgen --target web --out-dir examples/web_counter/pkg` on
//! the built `flui_web_counter.wasm`; serve `examples/web_counter/` over HTTP
//! and open `index.html` in a WebGPU-capable browser.

use flui::prelude::*;
use flui::widgets::{SafeArea, column};
use wasm_bindgen::prelude::*;

/// Module entry: install the panic hook so a failure reads as a console
/// error rather than an opaque `unreachable`, then hand the root to `run_app`.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    run_app(CounterApp);
}

#[derive(Clone, StatelessView)]
struct CounterApp;

impl StatelessView for CounterApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Theme::new(ThemeData::light(), SafeArea::new().child(CounterView))
    }
}

#[derive(Clone, StatefulView)]
struct CounterView;

struct CounterState {
    count: StateCell<usize>,
}

impl StatefulView for CounterView {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            count: StateCell::new(0),
        }
    }
}

impl ViewState<CounterView> for CounterState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count.bind(ctx);
    }

    fn build(&self, _view: &CounterView, _ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count.clone();
        Center::new().child(
            Column::new(column![
                Text::new("You have pushed the button this many times:"),
                SizedBox::height(16.0),
                Text::new(self.count.get().to_string()),
                SizedBox::height(16.0),
                ElevatedButton::new(Text::new("Increment"))
                    .on_pressed(move || count.update(|n| n + 1)),
            ])
            .main_axis_alignment(MainAxisAlignment::Center),
        )
    }
}
