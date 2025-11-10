//! CompoundAnimation - combines multiple animations with operators.

use crate::animation::{Animation, StatusCallback};
use flui_core::foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_types::animation::AnimationStatus;
use parking_lot::Mutex;
use std::fmt;
use std::sync::Arc;

/// Operator for combining two animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationOperator {
    /// Add the two animation values.
    Add,
    /// Multiply the two animation values.
    Multiply,
    /// Return the minimum of the two values.
    Min,
    /// Return the maximum of the two values.
    Max,
}

/// An animation that combines two animations with an operator.
///
/// This allows mathematical operations on animation values, such as:
/// - Addition: `anim1 + anim2`
/// - Multiplication: `anim1 * anim2`
/// - Min/Max: choosing between two animations
///
/// # Examples
///
/// ```
/// use flui_animation::{CompoundAnimation, AnimationController, AnimationOperator};
/// use flui_core::foundation::SimpleTickerProvider;
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// let ticker_provider = Arc::new(SimpleTickerProvider);
/// let controller1 = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     ticker_provider.clone(),
/// ));
/// let controller2 = Arc::new(AnimationController::new(
///     Duration::from_millis(300),
///     ticker_provider,
/// ));
///
/// controller1.set_value(0.5);
/// controller2.set_value(0.3);
///
/// let compound = CompoundAnimation::new(
///     controller1 as Arc<dyn Animation<f32>>,
///     controller2 as Arc<dyn Animation<f32>>,
///     AnimationOperator::Add,
/// );
///
/// assert_eq!(compound.value(), 0.8);  // 0.5 + 0.3
/// ```
#[derive(Clone)]
pub struct CompoundAnimation {
    first: Arc<dyn Animation<f32>>,
    next: Arc<dyn Animation<f32>>,
    operator: AnimationOperator,
    notifier: Arc<ChangeNotifier>,
    _first_listener_id: Arc<Mutex<Option<ListenerId>>>,
    _next_listener_id: Arc<Mutex<Option<ListenerId>>>,
}

