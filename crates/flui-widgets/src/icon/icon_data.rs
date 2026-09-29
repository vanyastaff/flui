//! [`IconData`] — describes a single glyph from an icon font.
//!
//! Flutter parity: `widgets/icon_data.dart` `IconData`.

use std::fmt;

/// Describes a glyph in an icon font, ready to be painted by
/// [`Icon`](crate::Icon).
///
/// A value type: it identifies a codepoint plus enough font metadata (family,
/// package, fallback families, and whether the glyph should mirror under
/// right-to-left text) to select and shape that glyph. `IconData` alone does
/// not draw anything.
///
/// Flutter parity: `widgets/icon_data.dart` `IconData`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IconData {
    /// The Unicode codepoint at which this icon is stored in the icon font.
    pub code_point: u32,

    /// The font family to resolve `code_point` against. `None` defers to
    /// whatever family the ambient [`TextStyle`](flui_painting::typography::TextStyle)
    /// resolves to.
    pub font_family: Option<String>,

    /// The package that bundles `font_family`, used to select a package font
    /// asset rather than an app-level one.
    pub font_package: Option<String>,

    /// Whether [`Icon`](crate::Icon) should mirror this glyph horizontally
    /// under a right-to-left reading direction (e.g. a "back" arrow).
    ///
    /// **Deferred:** `Icon::build` does not yet apply this flip — it needs a
    /// `Transform` composition step not yet wired into the icon build path.
    /// See the [`Icon`](crate::Icon) docs.
    pub match_text_direction: bool,

    /// Additional font families to try, in order, when `font_family` doesn't
    /// cover `code_point`.
    pub font_family_fallback: Vec<String>,
}

impl IconData {
    /// A codepoint with no font family (resolved via the ambient default),
    /// no package, a fixed (non-mirroring) orientation, and no fallback
    /// families.
    #[must_use]
    pub const fn new(code_point: u32) -> Self {
        Self {
            code_point,
            font_family: None,
            font_package: None,
            match_text_direction: false,
            font_family_fallback: Vec::new(),
        }
    }

    /// Set the font family to resolve `code_point` against.
    #[must_use]
    pub fn with_font_family(mut self, font_family: impl Into<String>) -> Self {
        self.font_family = Some(font_family.into());
        self
    }

    /// The codepoint as a one-character [`String`], ready to hand to a
    /// [`TextSpan`](flui_painting::typography::TextSpan).
    ///
    /// Returns `None` when `code_point` is not a valid Unicode scalar value
    /// (a lone UTF-16 surrogate) — private-use icon-font codepoints are
    /// always valid scalars in practice, but [`Icon::build`](crate::Icon)
    /// treats this the same as no icon rather than panicking.
    #[must_use]
    pub fn code_point_string(&self) -> Option<String> {
        char::from_u32(self.code_point).map(String::from)
    }
}

impl fmt::Display for IconData {
    /// Renders as `U+{codepoint:05X}`, e.g. `U+E87D`.
    ///
    /// Divergence: Flutter's `IconData.toString()` wraps this in
    /// `"IconData(...)"` (icon_data.dart:116); FLUI renders the bare
    /// codepoint so it composes cleanly into diagnostics without a redundant
    /// type-name prefix.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "U+{:05X}", self.code_point)
    }
}
