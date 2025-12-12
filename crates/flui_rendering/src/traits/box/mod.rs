//! Box protocol traits

mod render_box;
mod single_child;
mod proxy_box;
mod shifted_box;
mod aligning_shifted_box;
mod multi_child;

pub use render_box::{BoxHitTestResult, PaintingContext, RenderBox, TextBaseline};
pub use single_child::SingleChildRenderBox;
pub use proxy_box::RenderProxyBox;
pub use shifted_box::RenderShiftedBox;
pub use aligning_shifted_box::RenderAligningShiftedBox;
pub use multi_child::MultiChildRenderBox;
