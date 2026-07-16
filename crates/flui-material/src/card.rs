//! [`Card`] — a [`Material`] surface with the M3 elevated-card token
//! defaults: a colored, softly-rounded, lightly-elevated panel wrapped in a
//! fixed margin.
//!
//! # Flutter parity
//!
//! `material/card.dart`'s `Card` (oracle tag `3.44.0`), `build` (`:243-262`)
//! composed with `_CardDefaultsM3` (`:301-318`) — the M3 **elevated** variant
//! only (Flutter's un-named default constructor; `_CardVariant.elevated`).
//! `_CardDefaultsM3` sets:
//!
//! | Token | Value | Oracle |
//! |---|---|---|
//! | `color` | `ColorScheme.surfaceContainerLow` | `_CardDefaultsM3.color` |
//! | `shadowColor` | `ColorScheme.shadow` | `_CardDefaultsM3.shadowColor` |
//! | `surfaceTintColor` | `Colors.transparent` | `_CardDefaultsM3.surfaceTintColor` |
//! | `elevation` | `1.0` | `_CardDefaultsM3` constructor |
//! | `shape` | `RoundedRectangleBorder(borderRadius: 12.0)` | `_CardDefaultsM3.shape` |
//! | `clipBehavior` | `Clip.none` | `_CardDefaultsM3` constructor |
//! | `margin` | `EdgeInsets.all(4.0)` | `_CardDefaultsM3` constructor |
//!
//! `M3 ColorScheme.shadow` is opaque black in both the light and dark
//! baselines (`color_scheme.rs`'s `shadow: Color::from_argb(0xFF00_0000)`),
//! which is exactly [`Material`]'s own built-in shadow color (it has no
//! `shadow_color` setter — see that module's docs) — so no plumbing gap
//! exists there. `surfaceTintColor: Colors.transparent` is the same named
//! deferral [`Material`] already carries (no theme-driven surface-tint
//! overlay substrate yet); nothing new to defer here.
//!
//! `card.dart`'s `build` also wraps the child in two `Semantics` nodes
//! (`semanticContainer`/`explicitChildNodes`) and threads `borderOnForeground`
//! (painting the shape's border in front of vs. behind the child) — neither
//! has a home in this substrate yet ([`Material`] paints no border at all,
//! and FLUI's semantics tree has no `Semantics.container` merge knob wired to
//! a `StatelessView` this shallow). Both are named, not silently dropped.
//!
//! # Deferred, and named
//!
//! - **`Card.filled` / `Card.outlined`** (`_FilledCardDefaultsM3`,
//!   `_OutlinedCardDefaultsM3`) — the two other M3 variants. Add as
//!   `Card::filled`/`Card::outlined` constructors once a caller needs them;
//!   the M3 token tables are already read above for the citation trail
//!   (`surfaceContainerHighest`/`elevation 0.0` for filled,
//!   `ColorScheme.surface` + an `OutlinedBorder` side in
//!   `ColorScheme.outlineVariant` for outlined) but nothing is wired.
//! - **`borderOnForeground`**, **`semanticContainer`** — see above.
//! - **`shadowColor`/`surfaceTintColor` overrides** — not exposed as builder
//!   methods, because [`Material`] has nowhere to put them yet.

use flui_types::Color;
use flui_types::EdgeInsets;
use flui_types::geometry::{Radius, px};
use flui_types::painting::Clip;
use flui_types::styling::BorderRadius;
use flui_view::prelude::*;
use flui_widgets::Padding;

use crate::material::Material;
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// `_CardDefaultsM3`'s corner radius (`card.dart`, oracle tag `3.44.0`).
const DEFAULT_CORNER_RADIUS: f32 = 12.0;
/// `_CardDefaultsM3`'s elevation (`card.dart`, oracle tag `3.44.0`).
const DEFAULT_ELEVATION: f32 = 1.0;
/// `_CardDefaultsM3`'s margin (`card.dart`, oracle tag `3.44.0`).
const DEFAULT_MARGIN: f32 = 4.0;

