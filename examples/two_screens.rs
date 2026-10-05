//! Notes: a three-screen Router application (Home, Note, Settings).
//! Run with `cargo run --example two_screens --features material`.
//! The application tree and its README live in `two_screens/`.
#[path = "two_screens/tree.rs"]
mod tree;

fn main() {
    flui::run_app(tree::NotesApp::default());
}
