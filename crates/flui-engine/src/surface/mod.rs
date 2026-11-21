//! What a window slot presents to: the owned window target, the surface lease
//! over it, the factory that makes surfaces, the colour, latency and pacing a
//! surface is configured with, and the acquire-and-present protocol.

pub(crate) mod color;
pub(crate) mod factory;
#[cfg(test)]
mod fake_target;
pub(crate) mod latency;
mod lease;
mod pacing;
pub(crate) mod present;
pub(crate) mod slot;
pub(crate) mod target;
