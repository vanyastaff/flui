//! A package that names only `flui-sdk` derives `Inspect` and reads the description back. A
//! field opted into the untyped escape hatch with `#[inspect(debug)]` compiles under
//! `deny(deprecated)`: the attribute is the field's written exception.
#![deny(deprecated)]

use flui_sdk::foundation::inspect::{Field, Fields, Inspect, Meta, Seq, Sink};
use flui_sdk::geometry::EdgeInsets;

#[derive(Inspect)]
enum Tone {
    Plain,
    Raised,
}

#[derive(Debug)]
struct Opaque;

#[derive(Inspect)]
struct Badge<T> {
    #[inspect(unit = logical_px)]
    padding: EdgeInsets,
    #[inspect(default = Tone::Plain)]
    tone: Tone,
    count: T,
    #[inspect(debug)]
    opaque: Opaque,
}

struct Names(Vec<&'static str>);

impl Sink for Names {
    fn field(&mut self, field: Field<'_>) {
        self.0.push(field.name);
    }

    fn seq(&mut self, name: &'static str, _: Meta<'_>, _: &dyn Seq) {
        self.0.push(name);
    }
}

fn main() {
    let badge = Badge {
        padding: EdgeInsets::all(4.0),
        tone: Tone::Raised,
        count: 3_u8,
        opaque: Opaque,
    };
    let mut names = Names(Vec::new());
    badge.inspect(&mut Fields::new(&mut names));
    assert_eq!(badge.kind(), "Badge");
    assert_eq!(names.0, ["padding", "tone", "count", "opaque"]);
}
