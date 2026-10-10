//! Independently interruptible optional properties sharing one builder channel.

use std::rc::Rc;

use flui_animation::{
    AnimatedValue, AnimatedValueView, AnimationError, MotionSpec, MotionUpdate, TwoWayConverter,
    Vsync, VsyncUpdate,
};
use flui_foundation::{ChangeNotifier, Listenable};

pub(super) fn observed_property<T: TwoWayConverter + 'static>(
    initial: T,
    motion: MotionSpec,
    notifications: &Rc<ChangeNotifier>,
    vsync: Option<&Vsync>,
) -> Result<AnimatedValue<T>, AnimationError> {
    let owner = AnimatedValue::new(initial, motion, vsync)?;
    let weak = Rc::downgrade(notifications);
    owner.animation().add_observer(Rc::new(move |recovery| {
        if let Some(notifications) = weak.upgrade() {
            notifications.notify_listeners_with_recovery(recovery);
        }
    }));
    Ok(owner)
}

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
            .map(|initial| observed_property(initial, motion, notifications, vsync))
            .transpose()?;
        Ok(Self { owner })
    }

    pub(super) fn stage_binding<'owners>(
        &'owners mut self,
        update: &mut VsyncUpdate<'owners>,
        vsync: Option<&Vsync>,
    ) {
        if let Some(owner) = &mut self.owner {
            update.rebind(owner, vsync);
        }
    }

    pub(super) fn stage<'owners>(
        &'owners mut self,
        update: &mut MotionUpdate<'owners>,
        target: Option<T>,
        motion: MotionSpec,
        notifications: &Rc<ChangeNotifier>,
        vsync: Option<&Vsync>,
    ) -> Result<(), AnimationError> {
        match (&mut self.owner, target) {
            (Some(owner), Some(target)) => {
                update.retarget(owner, target, motion)?;
            }
            (owner @ None, Some(target)) => {
                // Appearing properties have no previous value to interpolate.
                let next = Self::new(Some(target), motion, notifications, vsync)?;
                update.replace(owner, next.owner);
            }
            (owner, None) => {
                if owner.is_some() {
                    update.replace(owner, None);
                }
            }
        }
        Ok(())
    }

    pub(super) fn animation(&self) -> Option<AnimatedValueView<T>> {
        self.owner.as_ref().map(AnimatedValue::animation)
    }

    pub(super) fn stage_restart<'owners>(
        &'owners mut self,
        update: &mut MotionUpdate<'owners>,
        origin: T,
        target: T,
        motion: MotionSpec,
    ) -> Result<(), AnimationError> {
        let owner = self
            .owner
            .as_mut()
            .expect("BUG: restarting motion has an existing endpoint");
        update.restart(owner, origin, target, motion)
    }

    pub(super) fn stage_disposal<'owners>(&'owners mut self, update: &mut MotionUpdate<'owners>) {
        if self.owner.is_some() {
            update.replace(&mut self.owner, None);
        }
    }
}
