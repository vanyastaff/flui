//! Type-safe container system for render objects.
//!
//! Containers use Protocol associated types to store protocol-specific children
//! at compile time, eliminating runtime type checks and downcasts.
//!
//! # Container Types
//!
//! ## Children Storage
//!
//! - [`Children`] - Simple children without parent data
//! - [`ChildList`] - Children with per-child parent data
//!
//! ## Single-Child Wrappers
//!
//! - [`Proxy`] - Child where size equals child's size
//! - [`Shifted`] - Child with custom offset positioning
//! - [`Aligning`] - Child with alignment and size factors
//!
//! ## Cross-Protocol
//!
//! - [`Adapter`] - Zero-cost protocol wrapper
//!
//! # Type Aliases
//!
//! ```rust,ignore
//! // Simple children
//! type BoxChild = Children<BoxProtocol, Optional>;
//! type BoxChildren = Children<BoxProtocol, Variable>;
//!
//! // Children with parent data
//! type FlexChildren = ChildList<BoxProtocol, Variable, FlexParentData>;
//! type StackChildren = ChildList<BoxProtocol, Variable, StackParentData>;
//!
//! // Wrappers
//! type ProxyBox = Proxy<BoxProtocol>;
//! type ShiftedBox = Shifted<BoxProtocol>;
//! type AligningBox = Aligning<BoxProtocol>;
//!
//! // Cross-protocol adapters
//! type BoxToSliver = Adapter<BoxChild, SliverProtocol>;
//! type SliverToBox = Adapter<SliverChild, BoxProtocol>;
//! ```

mod adapter;
mod aligning;
mod children;
mod proxy;
mod shifted;
mod viewport;

pub use adapter::{
    Adapter, BoxToSliver, MultiBoxToSliver, MultiSliverToBox, OptionalBoxToSliver,
    OptionalSliverToBox, SliverToBox,
};
pub use aligning::*;
pub use children::{
    // Box protocol aliases
    BoxChild,
    BoxChildList,
    BoxChildRequired,
    BoxChildren,
    ChildList,
    ChildNode,
    // Primary types
    Children,
    // Layout-specific aliases
    FlexChildren,
    HasOffset,
    // Generic multi-child traits
    MultiChildContainer,
    MultiChildContainerWithData,
    // Generic alias
    Single,
    // Sliver protocol aliases
    SliverChild,
    SliverChildRequired,
    SliverChildren,
    StackChildren,
    WrapChildren,
};
// Re-export generic traits from their respective modules
pub use proxy::{ProxyContainer, SingleChildContainer};
pub use shifted::ShiftedContainer;
// Re-export concrete types
pub use proxy::*;
pub use shifted::*;
pub use viewport::{SliverViewport, Viewport};
