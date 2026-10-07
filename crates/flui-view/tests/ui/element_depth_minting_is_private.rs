use flui_view::{ElementBase, ElementDepth};

fn restamp(element: &mut dyn ElementBase) {
    element.set_depth(ElementDepth::new(9));
}

fn main() {}
