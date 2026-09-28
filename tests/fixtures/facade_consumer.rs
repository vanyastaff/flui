use flui::Diagnosticable;
use flui::animation::{Animatable, TwoWayConverter};
use flui::prelude::*;

#[derive(Clone, StatelessView)]
pub struct Greeting<T: View + Clone> {
    pub child: T,
}

impl<T: View + Clone> StatelessView for Greeting<T> {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }
}

#[derive(Clone, StatefulView)]
pub struct Counter;

pub struct CounterState;

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState
    }
}

impl ViewState<Counter> for CounterState {
    fn build(&self, _view: &Counter, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new("Facade-only stateful widget")
    }
}

#[derive(Clone, Animatable)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Diagnosticable)]
pub struct Details {
    pub count: usize,
}

#[derive(Routable, Clone, PartialEq)]
pub enum AppRoute {
    #[route("/")]
    Home,
    #[route("/note/:id")]
    Note { id: u32 },
}

pub fn exercise_generated_impls() {
    let view = Greeting {
        child: Text::new("Hello"),
    };
    let _element = view.create_element();
    let _stateful_element = Counter.create_element();
    let vector = Position { x: 1.0, y: 2.0 }.to_vector();
    let _position = Position::from_vector(vector);
    let _node = Details { count: 1 }.to_diagnostics_node();
    let path = AppRoute::Note { id: 1 }.to_path();
    let _route = AppRoute::from_path(&path);
    let _home = AppRoute::Home.to_path();
}
