//! # flui_painting
//!
//! Visual primitives layer between RenderObjects and egui::Painter.
//!
//! This crate provides the implementation of painting logic for visual primitives
//! defined in flui_types. It bridges the gap between declarative styling types
//! (BoxDecoration, Border, Gradient, Shadow) and actual rendering via egui::Painter.
//!
//! ## Architecture
//!
//! ```text
//! RenderObject (flui_rendering)
//!     ↓
//! BoxDecoration, Border, etc. (flui_types - data structures)
//!     ↓
//! Painting Traits (flui_painting - this crate)
//!     ↓
//! egui::Painter (rendering backend)
//! ```
//!
//! ## Key Components
//!
//! - **BoxDecorationPainter**: Paints BoxDecoration (background, border, shadow, gradient)
//! - **BorderPainter**: Paints Border with rounded corners
//! - **GradientPainter**: Paints Linear/Radial/Sweep gradients
//! - **ShadowPainter**: Paints box shadows with blur
//! - **TextPainter**: Paints text with alignment, direction, overflow handling
//! - **ImagePainter**: Paints images with fit, repeat, and alignment
//! - **PathPainter**: Paints vector paths with fill and stroke
//! - **ShapePainter**: Paints shape borders (rounded, circle, star, etc.)
//!
//! ## Design Principles
//!
//! 1. **Pure functions**: All painting methods are pure (no state)
//! 2. **Zero allocations**: Reuse egui primitives, no intermediate buffers
//! 3. **Type safety**: Leverage Rust's type system for correctness
//! 4. **Separation of concerns**: Data (flui_types) vs Logic (flui_painting)

#![warn(missing_docs)]
pub mod border;
pub mod decoration;
pub mod gradient;
pub mod image;
pub mod path;
pub mod shadow;
pub mod shape;
pub mod text;







// Re-export main painting traits
pub use decoration::BoxDecorationPainter;
pub use border::BorderPainter;
pub use gradient::GradientPainter;
pub use shadow::ShadowPainter;
pub use text::TextPainter;
pub use image::ImagePainter;
pub use path::PathPainter;
pub use shape::ShapePainter;

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::decoration::BoxDecorationPainter;
    pub use crate::border::BorderPainter;
    pub use crate::gradient::GradientPainter;
    pub use crate::shadow::ShadowPainter;
    pub use crate::text::TextPainter;
    pub use crate::image::ImagePainter;
    pub use crate::path::PathPainter;
    pub use crate::shape::ShapePainter;
}







