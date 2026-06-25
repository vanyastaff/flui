//! Paint-effect widgets — wrap a child with a visual effect (color, decoration,
//! opacity) without changing its layout. Each is a thin
//! [`RenderView`](flui_view::prelude::RenderView) over a `flui-objects` proxy.

mod colored_box;
mod decorated_box;
mod opacity;

pub use colored_box::ColoredBox;
pub use decorated_box::DecoratedBox;
pub use opacity::Opacity;
