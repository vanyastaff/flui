// Without the `unstable-windows` feature the facade has no entry point for a
// second window (ADR-0091 §3–§5 are not built); with it this compiles.
use flui::app::open_window;

fn main() {
    let _ = open_window::<flui::widgets::SizedBox>;
}
