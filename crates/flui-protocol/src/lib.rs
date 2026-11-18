//! The typed vocabulary FLUI shares with tests, devtools and agents.
//!
//! Two vocabularies live here, and they are deliberately separate:
//!
//! - [`semantics`]: FLUI's own [`SemanticsRole`](semantics::SemanticsRole) and [`SemanticsAction`](semantics::SemanticsAction),
//!   the accessibility vocabulary. flui-semantics builds its tree
//!   from them and re-exports them.
//! - [`wire`]: the agent-protocol names of ADR-0080 — [`Role`](wire::Role),
//!   [`ActionName`](wire::ActionName) and [`Checked`](wire::Checked) — which an agent reads in every reply and
//!   which the `serde` and `schemars` features serialize and describe.
//!
//! On the wire vocabulary sits the driver protocol of ADR-0238, in ADR-0080's shapes:
//!
//! - [`method`]: the table of every operation, [`METHODS`](method::METHODS), each a sealed [`Method`](method::Method) with its
//!   wire name, the side that serves it, its effect class, its MCP [`Tier`](method::Tier) and its typed
//!   parameters and output;
//! - [`session`]: the [`Hello`](session::Hello) that opens a connection, launching and attaching, the
//!   target's [`Capabilities`](session::Capabilities) and its windows;
//! - [`tree`]: handles ([`ElementId`](tree::ElementId) `e12`, [`WindowId`](tree::WindowId) `w3`, [`ElementRef`](tree::ElementRef) `w3/e12`), a
//!   [`Node`](tree::Node), the [`Tree`](tree::Tree) a read returns at a [`TreeRevision`](tree::TreeRevision), the [`ReadQuery`](tree::ReadQuery) that
//!   scopes it and the one-line [`outline`](tree::outline);
//! - [`locator`]: the [`Locator`](locator::Locator) a caller writes instead of holding a handle, and the
//!   [`Target`](locator::Target) an action takes;
//! - [`act`]: semantics actions, the [`Effect`](act::Effect) an action had and the [`Actionability`](act::Actionability)
//!   facts checked before pointer input;
//! - [`input`]: the [`InputPath`](input::InputPath), the per-device [`ActionSequence`](input::ActionSequence), the gestures and the
//!   input [`Journal`](input::Journal);
//! - [`snapshot`]: what an action reads back, down to a [`SnapshotDelta`](snapshot::SnapshotDelta);
//! - [`wait`]: the [`Condition`](wait::Condition)s a wait holds out for and its [`WaitOutcome`](wait::WaitOutcome);
//! - [`events`]: the [`Event`](events::Event)s a subscriber streams;
//! - [`inspect`]: the owned snapshot of an inspected tree, [`InspectNode`](inspect::InspectNode), with typed
//!   [`InspectValue`](inspect::InspectValue) fields (ADR-0242);
//! - [`capture`], [`perf`] and [`audit`]: pixels, frame performance and audits, each with
//!   its [`Verdict`](verdict::Verdict) where it judges;
//! - [`launch`]: the environment variables and intent extras that tell an app where its agent
//!   listens, and the exit code of an app whose driver went;
//! - [`envelope`]: the lines of a conversation, a client's [`Greeting`](envelope::Greeting) and [`Request`](envelope::Request)s and every
//!   [`ServerLine`](envelope::ServerLine) an app answers with;
//! - [`framing`]: how a byte transport frames lines ([`LineReader`](framing::LineReader)) and binary payloads;
//! - [`error`]: the fixed [`ErrorCode`](error::ErrorCode)s, [`Retry`](error::Retry) and the [`ErrorObject`](error::ErrorObject) that carries them;
//! - [`input_recording`]: the schema of an input recording, the `HostInput` stream
//!   a UI runtime applied with its gesture decisions and preferences (ADR-0215 §2), which
//!   `flui-runtime` converts to and from the event vocabulary and a reader such as the driver
//!   reads without it;
//! - [`native_recording`]: the schema of a native recording, the journal of a
//!   backend's pointer registry before translation (ADR-0215 §4), which the core writes and
//!   a reader such as the driver reads without it;
//! - [`record`]: `record.start` and `record.stop`, a driver's recording of a window's UI runtime
//!   in the input recording's format, sent to it row by row as `record` events;
//! - [`version`]: the [`PROTOCOL_VERSION`](version::PROTOCOL_VERSION) the schema is published under,
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
#[cfg(feature = "serde")]
mod skip;

pub mod act;
pub mod audit;
pub mod capture;
pub mod envelope;
pub mod error;
pub mod events;
pub mod framing;
pub mod geometry;
pub mod input;
pub mod input_recording;
pub mod inspect;
pub mod launch;
pub mod locator;
pub mod method;
pub mod motion;
pub mod native_recording;
pub mod perf;
pub mod record;
pub mod semantics;
pub mod session;
pub mod snapshot;
pub mod tree;
pub mod verdict;
pub mod version;
pub mod wait;
pub mod wire;
