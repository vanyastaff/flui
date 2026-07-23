//! [`Divider`]/[`VerticalDivider`] — a thin, inset rule painted with the M3
//! `_DividerDefaultsM3` token defaults.
//!
//! # Flutter parity
//!
//! `material/divider.dart`'s `Divider`/`VerticalDivider`, composed with
//! `_DividerDefaultsM3` (oracle tag `3.44.0`):
//!
//! | Token | Value | Oracle |
//! |---|---|---|
//! | `space` (height/width) | `16.0` | `_DividerDefaultsM3` constructor |
//! | `thickness` | `1.0` | `_DividerDefaultsM3` constructor |
//! | `indent` | `0.0` | `_DividerDefaultsM3` constructor |
//! | `endIndent` | `0.0` | `_DividerDefaultsM3` constructor |
//! | `color` | `ColorScheme.outlineVariant` | `_DividerDefaultsM3.color` |
//!
//! Note the M3 `thickness` default is `1.0`, not M2's `0.0` — a divider with
//! no theme/widget override still paints a visible 1dp line.
//!
//! # Composition: a filled rectangle, not a bottom border
//!
//! The oracle paints the line as a `Container(height: thickness)` whose
//! `BoxDecoration.border` is a single `bottom BorderSide` of width
//! `thickness` — since the border's width equals the container's own height,
//! that border covers the container's entire area, which is paint-equivalent
//! to filling the container with `color` outright. This substrate does
//! exactly that: [`Container::decoration`](flui_widgets::Container::decoration)
//! with `BoxDecoration::with_color`, which also lets [`Divider::radius`]/
//! [`DividerThemeData::radius`](crate::theme_data::DividerThemeData::radius)
//! round the filled rect's corners directly, without inventing a
//! border-inset reduction FLUI's `BoxDecoration` doesn't model (see
//! [`crate::material::Material`]'s docs on the same class of
//! simplification).
//!
//! # Deferred, and named
//!
//! - **`Divider::createBorderSide`** — a `BuildContext`-optional static
//!   helper for composing a manual divider border; no caller needs it yet.
//! - **`ListTile::divideTiles`** — the `Iterable<Widget>` inter-tile divider
//!   helper; a natural, additive follow-up once a caller list-builds
//!   `ListTile`s.
//! - **`PopupMenuDivider`** — a distinct oracle type, out of this scope.

use flui_types::EdgeInsets;
use flui_types::geometry::px;
use flui_types::styling::{BorderRadius, BoxDecoration, Color};
use flui_view::prelude::*;
use flui_widgets::{Center, Container, SizedBox};

use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// `_DividerDefaultsM3`'s height/width (`divider.dart`, oracle tag `3.44.0`).
const DEFAULT_SPACE: f32 = 16.0;
/// `_DividerDefaultsM3`'s thickness (`divider.dart`, oracle tag `3.44.0`).
const DEFAULT_THICKNESS: f32 = 1.0;
/// `_DividerDefaultsM3`'s indent (`divider.dart`, oracle tag `3.44.0`).
const DEFAULT_INDENT: f32 = 0.0;
/// `_DividerDefaultsM3`'s end indent (`divider.dart`, oracle tag `3.44.0`).
const DEFAULT_END_INDENT: f32 = 0.0;

/// A thin horizontal rule, with padding on either side.
///
/// See the module docs for the M3 default token table and how the line is
/// painted.
///
/// ```rust
/// use flui_material::Divider;
///
/// let _divider = Divider::new().indent(16.0);
/// ```
#[derive(Clone, Debug, Default, StatelessView)]
pub struct Divider {
    height: Option<f32>,
    thickness: Option<f32>,
    indent: Option<f32>,
    end_indent: Option<f32>,
    color: Option<Color>,
    radius: Option<BorderRadius>,
}

impl Divider {
    /// A `Divider` with every property falling through to
    /// `_DividerDefaultsM3` (see the module docs' token table).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Overrides the divider's total height (the line is vertically centered
    /// within it). Defaults to `16.0`.
    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Overrides the line's thickness. Defaults to `1.0`.
    #[must_use]
    pub fn thickness(mut self, thickness: f32) -> Self {
        self.thickness = Some(thickness);
        self
    }

    /// Overrides the empty space before the line's leading edge. Defaults to
    /// `0.0`.
    #[must_use]
    pub fn indent(mut self, indent: f32) -> Self {
        self.indent = Some(indent);
        self
    }

    /// Overrides the empty space after the line's trailing edge. Defaults to
    /// `0.0`.
    #[must_use]
    pub fn end_indent(mut self, end_indent: f32) -> Self {
        self.end_indent = Some(end_indent);
        self
    }

