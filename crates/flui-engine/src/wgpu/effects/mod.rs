//! Gradient, shadow, and blur descriptors consumed by the painter's
//! instanced-batch pipelines.
//!
//! The gradient/shadow instance structs are GPU vertex payloads a caller never
//! builds by hand; `GradientStop` and `ShadowParams` are the value types the
//! public painter methods take.

pub(crate) mod blur;
pub(crate) mod gradient;
pub(crate) mod shadow;

pub(crate) use blur::kernel_radius;
pub use gradient::GradientStop;
#[doc(hidden)]
pub use gradient::{LinearGradientInstance, RadialGradientInstance, SweepGradientInstance};
#[doc(hidden)]
pub use shadow::ShadowInstance;
pub use shadow::ShadowParams;
