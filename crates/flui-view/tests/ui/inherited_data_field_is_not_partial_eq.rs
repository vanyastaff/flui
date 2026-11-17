//! `InheritedData` compares each field of the old data with the new one: a
//! field whose type has no `PartialEq` is reported at that field.
use flui_view::InheritedData;

#[derive(Clone)]
struct Handle;

#[derive(Clone, InheritedData)]
struct Shared {
    count: u32,
    handle: Handle,
}

fn main() {}
