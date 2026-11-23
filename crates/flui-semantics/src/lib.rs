//! # FLUI Semantics: the accessibility tree
//!
//! The fifth tree, beside View → Element → Render → Layer: what assistive
//! technology (screen readers, switch and voice control, braille displays)
//! is told about the interface, and the actions it can request.
//!
//! ```text
//! RenderObject::describe_semantics_configuration   (flui-rendering, post-paint)
//!     │  configurations merge into node-forming ancestors
//!     ▼
//! SemanticsTree / SemanticsOwner                    (this crate)
//!     │  flush(): one validated AccessKit TreeUpdate, full or incremental
//!     ▼
//! PlatformAccessibility → AccessKit adapters        (flui-native)
//! ```
//!
//! ## Key types
//!
//! - [`SemanticsConfiguration`]: what a render object says about itself (one
//!   [`SemanticsRole`], typed [`States`], naming, values, relations, actions).
//! - [`SemanticsNode`] and [`SemanticsTree`]: the assembled tree, addressed by
//!   a stable [`AccessibilityNodeId`] derived from the render identity.
//! - [`SemanticsOwner`]: publishes the tree to the platform and resolves the
//!   actions it requests into handler invocations.
//! - [`SemanticsAction`] and [`ActionCall`]: the AccessKit-aligned action
//!   vocabulary, and one action fused with exactly its payload.
//! - [`TreeSnapshot`] and [`WaitFacts`]: the in-process read of the published
//!   tree for ADR-0080 agents.
//! - [`platform`]: the contract a platform accessibility adapter implements.
//!
//! The rules that are not visible in the types live in the crate's
//! `ARCHITECTURE.md`; the cross-crate ones in ADR-0203, ADR-0205 and ADR-0206.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// Lint levels come from `[workspace.lints]` (Cargo.toml `[lints] workspace = true`).
#![deny(missing_docs)]
#![warn(clippy::disallowed_types)]

// Every type has one path: the crate root, except the platform adapter
// contract, which is named from `platform`. The modules stay private so that
// moving a type between them changes no consumer.
mod accesskit_translation;
mod action;
mod action_subject;
mod agent;
mod announce;
mod applicability;
mod assistive_focus;
mod attributes;
mod collection;
mod configuration;
mod document_selection;
mod identity;
mod node;
mod numeric_range;
mod owner;
pub mod platform;
mod properties;
mod reading_order;
mod regions;
mod relation;
mod role;
mod route_change;
mod states;
mod text_content;
mod tree;

pub use accesskit_translation::{
    semantics_action_for, semantics_action_request_for, tree_to_update,
};
pub use action::{
    ActionCall, ActionDecodeError, AssistiveFocusHandler, CustomActionId, Reveal, ScrollUnit,
    SemanticsAction, SemanticsActionHandler, SemanticsActionRequest, TextSelection, UnroutedAction,
};
pub use agent::{
    Evaluation, EventSet, Placement, TreeSnapshot, WaitFacts, WireActionError, WireLocateError,
    WireReadError, counts_frames,
};
pub use announce::{AnnounceError, Announcements, Announcer};
pub use applicability::RoleAttribute;
pub use assistive_focus::{AssistiveFocus, AssistiveFocusChange};
pub use collection::{
    CellPosition, SemanticsCollection, SemanticsCollectionKind, SemanticsCollectionOrigin,
    SemanticsIndexOrigin, SetSize, SortDirection, TableShape,
};
pub use configuration::{DeclaredCustomAction, SemanticsConfiguration};
pub use document_selection::{DocumentPosition, DocumentSelection};
pub use identity::AccessibilityNodeId;
pub use node::SemanticsNode;
pub use numeric_range::{NumericRange, NumericRangeError};
pub use owner::{
    DuplicateIdentifier, InapplicableAttribute, SemanticsActionError, SemanticsActionInvocation,
    SemanticsDiagnostic, SemanticsOwner, SemanticsUpdateCallback, TextRunsWithheld,
    report_diagnostic,
};
pub use properties::{HeadingLevel, SemanticsSortKey};
pub use reading_order::reading_order;
pub use regions::{
    SemanticsIdentityExhausted, SemanticsIdentitySource, SemanticsRegion, SemanticsRegionId,
    SemanticsRegionKey, SemanticsRegions,
};
pub use relation::{SemanticsAnchor, SemanticsRelation};
pub use role::{LiveSetting, SemanticsRole, Urgency};
pub use states::{AutoComplete, Current, HasPopup, Invalid, States, Toggled};
pub use text_content::{
    MAX_RUN_CHARACTERS, TextContent, TextRun, TextRunError, TextRunParts, TextRunPosition,
    TextRunSelection,
};
pub use tree::SemanticsTree;

// `SemanticsUpdateCallback` names `TreeUpdate` in its signature, so a consumer
// implementing that callback must be able to name it without adding accesskit
// itself at a version that must match ours. `NodeId` comes along because
// addressing a published node is the first thing such a consumer does. Named
// items only — a glob would enrol every future accesskit item into this crate's
// contract without anyone deciding to.
pub use accesskit::{NodeId, TreeUpdate};
