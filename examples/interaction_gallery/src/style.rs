//! The gallery's look: one light palette and the few text styles and
//! surfaces every screen composes. Theme-free widgets take their colours
//! and type from here rather than from a design system.

use flui::painting::styling::{BorderRadius, BorderRadiusExt, BoxDecoration};
use flui::painting::typography::{FontWeight, TextStyle};
use flui::prelude::*;

/// The page behind every screen.
pub(crate) const BACKGROUND: Color = Color::rgb(244, 245, 248);
/// Cards, rows and the sidebar.
pub(crate) const SURFACE: Color = Color::rgb(255, 255, 255);
/// The selected sidebar entry and primary buttons.
pub(crate) const ACCENT: Color = Color::rgb(46, 92, 230);
/// A tinted surface behind accented content.
pub(crate) const ACCENT_SOFT: Color = Color::rgb(222, 230, 252);
/// Body text.
pub(crate) const INK: Color = Color::rgb(28, 31, 38);
/// Secondary text.
pub(crate) const MUTED: Color = Color::rgb(96, 103, 117);
/// Hairlines between rows.
pub(crate) const LINE: Color = Color::rgb(224, 227, 233);
/// Behind a revealed pin action.
pub(crate) const PIN: Color = Color::rgb(232, 160, 32);
/// Behind an archive action.
pub(crate) const ARCHIVE: Color = Color::rgb(56, 142, 92);

/// The default text style every screen inherits.
pub(crate) fn body_style() -> TextStyle {
    TextStyle::new().with_color(INK).with_font_size(15.0)
}

/// A screen's title.
pub(crate) fn title(text: impl Into<String>) -> Semantics {
    Semantics::new().heading_level(HeadingLevel::H1).child(
        Text::new(text).style(
            TextStyle::new()
                .with_color(INK)
                .with_font_size(24.0)
                .with_font_weight(FontWeight::BOLD),
        ),
    )
}

/// Body text in the secondary colour.
pub(crate) fn caption(text: impl Into<String>) -> Text {
    Text::new(text).style(TextStyle::new().with_color(MUTED).with_font_size(13.0))
}

/// Text in `color`, at the body size.
pub(crate) fn tinted(text: impl Into<String>, color: Color) -> Text {
    Text::new(text).style(TextStyle::new().with_color(color).with_font_size(15.0))
}

/// Bold text at the body size.
pub(crate) fn strong(text: impl Into<String>) -> Text {
    Text::new(text).style(
        TextStyle::new()
            .with_color(INK)
            .with_font_size(15.0)
            .with_font_weight(FontWeight::BOLD),
    )
}

/// A rounded surface in `color`.
pub(crate) fn rounded(color: Color, radius: f64) -> BoxDecoration<f64> {
    BoxDecoration::default()
        .set_color(Some(color))
        .set_border_radius(Some(BorderRadius::circular(radius)))
}

/// A white card with rounded corners around `child`.
pub(crate) fn card(child: impl IntoView) -> Container {
    Container::new()
        .decoration(rounded(SURFACE, 12.0))
        .padding(EdgeInsets::all(16.0))
        .child(child)
}

/// The readout line each screen keeps current: what the last interaction
/// did, in words a person and an automation client can both read.
pub(crate) fn readout(text: impl Into<String>) -> Container {
    Container::new()
        .decoration(rounded(ACCENT_SOFT, 8.0))
        .padding(EdgeInsets::symmetric(8.0, 12.0))
        .child(Text::new(text).style(TextStyle::new().with_color(ACCENT).with_font_size(14.0)))
}

/// A theme-free button with a filled, rounded look.
pub(crate) fn button(label: impl Into<String>) -> Container {
    Container::new()
        .decoration(rounded(ACCENT, 8.0))
        .padding(EdgeInsets::symmetric(10.0, 16.0))
        .child(tinted(label, SURFACE))
}

/// A theme-free button with an outlined, quieter look.
pub(crate) fn quiet_button(label: impl Into<String>) -> Container {
    Container::new()
        .decoration(rounded(ACCENT_SOFT, 8.0))
        .padding(EdgeInsets::symmetric(10.0, 16.0))
        .child(tinted(label, ACCENT))
}
