//! Gesture, focus, keyboard, hit-test, and text-input capabilities for widgets.
//!
//! Ordinary handlers remain on [`crate::widgets::GestureDetector`]. Use these
//! contracts when naming callback payloads or integrating a recognizer with
//! the presentation's [`crate::widgets::GestureArenaScope`]. Lifecycle hooks can
//! also retain the focus, hit-test, and text-input handles provided by
//! [`crate::view::LifecycleContext`], using the callback and result types below.
//! Owners and backend adapter construction remain internal to the runtime.
//!
//! Phase payloads name the callback that delivers them:
//!
//! ```rust
//! use flui::interaction::{
//!     GestureArena, TapGestureRecognizer, TapDownDetails, TapUpDetails,
//!     LongPressGestureRecognizer, LongPressMoveUpdateDetails, LongPressEndDetails,
//! };
//!
//! let arena = GestureArena::new();
//! let tap = TapGestureRecognizer::builder(arena.clone())
//!     .on_tap_down(|details: TapDownDetails| { let _ = details.local_position; })
//!     .on_tap(|details: TapUpDetails| { let _ = details.global_position; })
//!     .build();
//! let press = LongPressGestureRecognizer::builder(arena)
//!     .on_long_press_move_update(|details: LongPressMoveUpdateDetails| {
//!         let _ = details.local_offset_from_origin;
//!     })
//!     .on_long_press_end(|details: LongPressEndDetails| { let _ = details.velocity; })
//!     .build();
//! ```
//!
//! A custom member retains its timer registration and publishes timing after
//! committing contact state. The weak update handle can publish cancellation
//! when the arena settles; the registration keeps that authority live until
//! its owner drops it. One registration covers the member's aggregate timer,
//! including when that member participates in several pointer arenas.
//!
//! ```rust
//! use std::{cell::{Cell, RefCell}, num::NonZeroU64, rc::Rc, time::Duration};
//! use flui::interaction::{
//!     GestureArena, GestureArenaMember, GestureDeadlineError,
//!     GestureDeadlineRegistration, GestureDeadlineUpdate, GestureDisposition, PointerId,
//! };
//!
//! struct TimedMember {
//!     armed: Cell<bool>,
//!     registration: RefCell<Option<GestureDeadlineRegistration>>,
//!     update: RefCell<Option<GestureDeadlineUpdate>>,
//! }
//!
//! impl TimedMember {
//!     fn settle(&self) {
//!         self.armed.set(false);
//!         let update = self.update.borrow().clone();
//!         if let Some(update) = update {
//!             update.publish(None).expect("live timer registration");
//!         }
//!     }
//! }
//!
//! impl GestureArenaMember for TimedMember {
//!     fn accept_gesture(&self, _: PointerId) { self.settle(); }
//!     fn reject_gesture(&self, _: PointerId) { self.settle(); }
//! }
//!
//! let arena = GestureArena::new();
//! let pointer = PointerId::new(NonZeroU64::MIN);
//! let member = Rc::new(TimedMember {
//!     armed: Cell::new(false),
//!     registration: RefCell::new(None),
//!     update: RefCell::new(None),
//! });
//! let deadline_member: Rc<dyn GestureArenaMember> = member.clone();
//! let registration: GestureDeadlineRegistration = arena
//!     .register_deadline_member(pointer, &deadline_member)
//!     .expect("one timer authority");
//! let update = registration.update_handle();
//! *member.registration.borrow_mut() = Some(registration);
//! *member.update.borrow_mut() = Some(update.clone());
//! assert!(matches!(
//!     arena.register_deadline_member(pointer, &deadline_member),
//!     Err(GestureDeadlineError::AlreadyRegistered),
//! ));
//!
//! // Explicit refresh requests a new owner query without calling the member here.
//! update.refresh().expect("fresh discovery revision");
//! member.armed.set(true);
//! update.publish(Some(arena.now() + Duration::from_millis(300)))
//!     .expect("committed timing publication");
//! let entry = arena.add(pointer, &member);
//! // Finish pointer-down membership before explicitly claiming the gesture.
//! arena.close(pointer);
//! entry.resolve(GestureDisposition::Accepted);
//! assert!(!member.armed.get());
//!
//! let retired = member.registration.borrow_mut().take();
//! drop(retired);
//! assert!(matches!(update.publish(None), Err(GestureDeadlineError::Retired)));
//! ```
//!
//! This example covers timer ownership and arena-driven cancellation. A
//! deadline-driven recognizer also implements its own deadline query and poll
//! policy through [`GestureArenaMember`]; runtime publication wakes its owner
//! without requiring a separate manual redraw request.
//!
//! The owning layer's internal runtime bridge is not part of this facade:
//!
//! ```compile_fail,E0432
//! use flui::interaction::__runtime::CloseMode;
//! ```

