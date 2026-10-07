use std::num::NonZeroU64;

use flui_interaction::arena::{PointerSignalResolver, SignalPriority};
use flui_interaction::{FocusNode, PointerId};

fn main() {
    let first = FocusNode::new();
    let second = FocusNode::new();
    assert_ne!(first.id(), second.id());
    let focus_raw: NonZeroU64 = first.id().into();
    assert_eq!(focus_raw.get(), first.id().get());

    let resolver = PointerSignalResolver::new();
    let pointer = PointerId::try_from(1_u64).expect("authored contact identity");
    let handler = resolver.register(pointer, SignalPriority::Normal, |_| {});
    let handler_raw: NonZeroU64 = handler.into();
    assert_eq!(handler_raw.get(), handler.get());
    resolver.unregister(pointer, handler);
}
