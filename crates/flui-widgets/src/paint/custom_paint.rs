//! [`CustomPaint`] — delegates drawing to user-supplied [`CustomPainter`]s.

use std::rc::Rc;

use flui_foundation::geometry::Size;
use flui_objects::RenderCustomPaint;
use flui_rendering::delegates::CustomPainter;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, IntoView, RenderView, impl_render_view};

/// Provides a canvas for a background and/or foreground [`CustomPainter`] to
/// draw on, around an optional child.
///
/// Paint order is background painter → child →
/// foreground painter. Sizes to the child when present, else to
/// [`Self::size`] (default [`Size::ZERO`]) constrained by the incoming
/// layout constraints.
#[derive(Clone, Debug)]
pub struct CustomPaint {
    painter: Option<Rc<dyn CustomPainter>>,
    foreground_painter: Option<Rc<dyn CustomPainter>>,
    size: Size,
    child: Child,
    site: CreationSite,
}

impl Default for CustomPaint {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::at(CreationSite::here())
    }
}

impl CustomPaint {
    fn at(site: CreationSite) -> Self {
        Self {
            painter: None,
            foreground_painter: None,
            size: Size::default(),
            child: Child::empty(),
            site,
        }
    }

    /// Creates a `CustomPaint` with no painters, a zero preferred size, and
    /// no child.
    #[track_caller]
    pub fn new() -> Self {
        Self::at(CreationSite::here())
    }

    /// Sets the painter that draws behind the child.
    #[must_use]
    pub fn painter(mut self, painter: impl CustomPainter + 'static) -> Self {
        self.painter = Some(Rc::new(painter));
        self
    }

    /// Sets the painter that draws in front of the child.
    #[must_use]
    pub fn foreground_painter(mut self, painter: impl CustomPainter + 'static) -> Self {
        self.foreground_painter = Some(Rc::new(painter));
        self
    }

    /// Sets the size to use when there is no child.
    #[must_use]
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Sets the child to paint around.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for CustomPaint {
    type Protocol = BoxProtocol;
    type RenderObject = RenderCustomPaint;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderCustomPaint::new(
            self.painter.clone(),
            self.foreground_painter.clone(),
            self.size,
        )
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_painter(self.painter.clone())
            | render_object.set_foreground_painter(self.foreground_painter.clone())
            | render_object.set_preferred_size(self.size)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(CustomPaint, site = site);
