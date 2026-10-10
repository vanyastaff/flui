//! Defaults and matrix interpolation for implicit property motion.

use std::time::Duration;

use flui_animation::curve::ArcCurve;
use flui_animation::{Animatable, Curves, Tween};
use flui_foundation::geometry::Matrix4;

/// Default implicit-motion duration.
pub(crate) const DEFAULT_DURATION: Duration = Duration::from_millis(200);

/// Built-in curves compare by value and require no allocation or global cache.
pub(crate) fn default_curve() -> ArcCurve {
    ArcCurve::new(Curves::EaseInOut)
}

/// Optional matrix interpolation, using the decomposition contract of ADR-0149.
/// Replacement re-anchors the displayed matrix; absent endpoints snap.
#[derive(Debug, Clone)]
pub(crate) struct TransformTween {
    tween: Option<Tween<Matrix4>>,
}

impl TransformTween {
    pub(crate) fn at_rest(target: Option<Matrix4>) -> Self {
        Self {
            tween: target.map(|value| Tween::new(value, value)),
        }
    }

    pub(crate) fn current(&self, progress: f64) -> Option<Matrix4> {
        self.tween.as_ref().map(|tween| tween.transform(progress))
    }

    pub(crate) fn animates_toward(&self, target: Option<&Matrix4>) -> bool {
        matches!((target, &self.tween), (Some(target), Some(existing)) if existing.end != *target)
    }

    pub(crate) fn retarget(&mut self, target: Option<Matrix4>, restart_at: Option<f64>) {
        match (target, self.tween.as_ref()) {
            (Some(target), Some(existing)) => {
                if let Some(progress) = restart_at {
                    self.tween = Some(Tween::new(existing.transform(progress), target));
                }
            }
            (Some(target), None) => {
                self.tween = Some(Tween::new(target, target));
            }
            (None, _) => self.tween = None,
        }
    }
}
