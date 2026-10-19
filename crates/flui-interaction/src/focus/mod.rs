//! Keyboard focus: the focus tree, its owner and traversal.
//!
//! - [`FocusManager`] - Keyboard focus management for one presentation
//! - [`FocusScopeNode`] - Groups focusable elements for keyboard navigation
//! - [`FocusTraversalPolicy`] - Determines Tab/Shift+Tab navigation order
//!
//! [`FocusManager`] owns keyboard dispatch for the presentation.

mod focus_scope;
mod key_ledger;
mod manager;
mod traversal;

pub use focus_scope::{
    EntryTarget, FocusAttachment, FocusCause, FocusDetachOutcome, FocusNode,
    FocusNodeChangeCallback, FocusNodeRegistration, FocusRequestOutcome, FocusScopeNode,
    FocusSubscription, FocusTraversalPolicy, FocusTreeError, KeyCancelHandler, KeyEventHandler,
    KeyEventResult, NodeContext, ReadingOrderPolicy, RectProvider, ResolvedStep,
    TraversalDirection, TraversalEdgeBehavior, TraversalEntry,
};
pub use manager::{
    FocusChangeCallback, FocusManager, GlobalKeyRegistration, GlobalKeyRegistrationError,
};
pub use traversal::{FocusDirection, FocusTraversalOverrides};
