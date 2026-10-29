//! Particle effects: [`ParticleEmitter`] runs a [`ParticleConfig`] (confetti, sparkle, snow,
//! or the app's own) and [`ParticleController`] bursts it.
//!
//! The simulation runs on the CPU in fixed steps of 1/60 s taken from the frame clock, and a
//! frame draws every live particle with one `draw_atlas` call from a sprite rasterised once.
//! Every value a configuration holds is checked when it is built, and a particle whose
//! position, velocity, size, rotation or sprite transform stops being finite, or whose sprite
//! leaves the range the engine draws in, is removed before it reaches the atlas.

mod atlas;
mod config;
mod emitter;
mod system;

pub use config::{
    Coverage, DEFAULT_CAPACITY, EmitterDuration, EmitterSource, MAX_CAPACITY, ParticleConfig,
    ParticleConfigBuilder, ParticleConfigError, ParticleField, Sprite,
};
pub use emitter::{ParticleController, ParticleEmitter, ParticleEmitterState};
