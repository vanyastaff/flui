//! Interaction widgets — modify how a subtree participates in hit-testing and
//! visibility without changing its appearance. Each is a thin
//! [`RenderView`](flui_view::prelude::RenderView) over a `flui-objects` proxy.

mod absorb_pointer;
pub(crate) mod actions;
mod draggable;
mod drop_target;
pub(crate) mod focus;
pub(crate) mod focus_reveal;
mod focus_ring;
mod focus_traversal_group;
mod gesture_alternatives;
mod gesture_arena_scope;
mod gesture_detector;
mod hit_slop;
mod ignore_pointer;
mod listener;
mod meta_data;
mod mouse_region;
mod offstage;
mod pan_claim_region;
mod raw_button;
pub(crate) mod raw_gesture_detector;
pub(crate) mod recognizer_attachment;
mod shortcuts;
mod system_gesture_exclusion;
mod transformation_controller;
mod visibility;
mod visibility_gate;

pub use absorb_pointer::AbsorbPointer;
pub use actions::{
    Action, ActionOutcome, Actions, ActivateIntent, ActivationPressIntent, BackIntent,
    ButtonActivateIntent, CallbackAction, CopySelectionTextIntent, DirectionalFocusAction,
    DirectionalFocusIntent, DismissIntent, Intent, NextFocusAction, NextFocusIntent,
    PasteTextIntent, PreviousFocusAction, PreviousFocusIntent, SelectAllTextIntent,
    ShowContextMenuIntent,
};
pub use draggable::{DragAnchor, DragCommands, Draggable, DraggableState};
pub use drop_target::{DropCommands, DropHover, DropTarget, DropTargetState};
pub use focus::{
    ExcludeFocus, Focus, FocusChangeHandler, FocusRoot, FocusRootState, FocusScope,
    FocusScopeState, FocusState,
};
pub use focus_ring::{DefaultFocusRingStyle, FocusRing, FocusRingStyle};
pub use focus_traversal_group::{FocusTraversalGroup, FocusTraversalGroupState};
pub use gesture_alternatives::{
    Alternative, AlternativeKind, CommandAppearance, CommandHandle, ControlPlacement, ControlSpec,
    GestureAlternatives, GestureAlternativesError, GestureAlternativesState, GestureCommand,
    MenuCause, MenuItem, MenuPresenter, MenuRequest, MenuSpec,
};
pub use gesture_arena_scope::GestureArenaScope;
pub use gesture_detector::{
    DragSlot, ForcePressSlot, GestureDetector, GestureDetectorState, MultiTapCount,
    MultiTapCountError, TransformSlot,
};
pub use hit_slop::{ContentShape, HitSlop};
pub use ignore_pointer::IgnorePointer;
pub use listener::Listener;
pub use meta_data::MetaData;
pub use mouse_region::MouseRegion;
pub use offstage::Offstage;
pub use pan_claim_region::PanClaimRegion;
pub use raw_button::RawButton;
pub use raw_gesture_detector::{
    ContactAdmission, GestureRecognizerFactory, RawGestureDetector, RawGestureDetectorState,
    RecognizerContext,
};
pub use shortcuts::{
    CallbackShortcuts, DefaultFocusTraversal, DefaultFocusTraversalState, ShortcutCallback,
    Shortcuts, ShortcutsState, SingleActivator,
};
pub use system_gesture_exclusion::SystemGestureExclusion;
pub use transformation_controller::TransformationController;
pub use visibility::Visibility;
pub use visibility_gate::VisibilityGate;
