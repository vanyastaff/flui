//! Coordinated admission of independently owned property motion.

use super::{Admission, AnimatedValue, Change, Prepared, Terminal};
use crate::{AnimationError, MotionSpec, TwoWayConverter};
use flui_foundation::panic::{PanicRecovery, RecoveryScope};
use std::rc::Rc;

trait Plan {
    fn validate(&self) -> Result<(), AnimationError>;
    fn install(&mut self, recovery: &mut RecoveryScope<'_>);
    fn publish(&mut self, recovery: &mut RecoveryScope<'_>);
    fn retire(&mut self, recovery: &mut RecoveryScope<'_>);
}

struct Retarget<'a, T: TwoWayConverter + 'static> {
    owner: &'a mut AnimatedValue<T>,
    target: Terminal<Rc<T>>,
    motion: Terminal<MotionSpec>,
    prepared: Option<Terminal<Prepared<T::Vector>>>,
    admission: Option<Terminal<Admission<T>>>,
    // Keep converter-owned origin data until the whole update retires.
    _origin: Terminal<Option<T>>,
}

impl<T: TwoWayConverter + 'static> Plan for Retarget<'_, T> {
    fn validate(&self) -> Result<(), AnimationError> {
        let prepared = self
            .prepared
            .as_ref()
            .expect("BUG: a staged update owns prepared motion");
        self.owner
            .driven
            .controller()
            .validate_value_seam(&prepared.seam)
    }

    fn install(&mut self, recovery: &mut RecoveryScope<'_>) {
        let prepared = self
            .prepared
            .take()
            .expect("BUG: a validated update owns prepared motion");
        // Every seam was validated with no intervening callouts. Exclusive
        // owner borrows prevent a participant from appearing twice.
        let admission = self
            .owner
            .install(
                prepared.into_inner(),
                &mut self.target,
                &mut self.motion,
                recovery,
            )
            .expect("BUG: validated grouped motion admits without intervening user code");
        self.admission = Some(Terminal::new(admission));
    }

    fn publish(&mut self, recovery: &mut RecoveryScope<'_>) {
        let admission = self
            .admission
            .take()
            .expect("BUG: admitted motion owns publication");
        let future = admission.into_inner().publish(recovery);
        recovery.retire(future);
    }

    fn retire(&mut self, recovery: &mut RecoveryScope<'_>) {
        if self.admission.is_some() {
            self.publish(recovery);
        }
    }
}

struct Replace<'a, T: TwoWayConverter + 'static> {
    owner: &'a mut Option<AnimatedValue<T>>,
    next: Option<Terminal<AnimatedValue<T>>>,
    outgoing: Option<Terminal<AnimatedValue<T>>>,
    disposal: Option<Terminal<crate::driven::DrivenRetirement>>,
}

struct Dispose<'a, T: TwoWayConverter + 'static> {
    owner: &'a mut AnimatedValue<T>,
    disposal: Option<Terminal<crate::driven::DrivenRetirement>>,
}

impl<T: TwoWayConverter + 'static> Plan for Dispose<'_, T> {
    fn validate(&self) -> Result<(), AnimationError> {
        Ok(())
    }
    fn install(&mut self, _recovery: &mut RecoveryScope<'_>) {
        self.disposal = Some(Terminal::new(self.owner.driven.get_mut().prepare_dispose()));
    }
    fn publish(&mut self, recovery: &mut RecoveryScope<'_>) {
        self.disposal
            .take()
            .expect("BUG: a closed owner has retirement custody")
            .into_inner()
            .publish(recovery);
    }
    fn retire(&mut self, recovery: &mut RecoveryScope<'_>) {
        if self.disposal.is_some() {
            self.publish(recovery);
        }
    }
}

impl<T: TwoWayConverter + 'static> Plan for Replace<'_, T> {
    fn validate(&self) -> Result<(), AnimationError> {
        Ok(())
    }
    fn install(&mut self, _recovery: &mut RecoveryScope<'_>) {
        self.outgoing = std::mem::replace(self.owner, self.next.take().map(Terminal::into_inner))
            .map(Terminal::new);
        if let Some(outgoing) = &mut self.outgoing {
            self.disposal = Some(Terminal::new(
                outgoing.get_mut().driven.get_mut().prepare_dispose(),
            ));
        }
    }
    fn publish(&mut self, recovery: &mut RecoveryScope<'_>) {
        if let Some(disposal) = self.disposal.take() {
            disposal.into_inner().publish(recovery);
        }
        recovery.retire(self.outgoing.take());
    }
    fn retire(&mut self, recovery: &mut RecoveryScope<'_>) {
        if self.disposal.is_some() {
            self.publish(recovery);
        }
        for owner in [&mut self.next, &mut self.outgoing].into_iter().flatten() {
            recovery.run_with(|scope| owner.get_mut().dispose_with_recovery(scope));
        }
    }
}

/// Prepares several independent values before changing any of their runs.
///
/// Use [`Self::run`] to delimit the update. Converters and curves run during
/// preparation; every sampled identity is checked again before installation.
/// Only after all owners are installed can listeners, wakers and outgoing
/// captures run. A refused or interrupted preparation admits no prefix.
pub struct MotionUpdate<'owners> {
    plans: Vec<Terminal<Box<dyn Plan + 'owners>>>,
    refusal: Option<AnimationError>,
}

impl std::fmt::Debug for MotionUpdate<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MotionUpdate")
            .field("refusal", &self.refusal)
            .finish_non_exhaustive()
    }
}

