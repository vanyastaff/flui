//! Gradient, shadow, and blur descriptors consumed by the painter's
//! instanced-batch pipelines.
//!
//! Nothing here is public: a gradient reaches the engine as a `Shader` on a
//! `Paint` and a shadow as `DrawOp::Shadow`; the batches build these payloads
//! from those.

pub(crate) mod blur;
pub(crate) mod gradient;
pub(crate) mod shadow;

pub(crate) use blur::kernel_radius;
pub(crate) use gradient::GradientStop;
pub(crate) use gradient::{LinearGradientInstance, RadialGradientInstance, SweepGradientInstance};
pub(crate) use shadow::ShadowInstance;
pub(crate) use shadow::ShadowParams;