pub use flui_interaction::events::{
    CursorIcon, PointerButtons, PointerEvent, PointerEventExt, PointerKind,
};
pub use flui_interaction::gesture::DoubleTapDetails;
pub use flui_interaction::gesture::MultiTapDetails;
pub use flui_interaction::gesture::{
    ArenaMembership, BeginContactError, CancelOutcome, ContactId, ContactSnapshot,
    GestureRecognizerState, PrimaryContact, RecognizerSet, cancel_all,
};
pub use flui_interaction::gesture::{
    CompositionBranch, CompositionError, CompositionKind, ContestCancel, GestureBranches,
    GestureComposition, GestureContest, GestureObservation, GestureObserver, MotionRole,
    ObservedComposition, ObservedMember, ObserverHandle, PanClaim, RejectionReason,
    SequencedBranch,
};
pub use flui_interaction::gesture::{
    GestureArena, GestureArenaEntry, GestureArenaMember, GestureDeadlineError,
    GestureDeadlineRegistration, GestureDeadlineUpdate, GestureDisposition, SweepModel,
};
pub use flui_interaction::gesture::{ScaleEndDetails, ScaleStartDetails, ScaleUpdateDetails};
pub use flui_interaction::{
    DoubleTapGestureRecognizer, DragAxis, DragDownDetails, DragEndDetails, DragGestureRecognizer,
    DragStartDetails, DragUpdateDetails, EagerGestureRecognizer, ForcePressGestureRecognizer,
    GestureEndReason, GestureProfiles, GestureRecognizer, GestureSettings, GestureSettingsError,
    HorizontalDragGestureRecognizer, LongPressGestureRecognizer, MultiDragAxis,
    MultiDragEndDetails, MultiDragGestureRecognizer, MultiDragHandle, MultiDragUpdateDetails,
    MultiTapGestureRecognizer, PanGestureRecognizer, PointerId, ScaleGestureRecognizer,
    TapAndDragGestureRecognizer, TapDragDownDetails, TapDragEndDetails, TapDragStartDetails,
    TapDragUpDetails, TapDragUpdateDetails, TapGestureRecognizer, VerticalDragGestureRecognizer,
};
pub use flui_interaction::{
    ForcePressDetails, LongPressCallback, LongPressDetails, LongPressDownCallback,
    LongPressDownDetails, LongPressEndCallback, LongPressEndDetails, LongPressMoveUpdateCallback,
    LongPressMoveUpdateDetails, LongPressSimpleCallback, LongPressStartCallback,
    LongPressStartDetails, TapCallback, TapDetails, TapDownCallback, TapDownDetails, TapUpCallback,
    TapUpDetails, Velocity, VelocityEstimate,
};
pub use flui_interaction::{GestureSettingsProvider, PointerKindSet};
// Arena deadlines are points on the event timeline (ADR-0207 §2).
pub use flui_platform_api::EventTime;
pub use flui_platform_api::pointer::{
    ButtonChange, CancelReason, DeviceId, PanZoomEvent, PanZoomPhase, PanZoomTransform, PenTool,
    PointerButton, PointerInfo, PointerMove, PointerPosition, PointerPress, PointerRelease,
    PointerRole, PointerSample, ScrollDelta, ScrollEvent, ScrollPhase, ScrollPrecision, ScrollUnit,
};

pub use flui_interaction::events::KeyEvent;
pub use flui_interaction::events::keyboard::{Code, Key, KeyState, Location, Modifiers, NamedKey};
pub use flui_interaction::focus::{
    EntryTarget, FocusAttachment, FocusCause, FocusChangeCallback, FocusDetachOutcome,
    FocusDirection, FocusManager, FocusNode, FocusNodeChangeCallback, FocusNodeRegistration,
    FocusRequestOutcome, FocusScopeNode, FocusSubscription, FocusTraversalOverrides,
    FocusTraversalPolicy, FocusTreeError, GlobalKeyRegistration, GlobalKeyRegistrationError,
    KeyEventHandler, KeyEventResult, ReadingOrderPolicy, RectProvider, ResolvedStep,
    TraversalDirection, TraversalEdgeBehavior, TraversalEntry,
};
pub use flui_interaction::ids::FocusNodeId;
pub use flui_interaction::pointer::{
    HitTestEntry, HitTestHandle, HitTestSnapshot, InteractionDispatchError, PointerCapture,
    PointerCaptureError, PointerDispatch,
};
pub use flui_interaction::text_input::{
    ClientToken, DetachOutcome, TextInputClient, TextInputError, TextInputHandle,
};