impl<'owners> MotionUpdate<'owners> {
    /// Prepare and apply a coordinated update, then publish its deliveries.
    ///
    /// # Errors
    /// Returns the first preparation refusal, or `ReentrantMotion` when a
    /// later callout invalidates an earlier participant's published seam.
    pub fn run(
        action: impl FnOnce(&mut MotionUpdate<'owners>) -> Result<(), AnimationError>,
    ) -> Result<(), AnimationError> {
        Self::run_with(action, || {})
    }

    /// Run an update with a callback after all owner changes are admitted and
    /// before their deliveries. A consumer can publish associated geometry
    /// here. The callback runs outside borrows; a panic still drains admitted
    /// delivery and propagates after cleanup.
    ///
    /// # Errors
    /// Refused preparation leaves both the owners and callback untouched.
    pub fn run_with(
        action: impl FnOnce(&mut MotionUpdate<'owners>) -> Result<(), AnimationError>,
        admitted: impl FnOnce(),
    ) -> Result<(), AnimationError> {
        let mut recovery = PanicRecovery::new();
        let mut result = Err(AnimationError::AdmissionAborted);
        let mut update = MotionUpdate {
            plans: Vec::new(),
            refusal: None,
        };
        recovery.run_with(|scope| {
            result = action(&mut update);
            if result.is_ok() {
                result = update.commit(scope, admitted);
            }
        });
        for mut plan in update.plans {
            recovery.run_with(|scope| plan.get_mut().retire(scope));
            recovery.retire(plan);
        }
        recovery.finish();
        result
    }

    /// Stage a replacement preserving the last published value and velocity.
    ///
    /// # Errors
    /// Refuses invalid motion; the whole update remains refused even if the
    /// calling closure ignores this error.
    pub fn retarget<T: TwoWayConverter + 'static>(
        &mut self,
        owner: &'owners mut AnimatedValue<T>,
        target: T,
        motion: MotionSpec,
    ) -> Result<(), AnimationError> {
        self.stage(owner, target, motion, None)
    }

    /// Stage a replacement from an explicit resting origin. This is for
    /// scalar progress whose consumer reanchors a non-vector representation.
    /// The origin and new run are committed in one admission.
    ///
    /// # Errors
    /// Refuses a non-finite origin, target or unrepresentable motion.
    pub fn restart<T: TwoWayConverter + 'static>(
        &mut self,
        owner: &'owners mut AnimatedValue<T>,
        origin: T,
        target: T,
        motion: MotionSpec,
    ) -> Result<(), AnimationError> {
        self.stage(owner, target, motion, Some(origin))
    }

    /// Stage appearance or disappearance of an optional property owner.
    /// An outgoing owner retires only after every participant is installed.
    pub fn replace<T: TwoWayConverter + 'static>(
        &mut self,
        owner: &'owners mut Option<AnimatedValue<T>>,
        next: Option<AnimatedValue<T>>,
    ) {
        self.plans.push(Terminal::new(Box::new(Replace {
            owner,
            next: next.map(Terminal::new),
            outgoing: None,
            disposal: None,
        })));
    }

    /// Stage logical closure of a required owner. Its last published value
    /// stays readable; cancellation runs after every group member is closed.
    pub fn dispose<T: TwoWayConverter + 'static>(&mut self, owner: &'owners mut AnimatedValue<T>) {
        self.plans.push(Terminal::new(Box::new(Dispose {
            owner,
            disposal: None,
        })));
    }

    fn stage<T: TwoWayConverter + 'static>(
        &mut self,
        owner: &'owners mut AnimatedValue<T>,
        target: T,
        motion: MotionSpec,
        origin: Option<T>,
    ) -> Result<(), AnimationError> {
        let target = Terminal::new(Rc::new(target));
        let motion = Terminal::new(motion);
        let origin = Terminal::new(origin);
        if let Some(error) = &self.refusal {
            return Err(error.clone());
        }
        self.refusal = Some(AnimationError::AdmissionAborted);
        let origin_vector = origin.get().as_ref().map(TwoWayConverter::to_vector);
        let prepared = match owner.prepare_from(&target, &motion, Change::Animate, origin_vector) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.refusal = Some(error.clone());
                return Err(error);
            }
        };
        self.plans.push(Terminal::new(Box::new(Retarget {
            owner,
            target,
            motion,
            prepared: Some(Terminal::new(prepared)),
            admission: None,
            _origin: origin,
        })));
        self.refusal = None;
        Ok(())
    }

    fn commit(
        &mut self,
        recovery: &mut RecoveryScope<'_>,
        admitted: impl FnOnce(),
    ) -> Result<(), AnimationError> {
        if let Some(error) = &self.refusal {
            return Err(error.clone());
        }
        for plan in &self.plans {
            plan.validate()?;
        }
        for plan in &mut self.plans {
            plan.get_mut().install(recovery);
        }
        recovery.run(admitted);
        for plan in &mut self.plans {
            recovery.run_with(|scope| plan.get_mut().publish(scope));
        }
        Ok(())
    }
}
