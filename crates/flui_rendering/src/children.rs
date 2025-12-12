//! Child storage types for render objects
//!
//! This module provides Flutter-equivalent child storage containers:
//!
//! | Flutter Mixin | Rust Type | Description |
//! |---------------|-----------|-------------|
//! | `RenderObjectWithChildMixin<T>` | `Child<P>` | Single optional child |
//! | `ContainerRenderObjectMixin<T, PD>` | `Children<P, PD>` | Multiple children with parent data |
//! | `SlottedContainerRenderObjectMixin<T, S>` | `Slots<P, S>` | Named slots with offsets |
//!
//! # Protocol Parameterization
//!
//! All types are parameterized by `Protocol` (`P`) to ensure type safety:
//! - `BoxProtocol` - For box layout children
//! - `SliverProtocol` - For sliver layout children
//!
//! # Examples
//!
//! ## Single Child
//!
//! ```rust,ignore
//! use flui_rendering::{Child, BoxChild};
//!
//! struct RenderPadding {
//!     child: BoxChild,  // BoxChild = Child<BoxProtocol>
//!     padding: EdgeInsets,
//! }
//! ```
//!
//! ## Multiple Children
//!
//! ```rust,ignore
//! use flui_rendering::{Children, BoxChildren};
//!
//! #[derive(Clone, Debug)]
//! struct FlexParentData {
//!     flex: f32,
//! }
//!
//! struct RenderFlex {
//!     children: BoxChildren<FlexParentData>,  // BoxChildren<PD> = Children<BoxProtocol, PD>
//!     direction: Axis,
//! }
//! ```
//!
//! ## Named Slots
//!
//! ```rust,ignore
//! use flui_rendering::{Slots, BoxSlots};
//!
//! #[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
//! enum AppBarSlot {
//!     Leading,
//!     Title,
//!     Trailing,
//! }
//!
//! struct RenderAppBar {
//!     slots: BoxSlots<AppBarSlot>,  // BoxSlots<S> = Slots<BoxProtocol, S>
//! }
//! ```

mod child;
mod multi;
mod slots;

// Re-export all types
pub use child::{BoxChild, Child, SliverChild};
pub use multi::{BoxChildren, Children, SliverChildren};
pub use slots::{BoxSlots, SliverSlots, SlotKey, Slots};
