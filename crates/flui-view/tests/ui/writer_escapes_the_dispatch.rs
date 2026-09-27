//! ADR-0086: the writer an `EventCx` lends cannot outlive the dispatch that
//! lent it.

use flui_view::{Writer, WriterSource};

fn keep(source: &WriterSource) -> &'static mut Writer {
    let mut out = None;
    source.write(|cx| out = Some(&mut **cx));
    out.expect("written")
}

fn main() {}
