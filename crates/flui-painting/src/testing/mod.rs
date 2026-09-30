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
/// each [`TextContext`] built from it. A consumer's tests use it to pin who
/// holds the collection and when they let go.
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

/// How many collections were built from the process font system's host
/// faces ([`FontCollection::with_host_faces`]) in this process, whether or
/// not the build had the Parley path to feed them into. A composition root's
/// tests use it to show the feed runs once per app, not per realm.
#[must_use]
pub fn host_face_feeds() -> u64 {
    crate::shared_font_system().host_feeds()
}

/// Whether the process font system holds a face mapping each character of
/// `text` that is not whitespace. A test comparing measurement with paint on
/// host faces skips text no host face can draw.
#[must_use]
pub fn host_covers(text: &str) -> bool {
    crate::shared_font_system().covers(text)
}

/// Whether every character of `text` that is not whitespace is mapped by a
/// face both shapers fall back to: one of the sans-serif generic's family,
/// the fallback chain's list for the character's script, or its common list.
/// A test comparing Parley measurement with paint on host faces skips text
/// only cosmic-text's last resort reaches, which the Parley path does not
/// have.
#[cfg(feature = "parley")]
#[must_use]
pub fn host_chain_covers(text: &str) -> bool {
    crate::shared_font_system().chain_covers(text)
}

/// The family the process font system's sans-serif generic names.
#[must_use]
pub fn host_sans_serif_family() -> String {
    crate::shared_font_system().sans_serif_family()
}

/// The family names the process font system's faces carry first, each once:
/// the names a style resolves against on the paint side.
#[must_use]
pub fn host_family_names() -> Vec<String> {
    crate::shared_font_system().family_names()
}

/// Whether `fonts` holds a family named `family`: the check the Parley
/// path's family rule makes against the collection.
#[cfg(feature = "parley")]
#[must_use]
pub fn collection_holds(fonts: &FontCollection, family: &str) -> bool {
    fonts.holds(family)
}

/// Makes `painter` measure on Parley through the context it is given,
/// whatever the build's default, so the Parley measurement runs under
/// `parley` alone (flui-painting `ARCHITECTURE.md`, mapping decision 15).
#[cfg(feature = "parley")]
pub fn measure_with_parley(painter: &mut crate::TextPainter) {
    painter.pin_parley_measurement();
}
