//! ADR-0086: the writer an `EventContext` lends cannot outlive the dispatch that
//! lent it.

use flui_view::{Writer, WriterSource};

fn keep(source: &WriterSource) -> &'static mut Writer {
    let mut out = None;
    source.write(|ctx| out = Some(&mut **ctx));
    out.expect("written")
}

fn main() {}
