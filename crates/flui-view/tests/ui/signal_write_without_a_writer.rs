//! ADR-0086: a signal write names its write target; there is no
//! context-free `set`.

use flui_view::prelude::*;

fn build_body(count: Signal<u32>) {
    let _ = count.set(1);
}

fn main() {}
