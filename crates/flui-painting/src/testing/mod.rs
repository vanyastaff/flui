//! Painting test harness, compiled for this crate's own tests and for a
//! consumer that enables the `testing` feature.
//!
//! ```
//! use flui_painting::{Paint, testing::record};
//! use flui_foundation::geometry::Rect;
//! use flui_painting::styling::Color;
//!
//! let list = record(|canvas| {
//!     canvas.draw_rect(
//!         Rect::from_ltrb(0.0, 0.0, 40.0, 40.0),
//!         &Paint::fill(Color::RED),
//!     );
//! });
//! assert_eq!(list.len(), 1);
//! ```

use crate::{Canvas, DisplayList, FontCollection, TextContext};

/// Records drawing commands into a fresh [`DisplayList`]: runs `f` against
/// a new [`Canvas`] and finishes it.
pub fn record(f: impl FnOnce(&mut Canvas)) -> DisplayList {
    let mut canvas = Canvas::new();
    f(&mut canvas);
    canvas.finish()
}

/// How many handles hold `fonts`: the caller's own clones plus one inside
/// each [`TextContext`](crate::TextContext) built from it. A consumer's
/// tests use it to pin who holds the collection and when they let go.
#[must_use]
pub fn font_collection_holders(fonts: &FontCollection) -> usize {
    fonts.holders()
}

/// How many measurements `text` was lent for: one per layout, intrinsic or
/// dry query a [`TextPainter`](crate::TextPainter) made through it. A
/// consumer's tests use it to show which realm's context a layout measured
/// on.
#[must_use]
pub fn text_context_lends(text: &TextContext) -> u64 {
    text.lends()
}

/// Makes `painter` measure on Parley through the context it is given,
/// whatever the build's default, so the Parley measurement runs under
/// `parley` alone (flui-painting `ARCHITECTURE.md`, mapping decision 15).
#[cfg(feature = "parley")]
pub fn measure_with_parley(painter: &mut crate::TextPainter) {
    painter.pin_parley_measurement();
}

#[cfg(test)]
mod tests {
    use crate::styling::Color;
    use flui_foundation::Diagnosticable;
    use flui_foundation::geometry::Rect;

    use super::record;
    use crate::Paint;

    #[test]
    fn record_captures_commands_and_bounds() {
        let list = record(|canvas| {
            canvas.draw_rect(
                Rect::from_ltrb(0.0, 0.0, 40.0, 40.0),
                &Paint::fill(Color::RED),
            );
        });
        assert_eq!(list.len(), 1);
        assert_eq!(list.bounds(), Some(Rect::from_ltrb(0.0, 0.0, 40.0, 40.0)));
    }

    #[test]
    fn diagnostics_name_the_list_and_carry_its_properties() {
        let list = record(|canvas| {
            canvas.draw_rect(
                Rect::from_ltrb(0.0, 0.0, 10.0, 10.0),
                &Paint::fill(Color::RED),
            );
        });
        let dump = list.to_diagnostics_node().to_string();
        assert!(dump.contains("DisplayList"), "{dump}");
        assert!(dump.contains("commands"), "{dump}");
    }
}
