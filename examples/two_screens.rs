//! Three-screen Notes consumer, extending the original two-screen Router example.
//! Run with `cargo run --example two_screens --features material`.
//! Public headless execution is recorded in the shared example README; native Notes remains unverified.
#[path = "two_screens/tree.rs"]
mod tree;

fn main() {
    flui::run_app(tree::NotesApp::default());
}
