//! The GPU device the engine draws on: which backend and adapter it asks for,
//! the probe that tries them, the domain that admits and retires work against
//! the device, the worker that requests it off the executor's thread, and the
//! timestamps that time a frame on it.

pub(crate) mod adapter;
pub(crate) mod backend;
pub(crate) mod domain;
pub(crate) mod probe;
pub(crate) mod timer;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod worker;
