//! Compile-fail test: `BoxDecoration` is `#[non_exhaustive]`, so a downstream
//! crate cannot construct it with a struct literal.
//!
//! This is what lets the type grow a field without breaking callers — the
//! reason `shape` was a source-breaking change before the attribute landed.
//! The functional-update form (`..Default::default()`) is rejected too, which
//! is the part that is easy to assume still works.

use flui_types::geometry::Pixels;
use flui_types::styling::{BoxDecoration, Color};

fn main() {
    // Spelling out every field: rejected outright.
    let _explicit = BoxDecoration::<Pixels> {
        color: Some(Color::RED),
        image: None,
        border: None,
        border_radius: None,
        box_shadow: None,
        gradient: None,
        shape: Default::default(),
    };

    // Functional update from a legally-built value: also rejected.
    let _updated = BoxDecoration::<Pixels> {
        color: Some(Color::RED),
        ..BoxDecoration::new()
    };
}
