//! Paint-effect widgets — wrap a child with a visual effect (color, decoration,
//! opacity) or a compositing boundary without changing its layout. Each is a
//! thin [`RenderView`](flui_view::prelude::RenderView) over a `flui-objects`
//! proxy.

mod colored_box;
mod custom_paint;
mod decorated_box;
mod opacity;
mod repaint_boundary;

pub use colored_box::ColoredBox;
pub use custom_paint::CustomPaint;
pub use decorated_box::DecoratedBox;
pub use opacity::Opacity;
pub use repaint_boundary::RepaintBoundary;
