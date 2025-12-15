//! Box protocol traits for 2D cartesian layout.
//!
//! # Trait Hierarchy
//!
//! ```text
//! RenderBox
//!     ├── SingleChildRenderBox
//!     │   ├── RenderProxyBox (size = child size)
//!     │   │   ├── HitTestProxy
//!     │   │   └── ClipProxy<T>
//!     │   └── RenderShiftedBox (custom offset)
//!     │       └── RenderAligningShiftedBox
//!     └── MultiChildRenderBox
//!         └── RenderBoxContainerDefaultsMixin
//!
//! # Mixins
//!
//! - RenderAnimatedOpacityMixin: Animated opacity support
//! - RenderBoxContainerDefaultsMixin: Default container implementations
//! ```

mod aligning_shifted_box;
mod animated_opacity;
mod container_defaults;
mod multi_child;
mod proxy_box;
mod render_box;
mod shifted_box;
mod single_child;

pub use aligning_shifted_box::*;
pub use animated_opacity::*;
pub use container_defaults::*;
pub use multi_child::*;
pub use proxy_box::*;
pub use render_box::*;
pub use shifted_box::*;
pub use single_child::*;

// Re-export Ambassador delegation macros for these traits
pub use aligning_shifted_box::ambassador_impl_RenderAligningShiftedBox;
pub use proxy_box::ambassador_impl_RenderProxyBox;
pub use shifted_box::ambassador_impl_RenderShiftedBox;
pub use single_child::ambassador_impl_SingleChildRenderBox;
