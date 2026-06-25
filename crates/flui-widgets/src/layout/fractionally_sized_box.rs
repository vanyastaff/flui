//! [`FractionallySizedBox`] — sizes its child to a fraction of the available
//! space.

use flui_objects::{FractionFactor, RenderFractionallySizedBox};
use flui_rendering::protocol::BoxProtocol;
use flui_types::Alignment;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Sizes its child to a fraction of the available space along each axis.
///
/// Flutter parity: `widgets/basic.dart` `FractionallySizedBox` over
/// `RenderFractionallySizedBox`. A `None` factor leaves that axis at the
/// incoming constraint; factors must be finite and `>= 0`. Defaults to
/// `Alignment::CENTER`.
#[derive(Clone, Debug, Default)]
pub struct FractionallySizedBox {
    width_factor: Option<f32>,
    height_factor: Option<f32>,
    alignment: Option<Alignment>,
    child: Child,
}

impl FractionallySizedBox {
    /// An empty `FractionallySizedBox`; set factors with the builders below.
    pub fn new() -> Self {
        Self::default()
    }

    /// Size the child's width to `factor` × the available width.
    #[must_use]
    pub fn width_factor(mut self, factor: f32) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Size the child's height to `factor` × the available height.
    #[must_use]
    pub fn height_factor(mut self, factor: f32) -> Self {
        self.height_factor = Some(factor);
        self
    }

    /// Set how the sized child is aligned within the box.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Set the fractionally-sized child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    fn factor(value: Option<f32>) -> Option<FractionFactor> {
        value.map(FractionFactor::new_unchecked)
    }

    fn build_render_object(&self) -> RenderFractionallySizedBox {
        let mut render_object = RenderFractionallySizedBox::new();
        if let Some(alignment) = self.alignment {
            render_object = render_object.with_alignment(alignment);
        }
        if let Some(factor) = Self::factor(self.width_factor) {
            render_object = render_object.with_width_factor(factor);
        }
        if let Some(factor) = Self::factor(self.height_factor) {
            render_object = render_object.with_height_factor(factor);
        }
        render_object
    }
}

impl RenderView for FractionallySizedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFractionallySizedBox;

    fn create_render_object(&self) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_width_factor(Self::factor(self.width_factor));
        render_object.set_height_factor(Self::factor(self.height_factor));
        if let Some(alignment) = self.alignment {
            render_object.set_alignment(alignment);
        }
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(FractionallySizedBox);
