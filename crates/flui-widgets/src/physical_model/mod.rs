//! Physical-layer widgets — a clipped, shadow-casting, filled surface around
//! a single child, over `flui-objects`' `RenderPhysicalModelBase<C>` family.
//! Layout is a pass-through; only painting is affected. [`PhysicalModel`]
//! clips to a [`flui_types::layout::BoxShape`] (optionally rounded);
//! [`PhysicalShape`] clips to an arbitrary user-supplied `Path`.

mod physical_model;
mod physical_shape;

pub use physical_model::PhysicalModel;
pub use physical_shape::PhysicalShape;
