//! The typed vocabulary FLUI shares with tests, devtools and agents.
//!
//! Two vocabularies live here, and they are deliberately separate:
//!
//! - [`semantics`]: FLUI's own [`SemanticsRole`] and [`SemanticsAction`],
//!   modelled on Flutter's `dart:ui` enums. flui-semantics builds its tree
//!   from them and re-exports them.
//! - [`wire`]: the agent-protocol names of ADR-0080 — [`Role`],
//!   [`ActionName`] and [`Checked`] — which an agent reads in every reply and
//!   which the `serde` and `schemars` features serialize and describe.
//!
//! Each vocabulary enum is `#[non_exhaustive]` and carries an `ALL` slice
//! generated from the same list as the enum, so a consumer that needs every
//! variant (a mapping test, a list of supported actions) walks `ALL` instead
//! of an exhaustive `match` it could no longer write from outside this crate
//! (ADR-0089 §3).
//!
//! The crate is a contract crate (ADR-0095 §1): no upstream type appears in
//! its signatures and it depends on no runtime, platform or OS crate.

#![deny(missing_docs)]

#[macro_use]
mod vocabulary;

pub mod semantics;
pub mod wire;

pub use semantics::{SemanticsAction, SemanticsRole};
pub use wire::{ActionName, Checked, Role};
