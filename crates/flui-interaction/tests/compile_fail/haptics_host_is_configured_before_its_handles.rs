use std::num::NonZeroU32;

use flui_interaction::{GestureBinding, HapticsHost};

fn main() {
    let binding = GestureBinding::new();
    // A builder hands out no handle: configuration ends before one exists.
    let builder = HapticsHost::builder(&binding, || None);
    let _ = builder.handle();
    // A built host has handles, so it can no longer be configured.
    let host = HapticsHost::builder(&binding, || None).build();
    let handle = host.handle();
    let _ = host.deadline_factor(NonZeroU32::MIN);
    drop(handle);
}
