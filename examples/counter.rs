//! Counter — the minimal FLUI "hello world": one piece of state and a button
//! that updates it. This is the exact `CounterView`/`CounterState` shape
//! `flui create` generates for the `counter` template
//! (`crates/flui-cli/src/templates/counter.rs`'s `LIB` constant) — kept in
//! sync deliberately, so this example and the generated project never drift
//! apart.
//!
//! The count is a `Signal`: created in `create_state`, read in `build` (which
//! rebuilds this view when it changes), and written by the button's press,
//! whose callback receives the `ctx` a write needs. `build` has no `ctx` to
//! write with.
//!
//! Run with: cargo run --example counter

use flui::prelude::*;
use flui::widgets::SafeArea;

#[derive(Clone, StatefulView)]
struct CounterView;

struct CounterState {
    count: Signal<usize>,
}

impl StatefulView for CounterView {
    type State = CounterState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        CounterState {
            count: ctx.signal(0),
        }
    }
}

impl ViewState<CounterView> for CounterState {
    fn build(&self, _view: &CounterView, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;

        SafeArea::new(
            Center::new().child(
                Column::new((
                    Text::new("You have pushed the button this many times:"),
                    SizedBox::height(16.0),
                    Text::new(count.get(ctx).to_string()),
                    SizedBox::height(16.0),
                    RawButton::new(Text::new("Increment"))
                        .on_pressed(move |ctx| count.update(ctx, |n| *n += 1)),
                ))
                .main_axis_alignment(MainAxisAlignment::Center),
            ),
        )
    }
}

fn main() -> Result<(), LaunchError> {
    run_app(CounterView, AppLaunch::new())
}
