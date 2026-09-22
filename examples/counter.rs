//! Counter — the minimal FLUI "hello world": one piece of state and a button
//! that updates it. This is the exact `CounterView`/`CounterState` shape
//! `flui create` generates for the `counter` template
//! (`crates/flui-cli/src/templates/counter.rs`'s `LIB` constant) — kept in
//! sync deliberately, so this example and the generated project never drift
//! apart. For a tour of the wider widget catalog, see
//! `examples/widgets_gallery.rs`.
//!
//! Run with: cargo run --example counter

use flui::prelude::*;
use flui::widgets::column;

#[derive(Clone, StatelessView)]
pub struct CounterApp;

impl StatelessView for CounterApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Theme::new(ThemeData::light(), CounterView)
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
    fn init_state(&mut self, ctx: &dyn BuildContext) {
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

fn main() {
    run_app(CounterApp);
}
