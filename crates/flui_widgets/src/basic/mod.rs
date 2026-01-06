//! Basic single-child layout widgets.
//!
//! This module contains fundamental widgets for basic layout operations:
//! - Padding: Adds padding around a child
//! - Center: Centers its child
//! - SizedBox: Forces specific size constraints
//! - ColoredBox: Paints a colored rectangle

// Active widgets (using new RenderBox architecture)
pub mod center;
pub mod colored_box;
pub mod padding;
pub mod sized_box;

// Re-exports
pub use center::Center;
pub use colored_box::ColoredBox;
pub use padding::Padding;
pub use sized_box::SizedBox;

// ============================================================================
// DISABLED: Widgets below use old flui_core/flui_objects architecture
// They will be migrated when their RenderObjects are implemented
// ============================================================================

// pub mod text;  // Needs RenderParagraph
// pub mod container;
// pub mod align;
// pub mod aspect_ratio;
// pub mod baseline;
// pub mod constrained_box;
// pub mod fitted_box;
// pub mod limited_box;
// pub mod offstage;
// pub mod opacity;
// pub mod transform;
