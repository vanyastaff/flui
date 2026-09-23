//! `FieldMask::erase` is crate-private: application code cannot lower a typed
//! mask to the untyped `FieldSet` and hand it to another provider.
use flui_view::FieldMask;

struct Theme;

fn main() {
    let _untyped = FieldMask::<Theme>::bit(0).erase();
}
