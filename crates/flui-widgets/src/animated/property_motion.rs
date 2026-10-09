//! Independently interruptible optional properties sharing one builder channel.

use std::rc::Rc;

use flui_animation::{
    AnimatedValue, AnimatedValueView, AnimationError, MotionSpec, TwoWayConverter, Vsync,
    VsyncRegistrationError,
};
use flui_foundation::{ChangeNotifier, Listenable};

#[derive(Debug)]
pub(super) struct PropertyMotion<T: TwoWayConverter + 'static> {
    owner: Option<AnimatedValue<T>>,
}

impl<T: TwoWayConverter + 'static> PropertyMotion<T> {
    pub(super) fn new(
        initial: Option<T>,
        motion: MotionSpec,
        notifications: &Rc<ChangeNotifier>,
        vsync: Option<&Vsync>,
    ) -> Result<Self, AnimationError> {
        let owner = initial
            .map(|initial| {
                let owner = AnimatedValue::new(initial, motion, vsync)?;
                let weak = Rc::downgrade(notifications);
                owner.animation().add_observer(Rc::new(move |recovery| {
                    if let Some(notifications) = weak.upgrade() {
                        notifications.notify_listeners_with_recovery(recovery);
                    }
                }));
                Ok(owner)
            })
            .transpose()?;
        Ok(Self { owner })
    }

    pub(super) fn rebind(&mut self, vsync: Option<&Vsync>) -> Result<(), VsyncRegistrationError> {
        if let Some(owner) = &mut self.owner {
            owner.rebind(vsync)?;
        }
        Ok(())
    }

    pub(super) fn retarget(
        &mut self,
        target: Option<T>,
        motion: MotionSpec,
        notifications: &Rc<ChangeNotifier>,
        vsync: Option<&Vsync>,
    ) -> Result<(), AnimationError> {
        match (&mut self.owner, target) {
            (Some(owner), Some(target)) => {
                let _run = owner.retarget(target, motion)?;
            }
            (None, Some(target)) => {
                // Appearing properties have no previous value to interpolate.
                *self = Self::new(Some(target), motion, notifications, vsync)?;
            }
            (owner, None) => {
                let removed = owner.take();
                drop(removed);
            }
        }
        Ok(())
    }

    pub(super) fn animation(&self) -> Option<AnimatedValueView<T>> {
        self.owner.as_ref().map(AnimatedValue::animation)
    }

    pub(super) fn dispose(&mut self) {
        if let Some(owner) = &mut self.owner {
            owner.dispose();
        }
    }
}
