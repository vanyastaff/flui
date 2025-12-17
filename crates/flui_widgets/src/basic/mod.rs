//! Basic single-child layout widgets.
//!
//! This module contains fundamental widgets for basic layout operations:
//! - Text: Displays styled text
//! - Container: Combines padding, decoration, and sizing
//! - Padding: Adds padding around a child
//! - Center: Centers its child
//! - And many more...

// Temporarily disabled widgets for counter demo - only Text is enabled
// pub mod align;
// pub mod app_bar;
// pub mod aspect_ratio;
// pub mod builder;
// pub mod button;
// pub mod card;
// pub mod center;
// pub mod colored_box;
// pub mod constrained_box;
// pub mod container;
// pub mod custom_paint;
// pub mod decorated_box;
// pub mod divider;
// pub mod empty;
// pub mod fitted_box;
// pub mod layout_builder;
// pub mod limited_box;
pub mod padding;
// pub mod safe_area;
// pub mod sized_box;
pub mod text;
// pub mod vertical_divider;

// Re-exports
pub use padding::Padding;
pub use text::Text;
