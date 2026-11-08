//! Counter — the minimal FLUI "hello world": one piece of state and a button
//! that updates it. This example mounts the exact `CounterApp` source that
//! `flui create --template counter` generates.
//!
//! The count is a `Signal`: created in `init_state`, read in `build` (which
//! rebuilds the view when it changes), and written by the button's press,
//! whose callback receives the `cx` a write needs.
//!
//! Run with: cargo run --example counter

include!("../crates/flui-cli/src/templates/counter_app.rs");

fn main() -> Result<(), LaunchError> {
    run_app(CounterApp, AppLaunch::new())
}
