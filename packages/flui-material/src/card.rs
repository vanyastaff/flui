//! [`Card`] — a [`Material`] surface with the M3 elevated-card token
//! defaults: a colored, softly-rounded, lightly-elevated panel wrapped in a
//! fixed margin.
//!
//! # M3 elevated card
//!
//! This is the M3 **elevated** variant only. Its token defaults:
//!
//! | Token | Value |
//! |---|---|
//! | `color` | `ColorScheme.surfaceContainerLow` |
//! | `shadowColor` | `ColorScheme.shadow` |
//! | `surfaceTintColor` | transparent |
//! | `elevation` | `1.0` |
//! | `shape` | rounded rectangle, radius `12.0` |
//! | `clipBehavior` | `Clip::None` |
//! | `margin` | `EdgeInsets::all(4.0)` |
//!
//! `M3 ColorScheme.shadow` is opaque black in both the light and dark
//! baselines (`color_scheme.rs`'s `shadow: Color::from_argb(0xFF00_0000)`),
//! which is exactly [`Material`]'s own built-in shadow color (it has no
//! `shadow_color` setter — see that module's docs) — so no plumbing gap
//! exists there. A transparent surface tint is the same named
//! deferral [`Material`] already carries (no theme-driven surface-tint
//! overlay substrate yet); nothing new to defer here.
//!
//! The M3 card also wraps the child in two `Semantics` nodes (semantic
//! container / explicit child nodes) and threads a border-on-foreground flag
//! (painting the shape's border in front of vs. behind the child) — neither
//! has a home in this substrate yet ([`Material`] paints no border at all,
//! and FLUI's semantics tree has no container merge knob wired to
//! a `StatelessView` this shallow). Both are named, not silently dropped.
//!
//! # Deferred, and named
//!
//! - **`Card::filled` / `Card::outlined`** — the two other M3 variants. Add
//!   them once a caller needs them; the M3 token tables are already known
//!   (`surfaceContainerHighest`/`elevation 0.0` for filled,
//!   `ColorScheme.surface` + an `OutlinedBorder` side in
//!   `ColorScheme.outlineVariant` for outlined) but nothing is wired.
//! - **`borderOnForeground`**, **`semanticContainer`** — see above.
//! - **`shadowColor`/`surfaceTintColor` overrides** — not exposed as builder
//!   methods, because [`Material`] has nowhere to put them yet.

use flui_sdk::geometry::EdgeInsets;
use flui_sdk::geometry::Radius;
use flui_sdk::painting::BorderRadius;
use flui_sdk::painting::Clip;
use flui_sdk::painting::Color;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::Padding;

use crate::material::Material;
use crate::shape::MaterialShape;
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// The M3 elevated card's corner radius.
const DEFAULT_CORNER_RADIUS: f64 = 12.0;
/// The M3 elevated card's elevation.
const DEFAULT_ELEVATION: f64 = 1.0;
/// The M3 elevated card's margin.
const DEFAULT_MARGIN: f64 = 4.0;

/// A Material Design elevated card — a panel with rounded corners and an
/// elevation shadow, wrapped in a fixed outer margin.
///
/// Elevated only; see the module docs for the filled/outlined variants this
/// V1 does not yet ship.
///
/// ```rust
/// use flui_material::Card;
/// use flui_sdk::widgets::Text;
///
/// let _card = Card::new(Text::new("A related panel of content"));
/// ```
#[derive(Clone, StatelessView)]
pub struct Card {
    color: Option<Color>,
    elevation: Option<f64>,
    shape: Option<MaterialShape>,
    clip_behavior: Option<Clip>,
    margin: Option<EdgeInsets>,
    child: BoxedView,
}

impl std::fmt::Debug for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Card")
            .field("color", &self.color)
            .field("elevation", &self.elevation)
            .field("shape", &self.shape)
            .field("clip_behavior", &self.clip_behavior)
            .field("margin", &self.margin)
            .finish_non_exhaustive()
    }
}

impl Card {
    /// A `Card` around `child`, with every visual property falling through
    /// to `_CardDefaultsM3` (see the module docs' token table).
    pub fn new(child: impl IntoView) -> Self {
        Self {
            color: None,
            elevation: None,
            shape: None,
            clip_behavior: None,
            margin: None,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// Overrides the card's [`Material`] fill color.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Overrides the card's elevation. Must be non-negative (the same
    /// contract [`Material::elevation`] enforces on its render object).
    #[must_use]
    pub fn elevation(mut self, elevation: f64) -> Self {
        self.elevation = Some(elevation);
        self
    }

    /// Overrides the card's shape.
    #[must_use]
    pub fn shape(mut self, shape: MaterialShape) -> Self {
        self.shape = Some(shape);
        self
    }

    /// Overrides the card's clip behavior. Defaults to [`Clip::None`].
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = Some(clip_behavior);
        self
    }

    /// Overrides the outer margin. Defaults to `EdgeInsets::all(4.0)`.
    #[must_use]
    pub fn margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = Some(margin);
        self
    }
}

/// [`Card`]'s theme-resolved color/elevation/shape/margin — see
/// [`resolve_style`]'s doc comment for the widget → theme → default cascade.
/// Factored out for the same reason [`crate::app_bar`]'s
/// `ResolvedAppBarStyle` is: directly unit-testable without mounting a
/// widget tree.
struct ResolvedCardStyle {
    color: Color,
    elevation: f64,
    shape: MaterialShape,
    margin: EdgeInsets,
}

/// Resolve `Card`'s M3 defaults through the widget → theme → default
/// cascade, per field: `color`/`elevation`/`shape`/`margin` each fall back
/// through `ThemeData.card_theme`'s own field before the M3 default
/// constant.
fn resolve_style(
    theme: &ThemeData,
    color: Option<Color>,
    elevation: Option<f64>,
    shape: Option<MaterialShape>,
    margin: Option<EdgeInsets>,
) -> ResolvedCardStyle {
    let card_theme = theme.card_theme.as_ref();

    let color = color
        .or_else(|| card_theme.and_then(|t| t.color))
        .unwrap_or(theme.color_scheme.surface_container_low);
    let elevation = elevation
        .or_else(|| card_theme.and_then(|t| t.elevation))
        .unwrap_or(DEFAULT_ELEVATION);
    let shape = shape
        .or_else(|| card_theme.and_then(|t| t.shape))
        .unwrap_or_else(|| {
            MaterialShape::RoundedRect(BorderRadius::all(Radius::circular(DEFAULT_CORNER_RADIUS)))
        });
    let margin = margin
        .or_else(|| card_theme.and_then(|t| t.margin))
        .unwrap_or_else(|| EdgeInsets::all(DEFAULT_MARGIN));

    ResolvedCardStyle {
        color,
        elevation,
        shape,
        margin,
    }
}

impl StatelessView for Card {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let ResolvedCardStyle {
            color,
            elevation,
            shape,
            margin,
        } = resolve_style(&theme, self.color, self.elevation, self.shape, self.margin);

        Padding::new(margin).child(
            Material::new(color)
                .elevation(elevation)
                .shape(shape)
                .clip_behavior(self.clip_behavior.unwrap_or(Clip::None))
                .child(self.child.clone()),
        )
    }
}
