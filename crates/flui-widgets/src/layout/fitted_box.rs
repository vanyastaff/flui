//! [`FittedBox`] — scales and positions its child within itself per a [`BoxFit`].

use flui_objects::RenderFittedBox;
use flui_painting::Alignment;
use flui_painting::BoxFit;
use flui_painting::paint::Clip;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Scales and positions its child within itself according to a [`BoxFit`].
///
/// Flutter parity: `widgets/basic.dart` `FittedBox` over `RenderFittedBox`.
/// Defaults match Flutter: `BoxFit::Contain`, `Alignment::CENTER`, `Clip::None`.
#[derive(Clone, Debug)]
pub struct FittedBox {
    fit: BoxFit,
    alignment: Alignment,
    clip: Clip,
    child: Child,
}

impl Default for FittedBox {
    fn default() -> Self {
        Self {
            fit: BoxFit::Contain,
            alignment: Alignment::CENTER,
            clip: Clip::None,
            child: Child::empty(),
        }
    }
}

impl FittedBox {
    /// A `FittedBox` with Flutter's defaults (`Contain` / centered / no clip).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set how the child is inscribed into the available space.
    #[must_use]
    pub fn fit(mut self, fit: BoxFit) -> Self {
        self.fit = fit;
        self
    }

    /// Set how the scaled child is aligned within the box.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Set the clip behavior applied when the child overflows.
    #[must_use]
    pub fn clip(mut self, clip: Clip) -> Self {
        self.clip = clip;
        self
    }

    /// Set the fitted child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for FittedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFittedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderFittedBox::new(self.fit, self.alignment, self.clip)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_fit(self.fit)
            | render_object.set_alignment(self.alignment)
            | render_object.set_clip_behavior(self.clip)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(FittedBox);
