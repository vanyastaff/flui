//! Sliver protocol traits for scrollable content layout.
//!
//! # Trait Hierarchy
//!
//! ```text
//! RenderSliver
//!     ├── RenderProxySliver (single sliver child)
//!     ├── RenderSliverSingleBoxAdapter (single box child) + RenderSliverHelpers
//!     ├── RenderSliverMultiBoxAdaptor (multiple box children) + RenderSliverHelpers
//!     │   └── RenderSliverWithKeepAliveMixin
//!     ├── RenderSliverPersistentHeader (persistent header)
//!     └── RenderSliverEdgeInsetsPadding (sliver with padding)
//!
//! # Mixins
//!
//! - RenderSliverHelpers: Utility methods for slivers with box children
//! - RenderSliverWithKeepAliveMixin: Keep-alive support for sliver children
//! ```

mod edge_insets_padding;
mod helpers;
mod keep_alive;
mod multi_box_adaptor;
mod persistent_header;
mod proxy_sliver;
mod render_sliver;
mod single_box_adapter;

pub use edge_insets_padding::*;
pub use helpers::*;
pub use keep_alive::*;
pub use multi_box_adaptor::*;
pub use persistent_header::*;
pub use proxy_sliver::*;
pub use render_sliver::*;
pub use single_box_adapter::*;