    /// Overrides the line's color. Defaults to `ColorScheme.outlineVariant`.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Overrides the line's corner radius. Unset paints square corners.
    #[must_use]
    pub fn radius(mut self, radius: BorderRadius) -> Self {
        self.radius = Some(radius);
        self
    }
}

impl StatelessView for Divider {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let resolved = resolve_style(
            &theme,
            self.height,
            self.thickness,
            self.indent,
            self.end_indent,
            self.color,
            self.radius,
        );

        SizedBox::height(resolved.space).child(
            Center::new().child(
                Container::new()
                    .height(resolved.thickness)
                    .margin(EdgeInsets::new(
                        px(0.0),
                        px(resolved.end_indent),
                        px(0.0),
                        px(resolved.indent),
                    ))
                    .decoration(decoration(resolved.color, resolved.radius)),
            ),
        )
    }
}

/// A thin vertical rule, with padding on either side. The vertical analog of
/// [`Divider`] — see the module docs for the shared M3 token table.
///
/// ```rust
/// use flui_material::VerticalDivider;
///
/// let _divider = VerticalDivider::new().indent(8.0);
/// ```
#[derive(Clone, Debug, Default, StatelessView)]
pub struct VerticalDivider {
    width: Option<f32>,
    thickness: Option<f32>,
    indent: Option<f32>,
    end_indent: Option<f32>,
    color: Option<Color>,
    radius: Option<BorderRadius>,
}

impl VerticalDivider {
    /// A `VerticalDivider` with every property falling through to
    /// `_DividerDefaultsM3` (see the module docs' token table).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Overrides the divider's total width (the line is horizontally
    /// centered within it). Defaults to `16.0`.
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Overrides the line's thickness. Defaults to `1.0`.
    #[must_use]
    pub fn thickness(mut self, thickness: f32) -> Self {
        self.thickness = Some(thickness);
        self
    }

    /// Overrides the empty space above the line's top edge. Defaults to
    /// `0.0`.
    #[must_use]
    pub fn indent(mut self, indent: f32) -> Self {
        self.indent = Some(indent);
        self
    }

    /// Overrides the empty space below the line's bottom edge. Defaults to
    /// `0.0`.
    #[must_use]
    pub fn end_indent(mut self, end_indent: f32) -> Self {
        self.end_indent = Some(end_indent);
        self
    }

    /// Overrides the line's color. Defaults to `ColorScheme.outlineVariant`.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Overrides the line's corner radius. Unset paints square corners.
    #[must_use]
    pub fn radius(mut self, radius: BorderRadius) -> Self {
        self.radius = Some(radius);
        self
    }
}

impl StatelessView for VerticalDivider {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let resolved = resolve_style(
            &theme,
            self.width,
            self.thickness,
            self.indent,
            self.end_indent,
            self.color,
            self.radius,
        );

        SizedBox::width(resolved.space).child(
            Center::new().child(
                Container::new()
                    .width(resolved.thickness)
                    .margin(EdgeInsets::new(
                        px(resolved.indent),
                        px(0.0),
                        px(resolved.end_indent),
                        px(0.0),
                    ))
                    .decoration(decoration(resolved.color, resolved.radius)),
            ),
        )
    }
}

/// [`Divider`]/[`VerticalDivider`]'s theme-resolved geometry/color — see
/// [`resolve_style`]'s doc comment for the widget → theme → default cascade.
struct ResolvedDividerStyle {
    space: f32,
    thickness: f32,
    indent: f32,
    end_indent: f32,
    color: Color,
    /// Unlike every other field, `radius` has no concrete M3 default to fall
    /// through to — square corners (`None`) are Flutter's own fallback too
    /// (`BoxDecoration`'s default), not a value this module invents.
    radius: Option<BorderRadius>,
}

/// Resolve the M3 divider defaults through the widget → theme → default
/// cascade, per field. Flutter parity: `this.height ?? dividerTheme.space ??
/// defaults.space!` (and the `thickness`/`indent`/`endIndent`/`color`
/// equivalents), `divider.dart`, oracle tag `3.44.0`.
#[allow(clippy::too_many_arguments)] // mirrors the oracle's own per-field cascade; a patch struct would only relocate this
fn resolve_style(
    theme: &ThemeData,
    space: Option<f32>,
    thickness: Option<f32>,
    indent: Option<f32>,
    end_indent: Option<f32>,
    color: Option<Color>,
    radius: Option<BorderRadius>,
) -> ResolvedDividerStyle {
    let divider_theme = theme.divider_theme.as_ref();

    ResolvedDividerStyle {
        space: space
            .or_else(|| divider_theme.and_then(|t| t.space))
            .unwrap_or(DEFAULT_SPACE),
        thickness: thickness
            .or_else(|| divider_theme.and_then(|t| t.thickness))
            .unwrap_or(DEFAULT_THICKNESS),
        indent: indent
            .or_else(|| divider_theme.and_then(|t| t.indent))
            .unwrap_or(DEFAULT_INDENT),
        end_indent: end_indent
            .or_else(|| divider_theme.and_then(|t| t.end_indent))
            .unwrap_or(DEFAULT_END_INDENT),
        color: color
            .or_else(|| divider_theme.and_then(|t| t.color))
            .unwrap_or(theme.color_scheme.outline_variant),
        radius: radius.or_else(|| divider_theme.and_then(|t| t.radius)),
    }
}

