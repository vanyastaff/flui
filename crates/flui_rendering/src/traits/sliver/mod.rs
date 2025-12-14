//! Sliver protocol traits for scrollable content layout.
//!
//! # Trait Hierarchy
//!
//! ```text
//! RenderSliver
//!     ├── RenderProxySliver (single sliver child)
//!     ├── RenderSliverSingleBoxAdapter (single box child)
//!     ├── RenderSliverMultiBoxAdaptor (multiple box children)
//!     └── RenderSliverPersistentHeader (persistent header)
//! ```

mod multi_box_adaptor;
mod persistent_header;
mod proxy_sliver;
mod render_sliver;
mod single_box_adapter;

pub use multi_box_adaptor::*;
pub use persistent_header::*;
pub use proxy_sliver::*;
pub use render_sliver::*;
pub use single_box_adapter::*;
