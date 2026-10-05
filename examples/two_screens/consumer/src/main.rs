// Same application source as the repository example; no implementation crates.
#[path = "../../tree.rs"]
mod tree;

#[cfg(test)]
#[path = "../../../../tests/fixtures/notes_flow.rs"]
mod flow;

fn main() {
    flui::run_app(tree::NotesApp::default());
}
