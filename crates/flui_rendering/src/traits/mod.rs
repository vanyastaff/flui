//! Trait definitions for render objects.
//!
//! This module defines the trait hierarchy for render objects:
//!
//! ```text
//! RenderObject (base)
//!     ├── RenderBox (2D layout)
//!     │   ├── SingleChildRenderBox
//!     │   │   ├── RenderProxyBox
//!     │   │   └── RenderShiftedBox
//!     │   └── MultiChildRenderBox
//!     └── RenderSliver (scrollable)
//!         ├── RenderProxySliver
//!         └── RenderSliverMultiBoxAdaptor
//! ```

mod render_box;
mod render_object;
mod render_sliver;

pub use render_box::*;
pub use render_object::*;
pub use render_sliver::*;
