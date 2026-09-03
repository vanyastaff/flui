//! A plain value where parent data is required. The error must state that
//! an empty `impl ParentData for Offset {}` is all that is missing.
use flui_rendering::ParentData;

#[derive(Debug, Clone)]
struct Offset(f32);

fn attach<P: ParentData>(_data: P) {}

fn main() {
    attach(Offset(1.0));
}
