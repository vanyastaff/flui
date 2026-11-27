//! The acceptance application as an external crate sees it.
//!
//! `tests/facade_consumer.rs` copies this directory into a consumer that
//! depends on `flui` alone, with this file as its library root, and requires
//! every `#[test]` here to report `... ok`. `examples/acceptance_app.rs` runs
//! `app` in a window. Each spec's rows are a module beside `smoke`.

pub mod app;

#[cfg(test)]
#[expect(
    deprecated,
    reason = "ADR-0400: the run reads locators, the inspection tree and focus, which the scenario tester does not expose yet (docs/gaps.md)"
)]
pub mod testing;

#[cfg(test)]
mod smoke;

#[cfg(test)]
mod keyboard;

#[cfg(test)]
mod teardown;

#[cfg(test)]
mod persistence;

#[cfg(test)]
mod ime;
