//! Semantics system for accessibility support.
//!
//! This module provides the infrastructure for making FLUI applications
//! accessible to users with disabilities through assistive technologies
//! like screen readers.
//!
//! # Key Types
//!
//! - [`SemanticsNode`]: A node in the semantics tree
//! - [`SemanticsConfiguration`]: Configuration for a semantics node
//! - [`SemanticsAction`]: Actions that can be performed on a node
//! - [`SemanticsFlag`]: Boolean properties of a node
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's semantics system in `rendering/semantics.dart`
//! and `semantics/semantics.dart`.

mod action;
mod configuration;
mod event;
mod node;
mod properties;
mod tree;

pub use action::{ActionArgs, SemanticsAction, SemanticsActionHandler};
pub use configuration::SemanticsConfiguration;
pub use event::{SemanticsEvent, SemanticsEventData, SemanticsEventType};
pub use node::{SemanticsNode, SemanticsNodeData, SemanticsNodeId};
pub use properties::{
    AttributedString, CustomSemanticsAction, SemanticsFlag, SemanticsFlags, SemanticsHintOverrides,
    SemanticsProperties, SemanticsSortKey, SemanticsTag, StringAttribute, TextDirection,
};
pub use tree::{SemanticsOwner, SemanticsUpdate, SemanticsUpdateBuilder};
