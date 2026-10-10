//! Replayable public frame traces for independent derivative probes.
//! Production values have one owner and cannot be cloned or advanced manually.

use std::time::Duration;

use flui_animation::{
    AnimatedValue as DrivenValue, AnimationError, MotionClock, MotionSpec, SpringDescription,
    TwoWayConverter, Vsync,
};

#[derive(Clone)]
enum Command<T> {
    Target(T),
    Motion(MotionSpec),
    Snap(T),
    Advance(Duration),
}

pub(crate) struct AnimatedValue<T: TwoWayConverter + 'static> {
    value: DrivenValue<T>,
    registry: Vsync,
    clock: MotionClock,
    now: Duration,
    initial: T,
    motion: MotionSpec,
    commands: Vec<Command<T>>,
}

impl<T: TwoWayConverter + 'static> AnimatedValue<T> {
    pub(crate) fn new(initial: T, spring: SpringDescription) -> Result<Self, AnimationError> {
        Self::with_motion(initial, MotionSpec::Spring(spring))
    }

    pub(crate) fn with_motion(initial: T, motion: MotionSpec) -> Result<Self, AnimationError> {
        let registry = Vsync::new();
        let value = DrivenValue::new(initial.clone(), motion.clone(), Some(&registry))?;
        Ok(Self {
            value,
            registry,
            clock: MotionClock::new(),
            now: Duration::ZERO,
            initial,
            motion,
            commands: Vec::new(),
        })
    }

    pub(crate) fn animate_to(&mut self, target: T) -> Result<(), AnimationError> {
        let _run = self.value.animate_to(target.clone())?;
        self.commands.push(Command::Target(target));
        // Admit the fresh run's first frame at the explicit trace timestamp.
        // Mounted tests separately prove that interruptions need no extra anchor.
        self.registry.tick_all(&self.clock.frame(self.now));
        Ok(())
    }

    pub(crate) fn set_motion(&mut self, motion: MotionSpec) -> Result<(), AnimationError> {
        let _run = self.value.set_motion(motion.clone())?;
        self.commands.push(Command::Motion(motion));
        self.registry.tick_all(&self.clock.frame(self.now));
        Ok(())
    }

    pub(crate) fn set_value(&mut self, value: T) -> Result<(), AnimationError> {
        self.value.snap_to(value.clone())?;
        self.commands.push(Command::Snap(value));
        Ok(())
    }

    pub(crate) fn advance(&mut self, dt: Duration) {
        self.now = self.now.saturating_add(dt);
        self.registry.tick_all(&self.clock.frame(self.now));
        // Coalesce only the replay log. The original trace still samples every
        // requested frame; forked probes independently replay its elapsed time.
        if let Some(Command::Advance(elapsed)) = self.commands.last_mut() {
            *elapsed = elapsed.saturating_add(dt);
        } else {
            self.commands.push(Command::Advance(dt));
        }
    }

    pub(crate) fn value(&self) -> T {
        self.value.value()
    }
    pub(crate) fn velocity(&self) -> T::Vector {
        self.value.velocity()
    }
    pub(crate) fn target(&self) -> &T {
        self.value.target()
    }
    pub(crate) fn is_settled(&self) -> bool {
        self.value.is_settled()
    }
}

impl<T: TwoWayConverter + 'static> Clone for AnimatedValue<T> {
    fn clone(&self) -> Self {
        let mut fork = Self::with_motion(self.initial.clone(), self.motion.clone())
            .expect("BUG: an admitted initial trace is replayable");
        for command in &self.commands {
            match command {
                Command::Target(target) => fork
                    .animate_to(target.clone())
                    .expect("BUG: admitted target"),
                Command::Motion(motion) => fork
                    .set_motion(motion.clone())
                    .expect("BUG: admitted motion"),
                Command::Snap(value) => fork.set_value(value.clone()).expect("BUG: admitted snap"),
                Command::Advance(dt) => fork.advance(*dt),
            }
        }
        fork
    }
}
