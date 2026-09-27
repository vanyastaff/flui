//! ADR-0086: a closure bound with `let` gets no signature from the dispatch
//! it is later passed to, so its `cx` parameter has no type to infer.
//! `callback(move |cx| ..)` supplies the signature; this is the error without
//! it.

use flui_view::prelude::*;

fn wire(source: &WriterSource, count: Signal<u32>) {
    let press = move |cx| count.set(cx, 1);
    let _ = source.write(press);
}

fn main() {}
