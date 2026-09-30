//! The typed vocabulary FLUI shares with tests, devtools and agents.
//!
//! Two vocabularies live here, and they are deliberately separate:
//!
//! - [`semantics`]: FLUI's own [`SemanticsRole`] and [`SemanticsAction`],
//!   the accessibility vocabulary. flui-semantics builds its tree
//!   from them and re-exports them.
//! - [`wire`]: the agent-protocol names of ADR-0080 — [`Role`],
//!   [`ActionName`] and [`Checked`] — which an agent reads in every reply and
//!   which the `serde` and `schemars` features serialize and describe.
//!
//! On the wire vocabulary sits the schema of what an agent reads and sends,
//! in ADR-0080's shapes:
//!
//! - [`tree`]: handles ([`ElementId`] `e12`, [`WindowId`] `w3`), a
//!   [`Node`], the [`Tree`] a read returns, the [`ReadQuery`] that scopes it
//!   and the one-line [`outline`];
//! - [`act`]: the [`ActionRequest`] an agent sends;
//! - [`error`]: the fixed [`ErrorCode`]s and [`Retry`];
//! - [`version`]: the [`PROTOCOL_VERSION`] the schema is published under,
//!   versioned apart from this crate, and the rule for changing it.
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

pub mod act;
pub mod error;
pub mod semantics;
pub mod tree;
pub mod version;
pub mod wire;

pub use act::ActionRequest;
pub use error::{ErrorCode, Retry};
pub use semantics::{SemanticsAction, SemanticsRole};
pub use tree::{ElementId, Node, ReadQuery, Rect, Tree, WindowId, outline};
pub use version::{PROTOCOL_VERSION, ProtocolVersion};
pub use wire::{ActionName, Checked, Role};
