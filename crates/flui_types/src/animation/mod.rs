//! Animation types for Flui.
//!
//! This module provides types for creating animations, including curves for
//! interpolation, tweens for animating values, and animation status types.

pub mod curve;
pub mod status;
pub mod tween;

// Re-exports for convenience
pub use curve::{
    BounceInCurve, BounceInOutCurve, BounceOutCurve, CatmullRomCurve, CatmullRomSpline, Cubic,
    Curve, Curve2D, Curve2DSample, Curves, DecelerateCurve, ElasticInCurve, ElasticInOutCurve,
    ElasticOutCurve, FlippedCurve, Interval, Linear, ParametricCurve, ReverseCurve, SawTooth,
    Threshold,
};

pub use tween::{
    AlignmentTween, Animatable, AnimatableExt, BorderRadiusTween, ChainedTween, ColorTween,
    ConstantTween, CurveExt, CurveTween, EdgeInsetsTween, FloatTween, IntTween, OffsetTween,
    RectTween, ReverseTween, SizeTween, StepTween, Tween, TweenSequence, TweenSequenceItem,
};

pub use status::{AnimationBehavior, AnimationStatus};
