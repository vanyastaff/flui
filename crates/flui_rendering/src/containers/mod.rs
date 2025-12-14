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
//! ```

mod aligning;
mod children;
mod proxy;
mod shifted;
mod single;

pub use aligning::*;
pub use children::*;
pub use proxy::*;
pub use shifted::*;
pub use single::*;
