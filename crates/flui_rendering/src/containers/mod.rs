//! Generic container types for child storage
//!
//! This module provides containers with integrated arity support from flui-tree:
//! - `TypedChildren<P, A>` - Protocol-specific child storage with arity
//! - `Single<P, A>` - Single child with arity constraint
//! - `Children<P, PD, A>` - Multiple children with arity constraint
//! - `Proxy<P, A>` - Pass-through container with geometry
//! - `Shifted<P, A>` - Custom offset positioning
//! - `Aligning<P, A>` - Alignment-based positioning
//! - `Adapter<C, ToProtocol>` - Cross-protocol composition

mod typed_children;
mod single;
mod children;
mod proxy;
mod shifted;
mod aligning;
mod adapter;

pub use typed_children::TypedChildren;
pub use single::Single;
pub use children::Children;
pub use proxy::{Proxy, ProxyBox};
pub use shifted::{Shifted, ShiftedBox};
pub use aligning::{Aligning, AligningBox};
pub use adapter::{Adapter, BoxToSliver, SliverToBox, MultiBoxToSliver, MultiSliverToBox};

// Type aliases for common use cases
use crate::parent_data::{BoxParentData, SliverParentData};
use crate::protocol::{BoxProtocol, SliverProtocol};

/// Single Box child container
pub type BoxChild = Single<BoxProtocol>;

/// Single Sliver child container
pub type SliverChild = Single<SliverProtocol>;

/// Multiple Box children container
pub type BoxChildren<PD = BoxParentData> = Children<BoxProtocol, PD>;

/// Multiple Sliver children container
pub type SliverChildren<PD = SliverParentData> = Children<SliverProtocol, PD>;

/// Sliver proxy container
pub type SliverProxy = Proxy<SliverProtocol>;

/// Sliver shifted container
pub type ShiftedSliver = Shifted<SliverProtocol>;

/// Sliver aligning container
pub type AligningSliver = Aligning<SliverProtocol>;
