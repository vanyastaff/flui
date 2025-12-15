//! Type-safe container system for render objects.
//!
//! Containers use Protocol associated types to store protocol-specific children
//! at compile time, eliminating runtime type checks and downcasts.
//!
//! # Container Types
//!
//! - [`Single`]: Zero or one child
//! - [`Children`]: Multiple children with parent data
//! - [`Proxy`]: Single child where size equals child's size
//! - [`Shifted`]: Single child with custom offset positioning
//! - [`Aligning`]: Single child with alignment and size factors
//! - [`Adapter`]: Cross-protocol wrapper (zero-cost)
//!
//! # Type Aliases
//!
//! For ergonomics, each container has protocol-specific aliases:
//!
//! ```rust,ignore
//! // Box protocol
//! type BoxChild = Single<BoxProtocol>;
//! type ProxyBox = Proxy<BoxProtocol>;
//! type ShiftedBox = Shifted<BoxProtocol>;
//! type AligningBox = Aligning<BoxProtocol>;
//! type BoxChildren<PD> = Children<BoxProtocol, PD>;
//!
//! // Sliver protocol
//! type SliverChild = Single<SliverProtocol>;
//! type SliverProxy = Proxy<SliverProtocol>;
//! // ...
//!
//! // Cross-protocol adapters
//! type BoxToSliver = Adapter<Single<BoxProtocol>, SliverProtocol>;
//! type SliverToBox = Adapter<Single<SliverProtocol>, BoxProtocol>;
//! type MultiSliverToBox = Adapter<Children<SliverProtocol>, BoxProtocol>;
//! ```

mod adapter;
mod aligning;
mod children;
mod proxy;
mod shifted;
mod single;
mod viewport;

pub use adapter::{
    Adapter, BoxToSliver, MultiBoxToSliver, MultiSliverToBox, OptionalBoxToSliver,
    OptionalSliverToBox, SliverToBox,
};
pub use aligning::*;
pub use children::{BoxChildren, ChildEntry, Children, HasOffset, SliverChildren};
pub use proxy::*;
pub use shifted::*;
pub use single::*;
pub use viewport::{SliverViewport, Viewport};
