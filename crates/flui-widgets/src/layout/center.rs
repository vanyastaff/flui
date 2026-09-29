//! [`Center`] — centers its child within itself.

use flui_objects::RenderCenter;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Centers its child within itself.
///
/// Flutter parity: `widgets/basic.dart` `Center extends Align` with
/// `Alignment.center`. Optionally sizes itself to a multiple of the child's
/// dimensions via `width_factor`/`height_factor`.
#[derive(Clone, Debug, Default)]
pub struct Center {
    width_factor: Option<f64>,
    height_factor: Option<f64>,
    child: Child,
}

impl Center {
    /// Create a `Center` with no child yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Size this box to `factor` × the child's width (must be `>= 0`).
    #[must_use]
    pub fn width_factor(mut self, factor: f64) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Size this box to `factor` × the child's height (must be `>= 0`).
    #[must_use]
    pub fn height_factor(mut self, factor: f64) -> Self {
        self.height_factor = Some(factor);
        self
    }

    /// Set the centered child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    fn build_render_object(&self) -> RenderCenter {
        let mut render_object = RenderCenter::new();
        if let Some(factor) = self.width_factor {
            render_object = render_object.with_width_factor(factor);
        }
        if let Some(factor) = self.height_factor {
            render_object = render_object.with_height_factor(factor);
        }
        render_object
    }
}

impl RenderView for Center {
    type Protocol = BoxProtocol;
    type RenderObject = RenderCenter;

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
        render_object.update_factors(self.width_factor, self.height_factor)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Center);
