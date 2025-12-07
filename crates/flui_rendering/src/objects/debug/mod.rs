pub mod error_box;
// TODO: Re-enable once implemented
// pub mod performance_overlay;
pub mod placeholder;

pub use error_box::RenderErrorBox;
// TODO: Re-enable once implemented
// pub use performance_overlay::RenderPerformanceOverlay;
pub use placeholder::RenderPlaceholder;

// TODO: Implement PerformanceOverlay
// pub mod performance_overlay;
// pub use performance_overlay::RenderPerformanceOverlay;

// TODO: Re-enable once migrated to flui_painting::Canvas API
// #[cfg(debug_assertions)]
// pub mod overflow_indicator;
//
// #[cfg(debug_assertions)]
// pub use overflow_indicator::paint_overflow_indicators;
