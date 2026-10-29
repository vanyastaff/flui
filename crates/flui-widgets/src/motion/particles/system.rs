//! The particle simulation: emission, fixed-step integration and the sprites a frame draws.

use flui_foundation::geometry::{Matrix4, Point, Size};
use flui_painting::styling::Color;

use super::config::{EmitterDuration, ParticleConfig};

/// The length of one integration step, in seconds.
pub(super) const STEP: f64 = 1.0 / 60.0;
/// The longest gap between frames that live particles catch up on, in seconds.
pub(super) const MAX_CATCH_UP: f64 = 0.25;
/// The most steps one frame integrates: the catch-up limit in steps.
const MAX_STEPS: u32 = 15;
/// The largest coordinate the engine's `f32` atlas can take.
const ATLAS_LIMIT: f64 = f32::MAX as f64;

/// One live particle.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Particle {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    size: f64,
    rotation: f64,
    spin: f64,
    age: f64,
    life: f64,
    color: Option<Color>,
}

/// A sprite of one frame: where the atlas draws it and how it is tinted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Placement {
    pub(super) transform: Matrix4,
    pub(super) color: Color,
}

/// Every live particle of one emitter and the clock it has run.
#[derive(Debug)]
pub(super) struct ParticleSystem {
    config: ParticleConfig,
    rng: fastrand::Rng,
    particles: Vec<Particle>,
    /// Seconds the rate has emitted for, for [`EmitterDuration::For`].
    elapsed: f64,
    /// Particles owed by the rate but not yet emitted: the fraction below one.
    owed: f64,
    /// Time not yet integrated: less than one step.
    carry: f64,
    /// The box the emitter last painted, or `None` before its first paint.
    bounds: Option<Size>,
    /// The side, in pixels, of each sprite in the atlas.
    sprite_side: f64,
}

impl ParticleSystem {
    pub(super) fn new(config: ParticleConfig, sprite_side: f64) -> Self {
        let capacity = config.capacity;
        Self {
            rng: fastrand::Rng::with_seed(config.seed),
            config,
            particles: Vec::with_capacity(capacity),
            elapsed: 0.0,
            owed: 0.0,
            carry: 0.0,
            bounds: None,
            sprite_side,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.particles.len()
    }

    /// Whether the rate has nothing more to emit.
    fn finished(&self) -> bool {
        self.config.rate == 0.0
            || match self.config.duration {
                EmitterDuration::Forever => false,
                EmitterDuration::For(duration) => self.elapsed >= duration.as_secs_f64(),
            }
    }

    /// Whether the emitter has nothing to move or emit: its clock may stop.
    pub(super) fn idle(&self) -> bool {
        self.particles.is_empty() && self.finished()
    }

    /// Removes every live particle.
    pub(super) fn clear(&mut self) {
        self.particles.clear();
        self.carry = 0.0;
    }

    pub(super) fn set_bounds(&mut self, size: Size) {
        if size.width.is_finite() && size.height.is_finite() {
            self.bounds = Some(size);
        }
    }

    /// Advances the simulation by `dt` seconds of the frame clock.
    ///
    /// Up to [`MAX_CATCH_UP`] of it is integrated in steps of [`STEP`], so a frame that comes
    /// late after a pause moves the particles exactly as uninterrupted frames would; a longer
    /// gap removes the live particles instead of catching up on decoration. The rate emits
    /// for `dt` only when `emit` is set: a frame that ends a pause passes `false`, so the
    /// rate does not make up the time it missed.
    pub(super) fn advance(&mut self, dt: f64, emit: bool) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        if dt > MAX_CATCH_UP {
            self.clear();
        } else {
            self.carry += dt;
            let mut steps = 0;
            while self.carry >= STEP && steps < MAX_STEPS {
                self.carry -= STEP;
                steps += 1;
                self.step();
            }
            self.carry = self.carry.min(STEP);
        }
        if emit && !self.finished() {
            let window = match self.config.duration {
                EmitterDuration::Forever => dt,
                EmitterDuration::For(duration) => dt.min(duration.as_secs_f64() - self.elapsed),
            };
            self.owed += self.config.rate * window;
            // Truncation is the intent: whole particles now, the fraction later. A huge rate
            // saturates and the capacity limits it.
            let due = self.owed.floor();
            self.owed -= due;
            self.spawn(due as usize, None);
        }
        self.elapsed += dt;
    }

