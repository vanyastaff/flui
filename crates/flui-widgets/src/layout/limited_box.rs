//! [`LimitedBox`] — caps its child's size only when the incoming constraint on
//! that axis is unbounded.

use flui_objects::RenderLimitedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Pixels;
use flui_types::geometry::px;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Caps the maximum size of its child *only when* the corresponding incoming
/// constraint is unbounded; bounded constraints pass through untouched.
///
/// Flutter parity: `widgets/basic.dart` `LimitedBox` over `RenderLimitedBox`.
/// `f32::INFINITY` for a dimension means "no cap on that axis".
#[derive(Clone, Debug)]
pub struct LimitedBox {
    max_width: f32,
    max_height: f32,
    child: Child,
}

impl LimitedBox {
    /// A box capping width/height (use `f32::INFINITY` for "no cap").
    pub fn new(max_width: f32, max_height: f32) -> Self {
        Self {
            max_width,
            max_height,
            child: Child::empty(),
        }
    }

    /// Set the capped child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    fn cap(value: f32) -> Option<Pixels> {
        value.is_finite().then(|| px(value))
    }
}

impl RenderView for LimitedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderLimitedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderLimitedBox::new(Self::cap(self.max_width), Self::cap(self.max_height))
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        *render_object =
            RenderLimitedBox::new(Self::cap(self.max_width), Self::cap(self.max_height));
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

impl_render_view!(LimitedBox);
