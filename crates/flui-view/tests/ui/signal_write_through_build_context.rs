//! ADR-0086: `build` has no write target. The context it receives cannot
//! stand in for the `&mut EventCx` a signal write takes.

use flui_view::prelude::*;

fn build_body(ctx: &dyn BuildContext, count: Signal<u32>) {
    let _ = count.set(ctx, 1);
}

fn main() {}