    /// One integration step: gravity, drag, motion, ageing, and the removal of every particle
    /// that died or whose drawn sprite is no longer finite or drawable.
    fn step(&mut self) {
        let gravity = self.config.gravity;
        let damping = (-self.config.drag * STEP).exp();
        for particle in &mut self.particles {
            particle.vx = (particle.vx + gravity.dx * STEP) * damping;
            particle.vy = (particle.vy + gravity.dy * STEP) * damping;
            particle.x += particle.vx * STEP;
            particle.y += particle.vy * STEP;
            particle.rotation += particle.spin * STEP;
            particle.age += STEP;
        }
        let (config, bounds, side) = (&self.config, self.bounds, self.sprite_side);
        self.particles.retain(|particle| {
            particle.age < particle.life && place(config, particle, side, bounds).is_some()
        });
    }

    /// Emits up to `count` particles, at `at` (in logical pixels of the emitter's box) or
    /// from the source, as many as the free capacity allows. Returns how many it emitted.
    pub(super) fn spawn(&mut self, count: usize, at: Option<Point<f64>>) -> usize {
        let free = self.config.capacity.saturating_sub(self.particles.len());
        let count = count.min(free);
        let mut emitted = 0;
        for _ in 0..count {
            let particle = self.new_particle(at);
            if place(&self.config, &particle, self.sprite_side, self.bounds).is_some() {
                self.particles.push(particle);
                emitted += 1;
            }
        }
        emitted
    }

    fn new_particle(&mut self, at: Option<Point<f64>>) -> Particle {
        let config = &self.config;
        let rng = &mut self.rng;
        let position = at.unwrap_or_else(|| {
            let unit = config.source.sample(rng.f64(), rng.f64());
            let size = self.bounds.unwrap_or(Size::new(0.0, 0.0));
            Point::new(unit.x * size.width, unit.y * size.height)
        });
        let speed = config.speed.at(rng.f64());
        let (sin, cos) = config.direction.at(rng.f64()).sin_cos();
        let color = config
            .palette
            .as_ref()
            .map(|palette| palette[rng.usize(..palette.len())]);
        Particle {
            x: position.x,
            y: position.y,
            vx: speed * cos,
            vy: speed * sin,
            size: config.size.at(rng.f64()),
            rotation: config.rotation.at(rng.f64()),
            spin: config.spin.at(rng.f64()),
            age: 0.0,
            life: config.lifetime.at(rng.f64()),
            color,
        }
    }

    /// The sprites of this frame, oldest first.
    pub(super) fn placements(&self) -> Vec<Placement> {
        self.particles
            .iter()
            .filter_map(|particle| place(&self.config, particle, self.sprite_side, self.bounds))
            .collect()
    }
}

/// Where and how `particle` is drawn, or `None` when anything that reaches the atlas is not
/// finite, a corner leaves the `f32` range the engine draws in, or the sprite lies farther
/// than its own size outside `bounds`.
fn place(
    config: &ParticleConfig,
    particle: &Particle,
    side: f64,
    bounds: Option<Size>,
) -> Option<Placement> {
    let life = (particle.age / particle.life).clamp(0.0, 1.0);
    let lerp = |(start, end): (f64, f64)| start + (end - start) * life;
    let drawn = particle.size * lerp(config.scale);
    let (sin, cos) = particle.rotation.sin_cos();
    let values = [
        particle.x,
        particle.y,
        particle.vx,
        particle.vy,
        particle.rotation,
        particle.spin,
        drawn,
        sin,
        cos,
    ];
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let mut transform = Matrix4::translation(-side / 2.0, -side / 2.0, 0.0);
    let scale = drawn / side;
    transform.scale(scale, scale, 1.0);
    transform.rotate_z(particle.rotation);
    transform.translate(particle.x, particle.y, 0.0);
    if transform.m.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let reach = drawn.max(0.0);
    for (cx, cy) in [(0.0, 0.0), (side, 0.0), (0.0, side), (side, side)] {
        let (x, y) = transform.transform_point(cx, cy);
        if !x.is_finite() || !y.is_finite() || x.abs() > ATLAS_LIMIT || y.abs() > ATLAS_LIMIT {
            return None;
        }
        if let Some(bounds) = bounds
            && (x < -reach || y < -reach || x > bounds.width + reach || y > bounds.height + reach)
        {
            return None;
        }
    }
    let tint = particle
        .color
        .unwrap_or_else(|| Color::lerp(config.color.0, config.color.1, life));
    Some(Placement {
        transform,
        color: tint.with_opacity(f64::from(tint.opacity()) * lerp(config.opacity)),
    })
}
