//! Counter — the minimal FLUI "hello world": one piece of state and a button
//! that updates it. This is the exact `CounterView`/`CounterState` shape
//! `flui create` generates for the `counter` template
//! (`crates/flui-cli/src/templates/counter.rs`'s `LIB` constant) — kept in
//! sync deliberately, so this example and the generated project never drift
//! apart. For a tour of the wider widget catalog, see
//! `examples/widgets_gallery.rs`.
//!
//! The count is a `Signal`: created in `init_state`, read in `build` (which
//! rebuilds this view when it changes), and written by the button's press,
//! whose callback receives the `cx` a write needs. `build` has no `cx` to
//! write with.
//!
//! Run with: cargo run --example counter

use flui::prelude::*;
use flui::widgets::{SafeArea, column};

#[derive(Clone, StatelessView)]
struct CounterApp;

impl StatelessView for CounterApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        SafeArea::new().child(CounterView)
    }
}

#[derive(Clone, StatefulView)]
struct CounterView;

#[derive(Default)]
struct CounterState {
    count: Signal<usize>,
}

impl StatefulView for CounterView {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState::default()
    }
}

impl ViewState<CounterView> for CounterState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count = ctx.signal(0);
    }

    fn build(&self, _view: &CounterView, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;

        Center::new().child(
            Column::new(column![
                Text::new("You have pushed the button this many times:"),
                SizedBox::height(16.0),
                Text::new(count.get(ctx).to_string()),
                SizedBox::height(16.0),
                RawButton::new(Text::new("Increment"))
                    .on_press(move |cx| count.update(cx, |n| *n += 1)),
            ])
            .main_axis_alignment(MainAxisAlignment::Center),
        )
    }
}

fn main() {
    run_app(CounterApp);
}