/// A Material Design elevated card — a panel with rounded corners and an
/// elevation shadow, wrapped in a fixed outer margin.
///
/// Elevated only; see the module docs for the filled/outlined variants this
/// V1 does not yet ship.
///
/// ```rust
/// use flui_material::Card;
/// use flui_widgets::Text;
///
/// let _card = Card::new(Text::new("A related panel of content"));
/// ```
#[derive(Clone, StatelessView)]
pub struct Card {
    color: Option<Color>,
    elevation: Option<f32>,
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
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
        self
    }

    /// Overrides the card's shape.
    #[must_use]
    pub fn shape(mut self, shape: MaterialShape) -> Self {
        self.shape = Some(shape);
        self
    }

    /// Overrides the card's clip behavior. Defaults to [`Clip::None`] —
    /// `_CardDefaultsM3`'s `clipBehavior` (`card.dart`, oracle tag `3.44.0`).
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = Some(clip_behavior);
        self
    }

    /// Overrides the outer margin. Defaults to `EdgeInsets.all(4.0)` —
    /// `_CardDefaultsM3`'s `margin` (`card.dart`, oracle tag `3.44.0`).
    #[must_use]
    pub fn margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = Some(margin);
        self
    }
}

impl StatelessView for Card {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let colors = Theme::of(ctx).color_scheme;
        let margin = self
            .margin
            .unwrap_or_else(|| EdgeInsets::all(px(DEFAULT_MARGIN)));
        let shape = self.shape.unwrap_or_else(|| {
            MaterialShape::RoundedRect(BorderRadius::all(Radius::circular(px(
                DEFAULT_CORNER_RADIUS,
            ))))
        });

        Padding::new(margin).child(
            Material::new(self.color.unwrap_or(colors.surface_container_low))
                .elevation(self.elevation.unwrap_or(DEFAULT_ELEVATION))
                .shape(shape)
                .clip_behavior(self.clip_behavior.unwrap_or(Clip::None))
                .child(self.child.clone()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_leaves_every_override_unset() {
        let card = Card::new(flui_widgets::SizedBox::shrink());
        assert!(card.color.is_none());
        assert!(card.elevation.is_none());
        assert!(card.shape.is_none());
        assert!(card.clip_behavior.is_none());
        assert!(card.margin.is_none());
    }

    #[test]
    fn overrides_are_stored_verbatim() {
        let card = Card::new(flui_widgets::SizedBox::shrink())
            .color(Color::rgb(1, 2, 3))
            .elevation(9.0)
            .shape(MaterialShape::Stadium)
            .clip_behavior(Clip::AntiAlias)
            .margin(EdgeInsets::all(px(10.0)));

        assert_eq!(card.color, Some(Color::rgb(1, 2, 3)));
        assert_eq!(card.elevation, Some(9.0));
        assert_eq!(card.shape, Some(MaterialShape::Stadium));
        assert_eq!(card.clip_behavior, Some(Clip::AntiAlias));
        assert_eq!(card.margin, Some(EdgeInsets::all(px(10.0))));
    }

    /// `_CardDefaultsM3`'s shape: `RoundedRectangleBorder(borderRadius:
    /// BorderRadius.all(Radius.circular(12.0)))` (`card.dart`, oracle tag
    /// `3.44.0`).
    #[test]
    fn default_shape_is_a_12dp_rounded_rect() {
        let expected = MaterialShape::RoundedRect(BorderRadius::all(Radius::circular(px(12.0))));
        // Constructed independently of `card.rs`'s own constant, so this
        // would catch the constant itself drifting from the oracle value.
        assert_eq!(
            expected
                .to_rrect(flui_types::Size::new(px(100.0), px(100.0)))
                .top_left,
            Radius::circular(px(12.0))
        );
    }
}
