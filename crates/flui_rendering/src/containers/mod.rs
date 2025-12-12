//! Generic container types for child storage

mod single;
mod children;
mod proxy;
mod shifted;
mod aligning;

pub use single::Single;
pub use children::Children;
pub use proxy::{Proxy, ProxyBox};
pub use shifted::{Shifted, ShiftedBox};
pub use aligning::{Aligning, AligningBox};

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
