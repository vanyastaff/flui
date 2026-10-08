use std::num::NonZeroU64;

use flui_interaction::FocusNode;

fn main() {
    let first = FocusNode::new();
    let second = FocusNode::new();
    assert_ne!(first.id(), second.id());
    let focus_raw: NonZeroU64 = first.id().into();
    assert_eq!(focus_raw.get(), first.id().get());
}
