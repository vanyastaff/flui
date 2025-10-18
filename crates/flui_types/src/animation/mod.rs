//! Animation types for Flui.
//!
//! This module provides types for creating animations, including curves for
//! interpolation, tweens for animating values, and animation status types.

pub mod curve;
pub mod tween;
pub mod status;

// Re-exports for convenience
pub use curve::{
    Curve, ParametricCurve, Curve2D, Curve2DSample,
    Cubic, Linear, SawTooth, Interval, Threshold,
    ElasticInCurve, ElasticOutCurve, ElasticInOutCurve,
    CatmullRomCurve, CatmullRomSpline,
    FlippedCurve, ReverseCurve,
    Curves,
};

pub use tween::{
    Tween, Animatable,
    FloatTween, IntTween, StepTween, ConstantTween, ReverseTween,
    ColorTween, SizeTween, RectTween, OffsetTween,
    AlignmentTween, EdgeInsetsTween, BorderRadiusTween,
    TweenSequence, TweenSequenceItem,
};

pub use status::{AnimationStatus, AnimationBehavior};
