//! [`Container`] — the convenience widget that composes padding,
//! alignment, sizing, decoration, margin, and a transform around a child.

use flui_foundation::geometry::{EdgeInsets, Matrix4};
use flui_objects::RenderContainer;
use flui_painting::Alignment;
use flui_painting::styling::BoxDecoration;
use flui_painting::styling::Color;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// A convenience widget that composes common painting, positioning, and sizing
/// behaviour around a single child.
///
/// # One render object, not a stack
///
/// `Container` is not a conditional widget stack (`Align` → `Padding` →
/// `ColoredBox` → `DecoratedBox` → `ConstrainedBox` → `Padding` (margin) →
/// `Transform`, each layer present only while its property is set). FLUI keeps
/// that stack's observable geometry and folds it into one [`RenderContainer`].
/// Two things follow, and both are the reason:
///
/// * **Toggling an option does not recreate the child.** In a conditional
///   stack, an option turning on or off inserts or removes a level between the
///   parent and the child, so reconciliation diverges there and every element
///   below — including an unkeyed stateful child — is rebuilt from scratch.
///   Here the options are render-object fields, so
///   the child's slot never moves and no state is lost. No `GlobalKey`, no
///   reparenting, nothing for the caller to opt into.
/// * **Node count depends on whether there is a child.** With a child, the
///   stack costs zero extra nodes at identity (no options → the child itself)
///   and exactly one extra node per option set below that, so one node here is
///   a wash on count at one option, and only wins from two up (up to seven if
///   every option is set). Childless, the stack is never free: it reaches for
///   a two-node placeholder (`LimitedBox` + `ConstrainedBox`) even with no
///   option set at all (`Container()`), so one node here already wins; the
///   only childless tie is a *tight* effective constraint — both `width` and
///   `height` set, or an explicit tight `constraints` — which suppresses the
///   placeholder and leaves a single `ConstrainedBox` against one node here.
///   A lone `width` does not qualify: `BoxConstraints::is_tight` requires both
///   axes, so that case still takes the placeholder and costs three. Every
///   other childless option — color, padding, decoration, an alignment paired
///   with a fixed size — only grows the stack's node count further. What one
///   node here does *not* buy, in either regime, is a lighter node:
///   `RenderContainer` carries every field whether or not that option is set,
///   so it is heavier than whichever single-purpose object the stack would
///   have used. The reason for the divergence is the stable child slot, not a
///   cheaper or lighter `Container`. Because the identity case (with a child)
///   is still a `RenderContainer`, it is **not** parent-data-transparent: put
///   [`crate::Expanded`] / [`crate::Positioned`] *around* the container
///   (`Row → Expanded → Container`), not inside it.
///
/// The divergence is recorded in `ARCHITECTURE.md` mapping decision 15, and
/// the geometry is pinned against the stack it replaces by
/// `harness_container_matches_the_widget_stack_it_collapses`.
///
/// # Decoration limits
///
/// Decoration *painting* (color, gradient, border, radius, shadow) is
/// complete, but a [`BoxDecoration`]
/// border's thickness is not folded into the effective layout padding,
/// because `flui-painting`'s `BoxDecoration` does
/// not expose border insets. Set `padding` explicitly if a bordered container
/// must reserve the border's thickness.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// let _ = Container::new().width(120.0).padding(EdgeInsets::all(8.0));
/// ```
#[derive(Clone, Debug, Default)]
pub struct Container {
    alignment: Option<Alignment>,
    padding: Option<EdgeInsets>,
    color: Option<Color>,
    decoration: Option<BoxDecoration<f64>>,
    width: Option<f64>,
    height: Option<f64>,
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
    /// expand to fill).
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

    /// Paint a solid background `color` behind the child.
    ///
    /// Conceptually exclusive with [`Container::decoration`]; if both
    /// are set here, the color paints *over* the decoration, which is the
    /// order the widget stack produces (`DecoratedBox` encloses `ColoredBox`).
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Paint a [`BoxDecoration`] behind the child.
    #[must_use]
    pub fn decoration(mut self, decoration: BoxDecoration<f64>) -> Self {
        self.decoration = Some(decoration);
        self
    }

    /// Force the container's width (folded into its constraints).
    #[must_use]
    pub fn width(mut self, width: f64) -> Self {
        self.width = Some(width);
        self
    }

    /// Force the container's height (folded into its constraints).
    #[must_use]
    pub fn height(mut self, height: f64) -> Self {
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

    /// `width`/`height` fold into the additional constraints as
    /// follows: tighten the explicit constraints when present, else
    /// tight-for the given size.
    fn effective_constraints(&self) -> Option<BoxConstraints> {
        if self.width.is_some() || self.height.is_some() {
            let width = self.width;
            let height = self.height;
            Some(match self.constraints {
                Some(constraints) => constraints.tighten(width, height),
                None => BoxConstraints::tight_for(width, height),
            })
        } else {
            self.constraints
        }
    }
}

impl RenderView for Container {
    type Protocol = BoxProtocol;
    type RenderObject = RenderContainer;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let mut render_object = RenderContainer::new();
        let _ = render_object.set_alignment(self.alignment);
        let _ = render_object.set_padding(self.padding);
        let _ = render_object.set_margin(self.margin.unwrap_or_default());
        let _ = render_object.set_color(self.color);
        let _ = render_object.set_decoration(self.decoration.clone());
        let _ = render_object.set_additional_constraints(self.effective_constraints());
        let _ = render_object.set_transform(self.transform);
        render_object
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_alignment(self.alignment);
        impact |= render_object.set_padding(self.padding);
        impact |= render_object.set_margin(self.margin.unwrap_or_default());
        impact |= render_object.set_color(self.color);
        impact |= render_object.set_decoration(self.decoration.clone());
        impact |= render_object.set_additional_constraints(self.effective_constraints());
        impact |= render_object.set_transform(self.transform);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Container);
