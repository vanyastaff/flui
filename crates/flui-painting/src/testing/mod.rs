//! Painting test harness.
//!
//! Full API reference and examples: `crates/flui-painting/docs/TESTING.md`.
//!
//! Compiled only for this crate's own tests (`cfg(test)`) or when a consumer
//! enables the `testing` feature. Removes the `Canvas::new()` / `finish()`
//! boilerplate from tests and exposes `Diagnosticable`-backed inspection of a
//! recorded [`DisplayList`].
//!
//! # Example
//!
//! ```
//! use flui_painting::testing::{record, command_count};
//! use flui_painting::Paint;
//! use flui_types::{Rect, geometry::px, styling::Color};
//!
//! let list = record(|canvas| {
//!     canvas.draw_rect(
//!         Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
//!         &Paint::fill(Color::RED),
//!     );
//! });
//! assert_eq!(command_count(&list), 1);
//! ```

use flui_foundation::{Diagnosticable, DiagnosticsNode};
use flui_types::Rect;

use crate::{Canvas, DisplayList, DisplayListCore};

/// Records drawing commands into a fresh [`DisplayList`].
///
/// Runs `f` against a new [`Canvas`] and finishes it — the canonical
/// record-now pattern, without the `Canvas::new()` / `finish()` boilerplate.
pub fn record(f: impl FnOnce(&mut Canvas)) -> DisplayList {
    let mut canvas = Canvas::new();
    f(&mut canvas);
    canvas.finish()
}

/// The number of recorded commands.
#[must_use]
pub fn command_count(list: &DisplayList) -> usize {
    list.len()
}

/// The record-time bounds of the display list.
#[must_use]
pub fn bounds(list: &DisplayList) -> Rect {
    list.bounds()
}

/// A diagnostics node describing the display list (command count + bounds).
#[must_use]
pub fn diagnostics(list: &DisplayList) -> DiagnosticsNode {
    list.to_diagnostics_node()
}

/// A printable, indented dump of the display list's diagnostics.
#[must_use]
pub fn dump(list: &DisplayList) -> String {
    diagnostics(list).to_string()
}

#[cfg(test)]
mod tests {
    use flui_types::{Rect, geometry::px, styling::Color};

    use super::{bounds, command_count, diagnostics, record};
    use crate::Paint;

    #[test]
    fn record_captures_commands_and_bounds() {
        let list = record(|canvas| {
            canvas.draw_rect(
                Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
                &Paint::fill(Color::RED),
            );
        });

        assert_eq!(command_count(&list), 1);
        assert_eq!(
            bounds(&list),
            Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0))
        );
    }

    #[test]
    fn diagnostics_dump_names_the_list_and_carries_properties() {
        let list = record(|canvas| {
            canvas.draw_rect(
                Rect::from_ltrb(px(0.0), px(0.0), px(10.0), px(10.0)),
                &Paint::fill(Color::BLUE),
            );
        });

        let node = diagnostics(&list);
        assert_eq!(node.name(), Some("DisplayList"), "names the list");
        assert_eq!(
            node.get_property("commands"),
            Some("1"),
            "carries the command count",
        );
    }

    #[test]
    fn empty_record_has_zero_commands() {
        let list = record(|_canvas| {});
        assert_eq!(command_count(&list), 0);
    }
}