/// The line's fill: `color`, optionally rounded to `radius` — see the module
/// docs' "Composition" section for why a filled rect stands in for the
/// oracle's full-height bottom border.
fn decoration(color: Color, radius: Option<BorderRadius>) -> BoxDecoration<flui_types::Pixels> {
    BoxDecoration::with_color(color).set_border_radius(radius)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_leaves_every_override_unset() {
        let divider = Divider::new();
        assert!(divider.height.is_none());
        assert!(divider.thickness.is_none());
        assert!(divider.indent.is_none());
        assert!(divider.end_indent.is_none());
        assert!(divider.color.is_none());
        assert!(divider.radius.is_none());
    }

    #[test]
    fn overrides_are_stored_verbatim() {
        let divider = Divider::new()
            .height(24.0)
            .thickness(2.0)
            .indent(8.0)
            .end_indent(12.0)
            .color(Color::rgb(1, 2, 3));

        assert_eq!(divider.height, Some(24.0));
        assert_eq!(divider.thickness, Some(2.0));
        assert_eq!(divider.indent, Some(8.0));
        assert_eq!(divider.end_indent, Some(12.0));
        assert_eq!(divider.color, Some(Color::rgb(1, 2, 3)));
    }

    /// `_DividerDefaultsM3`'s literal token table (`divider.dart`, oracle
    /// tag `3.44.0`). Pins this module's own constants against the oracle's
    /// literals — the M3 `thickness` default is `1.0`, not M2's `0.0`.
    #[test]
    fn default_constants_match_the_oracle() {
        assert_eq!(DEFAULT_SPACE, 16.0);
        assert_eq!(DEFAULT_THICKNESS, 1.0);
        assert_eq!(DEFAULT_INDENT, 0.0);
        assert_eq!(DEFAULT_END_INDENT, 0.0);
    }

    #[test]
    fn resolve_style_defaults_to_the_m3_token_table() {
        let theme = ThemeData::light();
        let resolved = resolve_style(&theme, None, None, None, None, None, None);

        assert_eq!(resolved.space, DEFAULT_SPACE);
        assert_eq!(resolved.thickness, DEFAULT_THICKNESS);
        assert_eq!(resolved.indent, DEFAULT_INDENT);
        assert_eq!(resolved.end_indent, DEFAULT_END_INDENT);
        assert_eq!(resolved.color, theme.color_scheme.outline_variant);
        assert!(resolved.radius.is_none());
    }

    #[test]
    fn resolve_style_falls_through_to_the_divider_theme_when_no_widget_override_is_set() {
        let mut theme = ThemeData::light();
        let themed_color = Color::rgb(4, 5, 6);
        theme.divider_theme = Some(crate::theme_data::DividerThemeData {
            color: Some(themed_color),
            thickness: Some(3.0),
            ..Default::default()
        });

        let resolved = resolve_style(&theme, None, None, None, None, None, None);

        assert_eq!(resolved.color, themed_color);
        assert_eq!(resolved.thickness, 3.0);
        // `space`/`indent`/`end_indent` were left unset on the theme slot —
        // each falls through to its own M3 default independently.
        assert_eq!(resolved.space, DEFAULT_SPACE);
        assert_eq!(resolved.indent, DEFAULT_INDENT);
    }

    #[test]
    fn resolve_style_widget_override_wins_over_the_divider_theme() {
        let mut theme = ThemeData::light();
        theme.divider_theme = Some(crate::theme_data::DividerThemeData {
            color: Some(Color::rgb(1, 1, 1)),
            ..Default::default()
        });
        let widget_color = Color::rgb(9, 9, 9);

        let resolved = resolve_style(&theme, None, None, None, None, Some(widget_color), None);

        assert_eq!(resolved.color, widget_color);
    }

    #[test]
    fn vertical_divider_new_leaves_every_override_unset() {
        let divider = VerticalDivider::new();
        assert!(divider.width.is_none());
        assert!(divider.thickness.is_none());
        assert!(divider.indent.is_none());
        assert!(divider.end_indent.is_none());
        assert!(divider.color.is_none());
    }
}
