//! The acceptance application as an external crate sees it.
//!
//! `tests/facade_consumer.rs` copies this directory into a consumer that
//! depends on `flui` alone, with this file as its library root, and requires
//! every `#[test]` here to report `... ok`. `examples/acceptance_app.rs` runs
//! `app` in a window. Each spec's rows are a module beside `smoke`.

#![cfg_attr(
    test,
    expect(
        deprecated,
        reason = "the rows still drive flui::testing::harness; they move to Tester as it gains their verbs"
    )
)]

pub mod app;

#[cfg(test)]
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
