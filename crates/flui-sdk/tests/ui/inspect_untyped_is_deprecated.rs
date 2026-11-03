//! Describing a field by its `Debug` text is the deprecated escape hatch: a site outside a derive
//! names why with `#[expect(deprecated, reason = "…")]`.
#![deny(deprecated)]

use flui_sdk::foundation::inspect::{Fields, Inspect, Untyped, Value};

#[derive(Debug)]
struct Opaque;

struct Card {
    opaque: Opaque,
}

impl Inspect for Card {
    fn kind(&self) -> &'static str {
        "Card"
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        out.value("opaque", Value::Debug(Untyped::new(&self.opaque)));
    }
}

fn main() {}
