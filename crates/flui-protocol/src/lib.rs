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
//! On the wire vocabulary sits the driver protocol of ADR-0238, in ADR-0080's shapes:
//!
//! - [`method`]: the table of every operation, [`METHODS`], each a sealed [`Method`] with its
//!   wire name, the side that serves it, its effect class, its MCP [`Tier`] and its typed
//!   parameters and output;
//! - [`session`]: the [`Hello`] that opens a connection, launching and attaching, the
//!   target's [`Capabilities`] and its windows;
//! - [`tree`]: handles ([`ElementId`] `e12`, [`WindowId`] `w3`, [`ElementRef`] `w3/e12`), a
//!   [`Node`], the [`Tree`] a read returns at a [`TreeRevision`], the [`ReadQuery`] that
//!   scopes it and the one-line [`outline`];
//! - [`locator`]: the [`Locator`] a caller writes instead of holding a handle, and the
//!   [`Target`] an action takes;
//! - [`act`]: semantics actions, the [`Effect`] an action had and the [`Actionability`]
//!   facts checked before pointer input;
//! - [`input`]: the [`InputPath`], the per-device [`ActionSequence`], the gestures and the
//!   input [`Journal`];
//! - [`snapshot`]: what an action reads back, down to a [`SnapshotDelta`];
//! - [`wait`]: the [`Condition`]s a wait holds out for and its [`WaitOutcome`];
//! - [`events`]: the [`Event`]s a subscriber streams;
//! - [`inspect`]: the owned snapshot of an inspected tree, [`InspectNode`], with typed
//!   [`InspectValue`] fields (ADR-0242);
//! - [`capture`], [`perf`] and [`audit`]: pixels, frame performance and audits, each with
//!   its [`Verdict`] where it judges;
//! - [`error`]: the fixed [`ErrorCode`]s, [`Retry`] and the [`ErrorObject`] that carries them;
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
#[cfg(feature = "serde")]
mod skip;

pub mod act;
pub mod audit;
pub mod capture;
pub mod error;
pub mod events;
pub mod geometry;
pub mod input;
pub mod inspect;
pub mod locator;
pub mod method;
pub mod motion;
pub mod perf;
pub mod semantics;
pub mod session;
pub mod snapshot;
pub mod tree;
pub mod verdict;
pub mod version;
pub mod wait;
pub mod wire;

pub use act::{
    ActOutcome, ActParams, ActionRequest, Actionability, ActionabilityCheck, ActionabilityParams,
    Effect, EffectKind, EffectUnit, JournalSpan,
};
pub use audit::{AuditFinding, AuditParams, AuditReport, AuditRule, NotChecked};
pub use capture::{BlobRef, CaptureMeta, CaptureParams, CaptureSource};
pub use error::{ErrorCode, ErrorObject, HandleKind, Retry};
pub use events::{
    Announcement, AnnouncementEvent, AnnouncementJournal, AnnouncementUrgency, AnnouncementsParams,
    Event, EventFilter, EventKind, FrameEvent, GapEvent, InputEvent, SemanticsEvent,
    SubscribeParams, Subscription, WindowEvent,
};
pub use geometry::{LogicalPoint, LogicalRect, LogicalSize};
pub use input::{
    ActionSequence, DeviceClass, Direction, DragParams, GestureOptions, ImeAction, InputKind,
    InputParams, InputPath, InputPolicy, InputSource, Journal, JournalEntry, JournalParams,
    KeyAction, LongPressParams, PinchParams, PointerAction, PointerButton, PointerDevice,
    PressKeysParams, Released, ScrollIntoViewParams, ScrollUnit, SwipeParams, TapParams,
    TrackpadAction, TypeTextParams, WheelAction,
};
pub use inspect::{
    InspectField, InspectLevel, InspectNode, InspectParams, InspectReading, InspectUnit,
    InspectValue,
};
pub use locator::{LocateParams, Locator, Match, Matches, Pick, StateFilter, Target, TextMatch};
pub use method::{METHODS, Method, MethodEffect, MethodInfo, ServedBy, Tier};
pub use motion::{MotionParams, MotionRequest, MotionState};
pub use perf::{
    Distribution, FramePhase, FrameSample, Latency, PerfBudget, PerfExpectParams, PerfReport,
    PerfStartParams, PerfVerdict, PhaseTime, TargetBudget, TimeLimit,
};
pub use semantics::{SemanticsAction, SemanticsRole};
pub use session::{
    AppInfo, AttachParams, Capabilities, CloseParams, Closed, Empty, Hello, HelloReply, Isolation,
    LaunchParams, Placement, SessionInfo, WindowList, WindowParams,
};
pub use snapshot::{ChangedField, NodeChange, Snapshot, SnapshotDelta, SnapshotReading};
pub use tree::{
    ElementId, ElementRef, Node, ReadParams, ReadQuery, Rect, Tree, TreeDiagnostic, TreeRevision,
    WindowId, outline,
};
pub use verdict::Verdict;
pub use version::{PROTOCOL_VERSION, ProtocolVersion};
pub use wait::{
    Condition, Expectation, FramesParams, Observation, SettledParams, UnsettledCause,
    UnsettledReason, WaitOutcome, WaitParams,
};
pub use wire::{ActionName, Checked, Role};
