//! [`Container`] — the Flutter convenience widget that composes padding,
//! alignment, sizing, decoration, margin, and a transform around a child.

use flui_geometry::{EdgeInsets, Matrix4};
use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_types::styling::BoxDecoration;
use flui_types::{Alignment, Color, Pixels};
use flui_view::prelude::StatelessView;
use flui_view::{BoxedView, BuildContext, Child, IntoView, ViewExt};

use crate::layout::{Align, ConstrainedBox, LimitedBox, Padding, Transform};
use crate::paint::{ColoredBox, DecoratedBox};

/// A convenience widget that composes common painting, positioning, and sizing
/// widgets around a single child.
///
/// Flutter parity: `widgets/container.dart` `Container`. `build` composes, from
/// the child outward: `Align` → `Padding` → `ColoredBox` → `DecoratedBox` →
/// `ConstrainedBox` → `Padding` (margin) → `Transform`, each layer added only
/// when its property is set — exactly Flutter's order. `width`/`height` fold
/// into the constraints via `tightFor`/`tighten`.
///
/// # Parity scope
///
/// Decoration *painting* (color, gradient, border, radius, shadow) is faithful.
/// One Flutter nuance is not yet modelled: a [`BoxDecoration`] border's
/// thickness is not folded into the effective layout padding
/// (`_paddingIncludingDecoration`), because `flui-types`' `BoxDecoration` does
/// not expose border insets. Set `padding` explicitly if a bordered container
/// must reserve the border's thickness.
#[derive(Clone, Debug, Default, StatelessView)]
pub struct Container {
    alignment: Option<Alignment>,
    padding: Option<EdgeInsets>,
    color: Option<Color>,
    decoration: Option<BoxDecoration<Pixels>>,
    width: Option<f32>,
    height: Option<f32>,
    constraints: Option<BoxConstraints>,
    margin: Option<EdgeInsets>,
    transform: Option<Matrix4>,
    child: Child,
}

impl Container {
    /// An empty container. Configure it with the chainable setters below.
    pub fn new() -> Self {
        Self::default()
    }

    /// Align the child within the container (also makes a childless container
    /// expand to fill, per Flutter).
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Inset the child by `padding`.
    #[must_use]
    pub fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Paint a solid background `color` behind the child. Mutually exclusive
    /// with [`Container::decoration`] in Flutter; if both are set here, the
    /// color paints behind the decoration.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Paint a [`BoxDecoration`] behind the child.
    #[must_use]
    pub fn decoration(mut self, decoration: BoxDecoration<Pixels>) -> Self {
        self.decoration = Some(decoration);
        self
    }

    /// Force the container's width (folded into its constraints).
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Force the container's height (folded into its constraints).
    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Impose additional [`BoxConstraints`] on the child.
    #[must_use]
    pub fn constraints(mut self, constraints: BoxConstraints) -> Self {
        self.constraints = Some(constraints);
        self
    }

    /// Inset the container itself by `margin` (empty space outside any color/
    /// decoration).
    #[must_use]
    pub fn margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = Some(margin);
        self
    }

    /// Apply a paint-time [`Matrix4`] transform.
    #[must_use]
    pub fn transform(mut self, transform: Matrix4) -> Self {
        self.transform = Some(transform);
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// `width`/`height` fold into the additional constraints exactly as Flutter
    /// does: tighten the explicit constraints when present, else `tightFor`.
    fn effective_constraints(&self) -> Option<BoxConstraints> {
        if self.width.is_some() || self.height.is_some() {
            let width = self.width.map(px);
            let height = self.height.map(px);
            Some(match self.constraints {
                Some(constraints) => constraints.tighten(width, height),
                None => BoxConstraints::tight_for(width, height),
            })
        } else {
            self.constraints
        }
    }
}

impl StatelessView for Container {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Innermost: the child, or Flutter's childless placeholder
        // (LimitedBox(0,0) over a ConstrainedBox.expand()) so a childless
        // container fills bounded space and collapses under unbounded space.
        let mut current: BoxedView = match self.child.clone().into_inner() {
            Some(boxed) => boxed,
            None => LimitedBox::new(0.0, 0.0)
                .child(ConstrainedBox::new(BoxConstraints::expand()))
                .boxed(),
        };

        if let Some(alignment) = self.alignment {
            current = Align::new(alignment).child(current).boxed();
        }
        if let Some(padding) = self.padding {
            current = Padding::new(padding).child(current).boxed();
        }
        if let Some(color) = self.color {
            current = ColoredBox::new(color).child(current).boxed();
        }
        if let Some(decoration) = &self.decoration {
            current = DecoratedBox::new(decoration.clone()).child(current).boxed();
        }
        if let Some(constraints) = self.effective_constraints() {
            current = ConstrainedBox::new(constraints).child(current).boxed();
        }
        if let Some(margin) = self.margin {
            current = Padding::new(margin).child(current).boxed();
        }
        if let Some(transform) = self.transform {
            current = Transform::new(transform).child(current).boxed();
        }

        current
    }
}
