//! Animation types for Flui.
//!
//! This module provides types for creating animations, including curves for
//! interpolation, tweens for animating values, and animation status types.

pub mod curve;
pub mod status;
pub mod tween;

// Re-exports for convenience
pub use curve::{
    CatmullRomCurve, CatmullRomSpline, Cubic, Curve, Curve2D, Curve2DSample, Curves,
    ElasticInCurve, ElasticInOutCurve, ElasticOutCurve, FlippedCurve, Interval, Linear,
    ParametricCurve, ReverseCurve, SawTooth, Threshold,
};

pub use tween::{
    AlignmentTween, Animatable, BorderRadiusTween, ColorTween, ConstantTween, EdgeInsetsTween,
    FloatTween, IntTween, OffsetTween, RectTween, ReverseTween, SizeTween, StepTween, Tween,
    TweenSequence, TweenSequenceItem,
};

pub use status::{AnimationBehavior, AnimationStatus};
