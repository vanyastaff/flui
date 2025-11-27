pub mod error_box;
pub mod performance_overlay;
pub mod placeholder;

pub use error_box::RenderErrorBox;
pub use performance_overlay::RenderPerformanceOverlay;
pub use placeholder::RenderPlaceholder;

// Note: overflow_indicator is currently disabled pending layer system refactoring.
// The implementation exists in overflow_indicator.rs but requires direct access
// to flui_engine::layer types which should be abstracted through flui_painting.
// This is a debug-only feature that can be re-enabled when the layer abstraction is complete.
// #[cfg(debug_assertions)]
// pub mod overflow_indicator;
//
// #[cfg(debug_assertions)]
// pub use overflow_indicator::{paint_overflow_indicators, RenderOverflowIndicator};
