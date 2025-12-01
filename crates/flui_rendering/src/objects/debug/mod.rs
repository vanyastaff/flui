pub mod error_box;
pub mod performance_overlay;
pub mod placeholder;

pub use error_box::RenderErrorBox;
pub use performance_overlay::RenderPerformanceOverlay;
pub use placeholder::RenderPlaceholder;

// TODO: Re-enable once migrated to flui_painting::Canvas API
// #[cfg(debug_assertions)]
// pub mod overflow_indicator;
//
// #[cfg(debug_assertions)]
// pub use overflow_indicator::paint_overflow_indicators;
