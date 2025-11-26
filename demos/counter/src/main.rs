use flui_app::run_app;
use flui_core::prelude::*;
use flui_view::{IntoElement, Stateful, StatefulView, StatelessView};

fn main() {
    run_app(CounterAppWrapper);
}

// Wrapper to make StatefulView work with run_app
#[derive(Debug, Clone)]
struct CounterAppWrapper;

impl StatelessView for CounterAppWrapper {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        Stateful(CounterApp)
    }
}

#[derive(Debug)]
struct CounterApp;

impl StatefulView for CounterApp {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState { count: 0 }
    }

    fn build(&self, state: &mut Self::State, _ctx: &dyn BuildContext) -> impl IntoElement {
        tracing::info!("Building UI with current count: {}", state.count);

        // TODO: Add Text widget once flui_widgets is refactored
        // For now just return empty element
        Element::empty()
    }
}

#[derive(Debug)]
pub struct CounterState {
    count: i32,
}

impl CounterState {
    pub fn increment(&mut self) {
        self.count += 1;
        tracing::debug!(count = self.count, "Count incremented");
    }
}
