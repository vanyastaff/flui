//! Render pipelines: the per-key variants of the shape pipeline, the set of
//! pipelines a painter owns, the gradient and shadow pipelines, the device
//! they are created on, and the driver's pipeline cache that is stored between
//! runs and warmed on a worker.

pub(crate) mod device;
pub(crate) mod effects;
pub(crate) mod set;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod store;
pub(crate) mod variants;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod warmup;
