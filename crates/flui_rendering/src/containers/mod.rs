//! Type-safe container system for render objects.
//!
//! Containers use Protocol associated types and Arity markers to provide
//! compile-time guarantees about child count and type safety.
//!
//! # Container Types
//!
//! | Container | Use Case | Example |
//! |-----------|----------|---------|
//! | [`Child`] | Single child (0 or 1) | `RenderOpacity`, `RenderClipRect` |
//! | [`ChildList`] | Multiple children with parentData | `RenderFlex`, `RenderStack` |
//! | [`Adapter`] | Cross-protocol wrapper | `BoxToSliver`, `SliverToBox` |
//!
//! # Type Aliases
//!
//! ```rust,ignore
//! // Single child
//! type BoxChild = Child<BoxProtocol, Optional>;      // 0 or 1
//! type BoxChildRequired = Child<BoxProtocol, Exact<1>>;  // exactly 1
//!
//! // Multiple children with parent data
//! type FlexChildren = ChildList<BoxProtocol, Variable, FlexParentData>;
//! type StackChildren = ChildList<BoxProtocol, Variable, StackParentData>;
//!
//! // Cross-protocol adapters
//! type BoxToSliver = Adapter<Child<BoxProtocol>, SliverProtocol>;
//! type SliverToBox = Adapter<Child<SliverProtocol>, BoxProtocol>;
//! ```

mod adapter;
mod children;
mod viewport;

pub use adapter::{
    Adapter, BoxToSliver, MultiBoxToSliver, MultiSliverToBox, OptionalBoxToSliver,
    OptionalSliverToBox, SliverToBox,
};
pub use children::{
    // Box protocol - single child
    BoxChild,
    // Box protocol - multiple children
    BoxChildList,
    BoxChildRequired,

    BoxChildren,

    // Primary container types
    Child,
    ChildEntry,
    ChildList,

    // Layout-specific aliases
    FlexChildren,
    // Helper traits
    HasOffset,
    MultiChildContainer,
    MultiChildContainerWithData,
    SingleChildContainer,
    // Sliver protocol - single child
    SliverChild,
    // Sliver protocol - multiple children
    SliverChildList,
    SliverChildRequired,

    SliverChildren,

    StackChildren,
    WrapChildren,
};
pub use viewport::{SliverViewport, Viewport};
