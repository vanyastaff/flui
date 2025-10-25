//! # FLUI Core - Typed Render Architecture
//!
//! Clean, typed implementation of the FLUI rendering system based on idea.md.
//!
//! ## Architecture (from idea.md Chapters 2-6)
//!
//! ```text
//! Widget<W: RenderObjectWidget>
//!   ├─ type Render: RenderObject
//!   ├─ create_render_object() -> Self::Render
//!   └─ update_render_object(&mut Self::Render)
//!
//! RenderObject
//!   ├─ type Arity: Arity (Leaf/Single/Multi)
//!   ├─ fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size
//!   └─ fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer
//!
//! LayoutCx<A: Arity>
//!   ├─ LeafArity: only constraints()
//!   ├─ SingleArity: constraints() + child() + layout_child()
//!   └─ MultiArity: constraints() + children() + layout_child()
//!
//! PaintCx<A: Arity>
//!   ├─ LeafArity: only painter(), offset()
//!   ├─ SingleArity: painter(), offset(), child(), capture_child_layer()
//!   └─ MultiArity: painter(), offset(), children(), capture_child_layers()
//! ```
//!
//! ## Key Benefits
//!
//! ### 1. Compile-Time Safety
//!
//! ```rust,ignore
//! // This works - SingleArity has child()
//! impl RenderObject for RenderOpacity {
//!     type Arity = SingleArity;
//!
//!     fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
//!         let child = cx.child(); // ✅ Compiles!
//!         cx.layout_child(child, cx.constraints())
//!     }
//! }
//!
//! // This fails - LeafArity has no child()
//! impl RenderObject for RenderParagraph {
//!     type Arity = LeafArity;
//!
//!     fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
//!         let child = cx.child(); // ❌ Compile error: method not found!
//!         // ...
//!     }
//! }
//! ```
//!
//! ### 2. Zero-Cost Abstractions
//!
//! - No `Box<dyn>` - everything is monomorphized
//! - No `downcast_mut` - types known at compile time
//! - Full inline potential for LLVM
//!
//! ### 3. Integrated with flui_engine
//!
//! ```rust,ignore
//! fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
//!     let mut picture = PictureLayer::new();
//!     picture.draw_rect(self.bounds, self.paint);
//!
//!     if let Some(child) = cx.capture_child_layer() {
//!         let mut container = ContainerLayer::new();
//!         container.add_child(Box::new(picture));
//!         container.add_child(child);
//!         Box::new(container)
//!     } else {
//!         Box::new(picture)
//!     }
//! }
//! ```

// Re-export essential types from dependencies
pub use flui_types::*;
pub use flui_engine::{
    Layer, BoxedLayer,
    Scene, Compositor,
    Painter, Paint,
};

// Core modules
pub mod arity;
pub mod element;
pub mod render;
pub mod widget;


// Re-exports

// Universal Arity system (used across Widget/Element/RenderObject)
pub use arity::{Arity, LeafArity, SingleArity, MultiArity};

pub use render::{
    // RenderObject traits
    RenderObject,
    DynRenderObject,
    BoxedRenderObject,

    // Contexts & Pipeline
    LayoutCx, PaintCx, RenderContext,
    RenderPipeline,

    // Extension traits for arity-specific methods
    SingleChild, MultiChild,
    SingleChildPaint, MultiChildPaint,

    // Cache
    LayoutCache, LayoutCacheKey, LayoutResult,

    // State
    RenderState, RenderFlags,

    // ParentData
    ParentData,
    ParentDataWithOffset,
    BoxParentData,
    ContainerParentData,
    ContainerBoxParentData,
};

pub use widget::{
    Widget,
    DynWidget,
    BoxedWidget,
    WidgetKind,
    ComponentKind,
    StatefulKind,
    InheritedKind,
    ParentDataKind,
    RenderObjectKind,
    StatelessWidget,
    StatefulWidget,
    Stateful,  // Zero-cost wrapper for StatefulWidget
    State,
    InheritedWidget,
    ProxyWidget,
    ParentDataWidget,
    RenderObjectWidget,
};

pub use element::{
    ElementId,
    ElementTree,
    DynElement,
    BoxedElement,
    ElementLifecycle,
    ComponentElement,
    StatefulElement,
    InheritedElement,
    ParentDataElement,
    RenderObjectElement,
    BuildContext,
};

