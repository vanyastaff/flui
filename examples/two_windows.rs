//! Two windows — a second window opened beside the first, each with its own
//! UI runtime, both drawn by the application's one raster executor
//! (ADR-0305). Each window counts its own presses, and its other button
//! opens one more window.
//!
//! It is what the runs of the threaded placement use (ADR-0305 §10): on
//! Wayland the raster thread draws every window, and on X11, Windows and
//! macOS the owner thread does until a run there makes them threaded.
//!
//! Run with: cargo run --example two_windows --features unstable-windows

use flui::prelude::*;
use flui::widgets::SafeArea;

#[derive(Clone, StatefulView)]
struct WindowView;

struct WindowState {
    presses: Signal<usize>,
    /// Why the last open failed, shown under the buttons.
    status: Signal<String>,
}

impl StatefulView for WindowView {
    type State = WindowState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        WindowState {
            presses: ctx.signal(0),
            status: ctx.signal(String::new()),
        }
    }
}

impl ViewState<WindowView> for WindowState {
    fn build(&self, _view: &WindowView, ctx: &dyn BuildContext) -> impl IntoView {
        let presses = self.presses;
        let status = self.status;
        SafeArea::new(
            Center::new().child(
                Column::new((
                    Text::new(format!("Pressed {} times", presses.get(ctx))),
                    SizedBox::height(16.0),
                    RawButton::new(Text::new("Press"))
                        .on_pressed(move |ctx| presses.update(ctx, |n| *n += 1)),
                    SizedBox::height(16.0),
                    RawButton::new(Text::new("Open another window"))
                        .on_pressed(move |ctx| status.set(ctx, open_another())),
                    Text::new(status.get(ctx)),
                ))
                .main_axis_alignment(MainAxisAlignment::Center),
            ),
        )
    }
}

/// Open one more window beside this one, and answer why it did not open.
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
fn open_another() -> String {
    let opened = flui::open_window(
        AppConfig::new().title("Another window"),
        WindowPolicy::Isolated,
        WindowView,
    );
    opened
        .err()
        .map_or_else(String::new, |error| error.to_string())
}

/// A phone or a browser tab has one window.
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
fn open_another() -> String {
    String::from("This system has one window.")
}

fn main() -> Result<(), LaunchError> {
    run_app(WindowView)
}
