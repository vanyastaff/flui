//! SC-014 — `column!` with 17 children fails to compile with the
//! FR-034 friendly diagnostic substring.
//!
//! Driven by `crates/flui-view/tests/trybuild_ui.rs` via trybuild
//! `compile_fail`. The expected rustc output is captured in the
//! sibling `.stderr` file; trybuild's match is `contains` so any
//! drift in the surrounding rustc framing (line numbers, file
//! paths) does not regress the test as long as the FR-034
//! diagnostic substring stays intact.

use flui_view::column;

#[derive(Clone)]
struct Leaf;

impl flui_view::view::View for Leaf {
    fn create_element(&self) -> Box<dyn flui_view::view::ElementBase> {
        use flui_view::element::StatelessBehavior;
        use flui_view::view::StatelessElement;
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }
}

impl flui_view::view::StatelessView for Leaf {
    fn build(&self, _ctx: &dyn flui_view::context::BuildContext) -> impl flui_view::view::IntoView {
        Leaf
    }
}

fn main() {
    let _ = column![
        Leaf, Leaf, Leaf, Leaf, Leaf, Leaf, Leaf, Leaf, Leaf, Leaf, Leaf, Leaf, Leaf, Leaf, Leaf,
        Leaf, // 17th — exceeds the FR-013 cap of 16
        Leaf,
    ];
}
