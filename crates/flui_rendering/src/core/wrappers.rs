//! DEPRECATED: Legacy wrappers for type-erased RenderObject.
//!
//! This module previously contained `BoxRenderWrapper` and `SliverRenderWrapper`
//! that bridged typed `RenderBox<A>` / `SliverRender<A>` to `Box<dyn RenderObject>`.
//!
//! **This is no longer needed.** The new architecture uses:
//! - `RenderObjectWrapper<A, R>` - Generic wrapper that directly holds `R: RenderBox<A>`
//! - `RenderViewObject` trait with generic tree methods (`perform_layout<T: LayoutTree>`)
//!
//! The type erasure through `Box<dyn RenderObject>` has been replaced with
//! compile-time generics for better performance and type safety.
//!
//! # Migration
//!
//! Before:
//! ```rust,ignore
//! let wrapped = BoxRenderWrapper::new(render);
//! let boxed: Box<dyn RenderObject> = Box::new(wrapped);
//! ```
//!
//! After:
//! ```rust,ignore
//! let wrapper = RenderObjectWrapper::new(render, RuntimeArity::Exact(0));
//! // wrapper directly implements RenderViewObject with generic tree methods
//! ```

// Re-export EmptyRender for backward compatibility
pub use super::render_box::EmptyRender;
