//! ADR-0086 §8 step 3: the reactive graph is not a write target. A write
//! opens an `EventCx` through a `WriterSource`.

use flui_view::{Reactive, SignalWriteExt};

fn main() {
    let graph = Reactive::new();
    let count = graph.signal(0u32);
    let _ = count.set(&graph, 1);
}
