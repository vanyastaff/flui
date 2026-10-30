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
//! - [`agent`]: the in-process read of the published tree for ADR-0080 agents.
//!
//! The rules that are not visible in the types live in the crate's
//! `ARCHITECTURE.md`; the cross-crate ones in ADR-0203, ADR-0205 and ADR-0206.

// Lint levels come from `[workspace.lints]` (Cargo.toml `[lints] workspace = true`).
#![deny(missing_docs)]

// ============================================================================
// MODULES
// ============================================================================

pub mod agent;
mod applicability;
// Private: the curated entry point is re-exported below. Publishing the module
// would enrol its internal helpers (`to_node`, `resolve_role`) in the crate's
// contract, and a translation detail is not something a consumer should pin.
mod accesskit_translation;
pub mod action;
mod announce;
mod attributes;
mod collection;
pub mod configuration;
pub mod identity;
pub mod node;
mod numeric_range;
pub mod owner;
pub mod platform;
pub mod properties;
pub mod regions;
pub mod relation;
pub mod role;
pub mod states;
mod text_content;
pub mod tree;

// ============================================================================
// RE-EXPORTS - Action Types
// ============================================================================

pub use action::{
    ActionCall, ActionDecodeError, CustomActionId, Reveal, ScrollUnit, SemanticsAction,
    SemanticsActionHandler, SemanticsActionRequest, TextSelection, UnroutedAction,
};
// ============================================================================
// RE-EXPORTS - AccessKit Translation
// ============================================================================
pub use accesskit_translation::{
    semantics_action_for, semantics_action_request_for, tree_to_update,
};
pub use numeric_range::{NumericRange, NumericRangeError};
// ============================================================================
// RE-EXPORTS - Agent read and act (ADR-0095)
// ============================================================================
pub use agent::{Placement, WireActionError, WireLocateError, WireReadError};
// `SemanticsUpdateCallback` names `TreeUpdate` in its signature, so a consumer
// implementing that callback must be able to name it without adding accesskit
// itself at a version that must match ours. `NodeId` comes along because
// addressing a published node is the first thing such a consumer does. Named
// items only — a glob would enrol every future accesskit item into this crate's
// contract without anyone deciding to.
pub use accesskit::{NodeId, TreeUpdate};
// ============================================================================
// RE-EXPORTS - Configuration
// ============================================================================
pub use applicability::RoleAttribute;
pub use configuration::{DeclaredCustomAction, SemanticsConfiguration};
// ============================================================================
// RE-EXPORTS - Collections
// ============================================================================
pub use collection::{CellPosition, SetSize, SortDirection, TableShape};
// ============================================================================
// RE-EXPORTS - Announcements
// ============================================================================
pub use announce::{AnnounceError, Announcements, Announcer};
pub use text_content::{
    MAX_RUN_CHARACTERS, TextContent, TextRun, TextRunError, TextRunParts, TextRunPosition,
    TextRunSelection,
};
// ============================================================================
// RE-EXPORTS - State Types
// ============================================================================
pub use collection::{
    SemanticsCollection, SemanticsCollectionKind, SemanticsCollectionOrigin, SemanticsIndexOrigin,
};
pub use states::{AutoComplete, Current, HasPopup, Invalid, States, Toggled};
// ============================================================================
// RE-EXPORTS - Stable Accessibility Identity
// ============================================================================
pub use identity::AccessibilityNodeId;
pub use regions::{
    SemanticsIdentityExhausted, SemanticsIdentitySource, SemanticsRegion, SemanticsRegionId,
    SemanticsRegionKey, SemanticsRegions,
};
// ============================================================================
// RE-EXPORTS - Foundation Types
// ============================================================================
pub use flui_foundation::SemanticsId;
// ============================================================================
// RE-EXPORTS - Node Types
// ============================================================================
pub use node::SemanticsNode;
// ============================================================================
// RE-EXPORTS - Owner Types
// ============================================================================
pub use owner::{
    DuplicateIdentifier, InapplicableAttribute, SemanticsDiagnostic, TextRunsWithheld,
    report_diagnostic,
};
pub use owner::{
    SemanticsActionError, SemanticsActionInvocation, SemanticsOwner, SemanticsUpdateCallback,
};
// ============================================================================
// RE-EXPORTS - Property Types
// ============================================================================
pub use properties::{
    AttributedString, HeadingLevel, SemanticsHintOverrides, SemanticsSortKey, SemanticsTag,
    StringAttribute, StringAttributeType, TextDirection,
};
// ============================================================================
// RE-EXPORTS - Role Types
// ============================================================================
pub use relation::{SemanticsAnchor, SemanticsRelation};
pub use role::{
    AccessibilityFocusBlockType, DebugSemanticsDumpOrder, LiveSetting, SemanticsRole, Urgency,
};
// ============================================================================
// RE-EXPORTS - Attributes and Tree Types
// ============================================================================
pub use tree::SemanticsTree;

// ============================================================================
// PRELUDE
// ============================================================================

/// The semantics prelude - commonly used types and traits.
///
/// ```rust,ignore
/// use flui_semantics::prelude::*;
/// ```
pub mod prelude {
    // Re-export optimized types
    pub use rustc_hash::{FxHashMap, FxHashSet};
    pub use smallvec::SmallVec;
    pub use smol_str::SmolStr;

    pub use crate::{
        AccessibilityFocusBlockType, AccessibilityNodeId, ActionCall, Announcer, AttributedString,
        DebugSemanticsDumpOrder, HeadingLevel, LiveSetting, NumericRange, NumericRangeError,
        SemanticsAction, SemanticsActionError, SemanticsActionHandler, SemanticsActionInvocation,
        SemanticsActionRequest, SemanticsAnchor, SemanticsConfiguration, SemanticsId,
        SemanticsNode, SemanticsOwner, SemanticsRelation, SemanticsRole, SemanticsTag,
        SemanticsTree, States, TextDirection, Toggled, Urgency,
    };
}

// ============================================================================
// VERSION INFO
// ============================================================================

/// The version of the flui-semantics crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ============================================================================
// TESTS
// ============================================================================
