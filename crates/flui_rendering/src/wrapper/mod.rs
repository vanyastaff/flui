//! Wrapper types that bridge typed render traits to `RenderObject`.
//!
//! This module provides wrapper types that convert protocol-specific render traits
//! (`RenderBox`, `RenderSliver`) into `RenderObject` for storage in `RenderTree`.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     User Implements                             │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  struct MyWidget { ... }                                        │
//! │  impl RenderBox for MyWidget { ... }     ← BoxProtocol          │
//! │                                                                 │
//! │  struct MySliver { ... }                                        │
//! │  impl RenderSliver for MySliver { ... }  ← SliverProtocol       │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     Wrapped for Storage                         │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  BoxWrapper<MyWidget>     → impl RenderObject                   │
//! │  SliverWrapper<MySliver>  → impl RenderObject                   │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     Stored in RenderTree                        │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Box<dyn RenderObject>                                          │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Wrapper Types
//!
//! | Wrapper | Trait | Protocol | Layout |
//! |---------|-------|----------|--------|
//! | [`BoxWrapper<T>`] | `RenderBox` | `BoxProtocol` | Size-based (width × height) |
//! | [`SliverWrapper<T>`] | `RenderSliver` | `SliverProtocol` | Scroll-aware (extent-based) |
//!
//! # Usage
//!
//! ## BoxWrapper for Box Layout
//!
//! ```ignore
//! use flui_rendering::wrapper::BoxWrapper;
//! use flui_rendering::traits::RenderBox;
//!
//! struct ColoredBox {
//!     color: Color,
//!     size: Size,
//! }
//!
//! impl RenderBox for ColoredBox {
//!     type Arity = Leaf;
//!     type ParentData = BoxParentData;
//!
//!     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Leaf, BoxParentData>) {
//!         let size = ctx.constraints().constrain(self.size);
//!         ctx.complete_with_size(size);
//!     }
//!     // ...
//! }
//!
//! // Wrap for RenderTree storage
//! let wrapper = BoxWrapper::new(ColoredBox { color, size });
//! let render_object: Box<dyn RenderObject> = Box::new(wrapper);
//! ```
//!
//! ## SliverWrapper for Sliver Layout
//!
//! ```ignore
//! use flui_rendering::wrapper::SliverWrapper;
//! use flui_rendering::traits::RenderSliver;
//!
//! struct SliverList {
//!     items: Vec<Item>,
//! }
//!
//! impl RenderSliver for SliverList {
//!     type Arity = Variable;
//!     type ParentData = SliverMultiBoxAdaptorParentData;
//!
//!     fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<Variable, _>) {
//!         let geometry = SliverGeometry {
//!             scroll_extent: self.total_extent(),
//!             paint_extent: ctx.constraints().remaining_paint_extent,
//!             // ...
//!         };
//!         ctx.complete(geometry);
//!     }
//!     // ...
//! }
//!
//! // Wrap for RenderTree storage
//! let wrapper = SliverWrapper::new(SliverList { items });
//! let render_object: Box<dyn RenderObject> = Box::new(wrapper);
//! ```

mod box_wrapper;
mod sliver_wrapper;

pub use box_wrapper::BoxWrapper;
pub use sliver_wrapper::SliverWrapper;
