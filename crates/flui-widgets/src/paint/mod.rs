//! Paint-effect widgets — wrap a child with a visual effect (color, decoration,
//! opacity) or a compositing boundary without changing its layout. Each is a
//! thin [`RenderView`](flui_view::prelude::RenderView) over a `flui-objects`
//! proxy.

mod annotated_region;
mod backdrop_filter;
mod colored_box;
mod custom_paint;
mod decorated_box;
mod ignore_pointer;
mod opacity;
pub(crate) mod repaint_boundary;

pub use annotated_region::AnnotatedRegion;
pub use backdrop_filter::{BackdropFilter, BackdropGroup, BackdropGroupState};
pub use colored_box::ColoredBox;
pub use custom_paint::CustomPaint;
pub use decorated_box::DecoratedBox;
pub use ignore_pointer::IgnorePointer;
pub use opacity::Opacity;
pub use repaint_boundary::RepaintBoundary;
