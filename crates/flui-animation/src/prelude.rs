//! Common property motion, observation and authored mapping interfaces.
//!
//! The same items are available through `flui_sdk::animation::prelude`.
//! Clocks, registration/publication protocols, specialized curves and physics
//! remain explicit imports from the crate root or their domain modules.

pub use crate::{
    Animatable, AnimatedValue, AnimatedValueView, Animation, AnimationController, AnimationError,
    AnimationRunFuture, AnimationStatus, ArcCurve, Curve, CurvedAnimation, DrivenController,
    Keyframes, MotionSpec, RunCanceled, Tween, TwoWayConverter, curves,
};
pub use flui_foundation::Listenable;