impl CompoundAnimation {
    /// Create a new compound animation.
    ///
    /// # Arguments
    ///
    /// * `first` - The first animation
    /// * `next` - The second animation
    /// * `operator` - The operator to combine them with
    pub fn new(
        first: Arc<dyn Animation<f32>>,
        next: Arc<dyn Animation<f32>>,
        operator: AnimationOperator,
    ) -> Self {
        let notifier = Arc::new(ChangeNotifier::new());

        Self {
            first,
            next,
            operator,
            notifier,
            _first_listener_id: Arc::new(Mutex::new(None)),
            _next_listener_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a compound animation that adds two animations.
    #[must_use]
    pub fn add(first: Arc<dyn Animation<f32>>, next: Arc<dyn Animation<f32>>) -> Self {
        Self::new(first, next, AnimationOperator::Add)
    }

    /// Create a compound animation that multiplies two animations.
    #[must_use]
    pub fn multiply(first: Arc<dyn Animation<f32>>, next: Arc<dyn Animation<f32>>) -> Self {
        Self::new(first, next, AnimationOperator::Multiply)
    }

    /// Create a compound animation that returns the minimum of two animations.
    #[must_use]
    pub fn min(first: Arc<dyn Animation<f32>>, next: Arc<dyn Animation<f32>>) -> Self {
        Self::new(first, next, AnimationOperator::Min)
    }

    /// Create a compound animation that returns the maximum of two animations.
    #[must_use]
    pub fn max(first: Arc<dyn Animation<f32>>, next: Arc<dyn Animation<f32>>) -> Self {
        Self::new(first, next, AnimationOperator::Max)
    }

    /// Apply the operator to two values.
    fn apply_operator(&self, a: f32, b: f32) -> f32 {
        match self.operator {
            AnimationOperator::Add => a + b,
            AnimationOperator::Multiply => a * b,
            AnimationOperator::Min => a.min(b),
            AnimationOperator::Max => a.max(b),
        }
    }
}

impl Animation<f32> for CompoundAnimation {
    fn value(&self) -> f32 {
        let first_value = self.first.value();
        let next_value = self.next.value();
        self.apply_operator(first_value, next_value)
    }

    fn status(&self) -> AnimationStatus {
        // Return the status of the first animation
        // (both animations might have different statuses)
        self.first.status()
    }

    fn add_status_listener(&self, callback: StatusCallback) -> ListenerId {
        // Listen to the first animation's status
        self.first.add_status_listener(callback)
    }

    fn remove_status_listener(&self, id: ListenerId) {
        self.first.remove_status_listener(id)
    }
}

impl Listenable for CompoundAnimation {
    fn add_listener(&mut self, callback: ListenerCallback) -> ListenerId {
        Arc::get_mut(&mut self.notifier)
            .unwrap()
            .add_listener(callback)
    }

    fn remove_listener(&mut self, id: ListenerId) {
        Arc::get_mut(&mut self.notifier)
            .unwrap()
            .remove_listener(id)
    }

    fn remove_all_listeners(&mut self) {
        Arc::get_mut(&mut self.notifier)
            .unwrap()
            .remove_all_listeners()
    }
}

impl fmt::Debug for CompoundAnimation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompoundAnimation")
            .field("value", &self.value())
            .field("operator", &self.operator)
            .field("first_status", &self.first.status())
            .field("next_status", &self.next.status())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AnimationController;
    use flui_core::foundation::SimpleTickerProvider;
    use std::time::Duration;

    fn create_controller(value: f32) -> Arc<AnimationController> {
        let ticker_provider = Arc::new(SimpleTickerProvider);
        let controller = Arc::new(AnimationController::new(
            Duration::from_millis(100),
            ticker_provider,
        ));
        controller.set_value(value);
        controller
    }

    #[test]
    fn test_compound_animation_add() {
        let controller1 = create_controller(0.5);
        let controller2 = create_controller(0.3);

        let compound = CompoundAnimation::add(
            controller1.clone() as Arc<dyn Animation<f32>>,
            controller2.clone() as Arc<dyn Animation<f32>>,
        );

        assert_eq!(compound.value(), 0.8);

        controller1.dispose();
        controller2.dispose();
    }

    #[test]
    fn test_compound_animation_multiply() {
        let controller1 = create_controller(0.5);
        let controller2 = create_controller(0.4);

        let compound = CompoundAnimation::multiply(
            controller1.clone() as Arc<dyn Animation<f32>>,
            controller2.clone() as Arc<dyn Animation<f32>>,
        );

        assert!((compound.value() - 0.2).abs() < 1e-6);

        controller1.dispose();
        controller2.dispose();
    }

    #[test]
    fn test_compound_animation_min() {
        let controller1 = create_controller(0.7);
        let controller2 = create_controller(0.3);

        let compound = CompoundAnimation::min(
            controller1.clone() as Arc<dyn Animation<f32>>,
            controller2.clone() as Arc<dyn Animation<f32>>,
        );

        assert_eq!(compound.value(), 0.3);

        controller1.dispose();
        controller2.dispose();
    }

    #[test]
    fn test_compound_animation_max() {
        let controller1 = create_controller(0.7);
        let controller2 = create_controller(0.3);

        let compound = CompoundAnimation::max(
            controller1.clone() as Arc<dyn Animation<f32>>,
            controller2.clone() as Arc<dyn Animation<f32>>,
        );

        assert_eq!(compound.value(), 0.7);

        controller1.dispose();
        controller2.dispose();
    }

    #[test]
    fn test_compound_animation_status() {
        let controller1 = create_controller(0.5);
        let controller2 = create_controller(0.3);

        let compound = CompoundAnimation::add(
            controller1.clone() as Arc<dyn Animation<f32>>,
            controller2.clone() as Arc<dyn Animation<f32>>,
        );

        assert_eq!(compound.status(), AnimationStatus::Dismissed);

        controller1.forward().unwrap();
        assert_eq!(compound.status(), AnimationStatus::Forward);

        controller1.dispose();
        controller2.dispose();
    }
}
