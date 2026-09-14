//! [`Container`] — the Flutter convenience widget that composes padding,
//! alignment, sizing, decoration, margin, and a transform around a child.

use flui_foundation::ViewKey;
use flui_geometry::{EdgeInsets, Matrix4};
use flui_rendering::constraints::{BoxConstraints, Constraints};
use flui_types::geometry::px;
use flui_types::styling::BoxDecoration;
use flui_types::{Alignment, Color, Pixels};
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, Child, GlobalKey, IntoView, StatelessView, View, ViewExt, ViewState};

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
/// # State stability
///
/// Flutter's conditional stack recreates an unkeyed child when an optional
/// layer appears or disappears (flutter/flutter#161698). FLUI keeps that
/// composition shape for layout/paint parity, but owns a [`GlobalKey`] slot
/// around the caller's child so toggling options reparents the child instead
/// of disposing it. See `ARCHITECTURE.md` mapping decision 15.
///
/// # Parity scope
///
/// Decoration *painting* (color, gradient, border, radius, shadow) is faithful.
/// One Flutter nuance is not yet modelled: a [`BoxDecoration`] border's
/// thickness is not folded into the effective layout padding
/// (`_paddingIncludingDecoration`), because `flui-types`' `BoxDecoration` does
/// not expose border insets. Set `padding` explicitly if a bordered container
/// must reserve the border's thickness.
#[derive(Clone, Debug, Default, StatefulView)]
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

/// State for [`Container`]: owns the [`GlobalKey`] that pins the caller's child
/// across optional-layer topology changes.
pub struct ContainerState {
    child_slot_key: GlobalKey<()>,
}

impl std::fmt::Debug for ContainerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContainerState").finish_non_exhaustive()
    }
}

impl StatefulView for Container {
    type State = ContainerState;

    fn create_state(&self) -> Self::State {
        ContainerState {
            child_slot_key: GlobalKey::new(),
        }
    }
}

impl ViewState<Container> for ContainerState {
    fn build(&self, view: &Container, _ctx: &dyn BuildContext) -> impl IntoView {
        let effective_constraints = view.effective_constraints();
        let child = view.child.clone().into_inner().map(|child| {
            ContainerChildSlot {
                key: self.child_slot_key.clone(),
                child,
            }
            .boxed()
        });

        // Innermost: the child, or Flutter's childless placeholder
        // (LimitedBox(0,0) over a ConstrainedBox.expand()) so a childless
        // container fills bounded space and collapses under unbounded space.
        // Tight effective constraints suppress that placeholder; only then may
        // a childless alignment produce an empty Align, matching Flutter's
        // mutually-exclusive `if (...) placeholder else if (...) Align`.
        let use_placeholder = child.is_none()
            && effective_constraints
                .as_ref()
                .is_none_or(|constraints| !constraints.is_tight());
        let mut current: Option<BoxedView> = if use_placeholder {
            Some(
                LimitedBox::new(0.0, 0.0)
                    .child(ConstrainedBox::new(BoxConstraints::expand()))
                    .boxed(),
            )
        } else if let Some(alignment) = view.alignment {
            Some(match child {
                Some(child) => Align::new(alignment).child(child).boxed(),
                None => Align::new(alignment).boxed(),
            })
        } else {
            child
        };
        if let Some(padding) = view.padding {
            current = Some(match current {
                Some(child) => Padding::new(padding).child(child).boxed(),
                None => Padding::new(padding).boxed(),
            });
        }
        if let Some(color) = view.color {
            current = Some(match current {
                Some(child) => ColoredBox::new(color).child(child).boxed(),
                None => ColoredBox::new(color).boxed(),
            });
        }
        if let Some(decoration) = &view.decoration {
            current = Some(match current {
                Some(child) => DecoratedBox::new(decoration.clone()).child(child).boxed(),
                None => DecoratedBox::new(decoration.clone()).boxed(),
            });
        }
        if let Some(constraints) = effective_constraints {
            current = Some(match current {
                Some(child) => ConstrainedBox::new(constraints).child(child).boxed(),
                None => ConstrainedBox::new(constraints).boxed(),
            });
        }
        let mut current = match current {
            Some(current) => current,
            None => LimitedBox::new(0.0, 0.0)
                .child(ConstrainedBox::new(BoxConstraints::expand()))
                .boxed(),
        };
        if let Some(margin) = view.margin {
            current = Padding::new(margin).child(current).boxed();
        }
        if let Some(transform) = view.transform {
            current = Transform::new(transform).child(current).boxed();
        }

        current
    }
}

/// Stateless host that carries [`Container`]'s child-slot [`GlobalKey`].
///
/// Owns no render object: the child's render node still attaches to whatever
/// optional layer sits above this slot, so layout/paint topology stays Flutter-
/// shaped while the element can be retaken across layer insert/remove.
#[derive(Clone)]
struct ContainerChildSlot {
    key: GlobalKey<()>,
    child: BoxedView,
}

impl std::fmt::Debug for ContainerChildSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContainerChildSlot").finish_non_exhaustive()
    }
}

impl View for ContainerChildSlot {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

impl StatelessView for ContainerChildSlot {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }
}
