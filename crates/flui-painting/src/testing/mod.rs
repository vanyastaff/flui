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

use crate::{Canvas, DisplayList, FontCollection, HostFonts, TextContext};

/// The FLUI Probe Mono face at weight 100: a generated family no host
/// carries, which maps `A` one em wide. A consumer's tests register it on a
/// collection to tell which collection measured a paragraph; it ships inside
/// this package, so they build from a published archive too.
pub const PROBE_MONO_100: &[u8] = include_bytes!("../../assets/fonts/probe-mono-100.ttf");

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

/// Whether `fonts` was fed from a host scan
/// ([`FontCollection::with_host_fonts`]). A composition root's tests use it
/// to show the collection its realms share is the host-fed one.
#[must_use]
pub fn host_fed(fonts: &FontCollection) -> bool {
    fonts.host_fed()
}

/// Whether some face `host` found maps each character of `text` that is not
/// whitespace. A test comparing measurement with paint on host faces skips
/// text no host face can draw.
#[must_use]
pub fn host_covers(host: &HostFonts, text: &str) -> bool {
    host.covers(text)
}

/// Whether every character of `text` that is not whitespace is mapped by a
/// face a collection fed from `host` falls back to: one of the sans-serif
/// generic's family, the fallback list for the character's script, or the
/// common list. A test comparing measurement with paint on host faces skips
/// text only a face outside those lists covers, which the collection never
/// reaches.
#[must_use]
pub fn host_chain_covers(host: &HostFonts, text: &str) -> bool {
    host.chain_covers(text)
}

/// The family `host`'s sans-serif generic names.
#[must_use]
pub fn host_sans_serif_family(host: &HostFonts) -> String {
    host.sans_serif_family().to_owned()
}

/// The family names `host`'s faces carry first, each once: the families a
/// collection fed from it can hold.
#[must_use]
pub fn host_family_names(host: &HostFonts) -> Vec<String> {
    host.family_names()
}

/// Whether `fonts` holds a family named `family`: the check the Parley
/// path's family rule makes against the collection.
#[must_use]
pub fn collection_holds(fonts: &FontCollection, family: &str) -> bool {
    fonts.holds(family)
}
