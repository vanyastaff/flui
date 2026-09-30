//! The counter, served to an agent over the development endpoint.
//!
//! The same view as the workspace's `counter` example, run with an
//! [`AgentServer`] installed on the application's configuration. The server
//! reads its endpoint and token from `FLUI_AGENT_ENDPOINT` and
//! `FLUI_AGENT_TOKEN` (the tool that launches the app sets them) and stays
//! inert, with a warning, without them:
//!
//! ```text
//! # Linux and macOS: a socket path in a directory of its own with mode 0700.
//! mkdir -m 700 /tmp/flui-agent-demo
//! FLUI_AGENT_ENDPOINT=/tmp/flui-agent-demo/agent.sock \
//! FLUI_AGENT_TOKEN=0123456789abcdef0123456789abcdef \
//!   cargo run -p flui-devtools --example agent_counter --features agent
//!
//! # Windows (PowerShell): a pipe name.
//! $env:FLUI_AGENT_ENDPOINT = 'flui-agent-demo'
//! $env:FLUI_AGENT_TOKEN = '0123456789abcdef0123456789abcdef'
//! cargo run -p flui-devtools --example agent_counter --features agent
//! ```
//!
//! A client then sends newline-delimited JSON: the hello, then `windows`,
//! `read` and `act` requests (see `flui_devtools::agent`).

use flui_app::{AppConfig, run_app_with_config};
use flui_devtools::agent::AgentServer;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{Signal, StatefulView, StatelessView, ViewState};
use flui_sdk::widgets::{Center, Column, RawButton, SizedBox, Text, column};

#[derive(Clone, StatelessView)]
struct CounterApp;

impl StatelessView for CounterApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        CounterView
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
        Center::new().child(Column::new(column![
            Text::new(format!("Count: {}", count.get(ctx))),
            SizedBox::height(16.0),
            RawButton::new(Text::new("Increment"))
                .on_press(move |cx| count.update(cx, |n| *n += 1)),
        ]))
    }
}

fn main() {
    let config = AppConfig::new()
        .with_title("Agent counter")
        .with_dev_agent(AgentServer::from_env());
    run_app_with_config(CounterApp, config);
}
