//! Notes with a fault injected by mode, for the native teardown checks.
//! Run with `cargo run --example notes_faults --features material,persist -- <mode>`.
//!
//! With no mode it runs Notes unchanged. It knows no fault mode yet, so any
//! mode given exits with code 2 before opening a window.
#[path = "two_screens/tree.rs"]
mod tree;

fn main() {
    if let Some(mode) = std::env::args().nth(1) {
        eprintln!("notes_faults: unknown mode `{mode}`");
        std::process::exit(2);
    }
    flui::run_app(tree::NotesApp::default());
}
