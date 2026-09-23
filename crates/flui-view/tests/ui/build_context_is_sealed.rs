//! `BuildContext` is sealed: a downstream context cannot forward the untyped
//! field set to a different provider `TypeId`.
struct Mine;

impl flui_view::BuildContext for Mine {}

fn main() {}
