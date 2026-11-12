//! ADR-0086: a closure bound with `let` gets no signature from the dispatch
//! it is later passed to, so its `ctx` parameter has no type to infer.
//! `callback(move |ctx| ..)` supplies the signature; this is the error without
//! it.

use flui_view::prelude::*;

fn wire(source: &WriterSource, count: Signal<u32>) {
    let press = move |ctx| count.set(ctx, 1);
    let _ = source.write(press);
}

fn main() {}
