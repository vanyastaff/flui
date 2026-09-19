//! Painting test harness, compiled for this crate's own tests and for a
//! consumer that enables the `testing` feature.
//!
//! ```
//! use flui_painting::{Paint, testing::record};
//! use flui_types::{Rect, geometry::px, styling::Color};
//!
//! let list = record(|canvas| {
//!     canvas.draw_rect(
//!         Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
//!         &Paint::fill(Color::RED),
//!     );
//! });
//! assert_eq!(list.len(), 1);
//! ```

use crate::{Canvas, DisplayList};

/// Records drawing commands into a fresh [`DisplayList`]: runs `f` against
/// a new [`Canvas`] and finishes it.
pub fn record(f: impl FnOnce(&mut Canvas)) -> DisplayList {
    let mut canvas = Canvas::new();
    f(&mut canvas);
    canvas.finish()
}

#[cfg(test)]
mod tests {
    use flui_foundation::Diagnosticable;
    use flui_types::{Rect, geometry::px, styling::Color};

    use super::record;
    use crate::Paint;

    #[test]
    fn record_captures_commands_and_bounds() {
        let list = record(|canvas| {
            canvas.draw_rect(
                Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)),
                &Paint::fill(Color::RED),
            );
        });
        assert_eq!(list.len(), 1);
        assert_eq!(
            list.bounds(),
            Some(Rect::from_ltrb(px(0.0), px(0.0), px(40.0), px(40.0)))
        );
    }

    #[test]
    fn diagnostics_name_the_list_and_carry_its_properties() {
        let list = record(|canvas| {
            canvas.draw_rect(
                Rect::from_ltrb(px(0.0), px(0.0), px(10.0), px(10.0)),
                &Paint::fill(Color::RED),
            );
        });
        let dump = list.to_diagnostics_node().to_string();
        assert!(dump.contains("DisplayList"), "{dump}");
        assert!(dump.contains("commands"), "{dump}");
    }
}
