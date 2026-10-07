use flui_view::{ElementBase, ElementDepth, StateCell, StateHandle};

fn restamp(element: &mut dyn ElementBase, depth: ElementDepth) {
    element.set_depth(depth);
}

fn accept_local<T>(_: T) {}

fn main() {
    accept_local(StateCell::new(0_u32));
    accept_local(StateHandle::new(String::new()));
    let _ = restamp;
}
