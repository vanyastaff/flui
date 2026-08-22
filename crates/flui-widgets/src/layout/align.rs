//! [`Align`] — aligns its child within itself and optionally sizes itself to a
//! multiple of the child's size.

use flui_objects::RenderAlign;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Alignment;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Aligns its child within itself.
///
/// Flutter parity: `widgets/basic.dart` `Align` over `RenderPositionedBox`
/// (here `RenderAlign`). With no size factors the box expands to fill the
/// incoming constraints (or shrinks to the child when a dimension is
/// unbounded); a `width_factor`/`height_factor` sizes the box to that multiple
/// of the child's corresponding dimension.
#[derive(Clone, Debug)]
pub struct Align {
    alignment: Alignment,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
    child: Child,
}

impl Align {
    /// Align a child at the given [`Alignment`] (e.g. [`Alignment::CENTER`]).
    pub fn new(alignment: Alignment) -> Self {
        Self {
            alignment,
            width_factor: None,
            height_factor: None,
            child: Child::empty(),
        }
    }

    /// Size this box to `factor` × the child's width (must be `>= 0`).
    #[must_use]
    pub fn width_factor(mut self, factor: f32) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Size this box to `factor` × the child's height (must be `>= 0`).
    #[must_use]
    pub fn height_factor(mut self, factor: f32) -> Self {
        self.height_factor = Some(factor);
        self
    }

    /// Set the aligned child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    fn build_render_object(&self) -> RenderAlign {
        let mut render_object = RenderAlign::new(self.alignment);
        if let Some(factor) = self.width_factor {
            render_object = render_object.with_width_factor(factor);
        }
        if let Some(factor) = self.height_factor {
            render_object = render_object.with_height_factor(factor);
        }
        render_object
    }
}

impl RenderView for Align {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAlign;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.update_configuration(self.alignment, self.width_factor, self.height_factor)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Align);

#[cfg(test)]
mod tests {
    use flui_view::RenderView;

    use super::*;

    #[test]
    fn update_reports_layout_only_for_changed_configuration() {
        let initial = Align::new(Alignment::CENTER).width_factor(2.0);
        let mut render = initial.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(
            initial.update_render_object(&flui_view::RenderObjectContext::detached(), &mut render,),
            flui_rendering::RenderUpdateImpact::NONE
        );
        assert_eq!(
            Align::new(Alignment::BOTTOM_RIGHT)
                .width_factor(2.0)
                .update_render_object(&flui_view::RenderObjectContext::detached(), &mut render,),
            flui_rendering::RenderUpdateImpact::LAYOUT
        );
    }
}
