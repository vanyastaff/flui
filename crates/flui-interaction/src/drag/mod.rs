//! Drag-and-drop sessions (ADR-0211): in-window drags on pointer capture that
//! escalate to the OS, and the receivers that answer them.
//!
//! The source side starts a [`DragSession`] through a [`DragHandle`], with
//! [`DragItem`]s and the [`AllowedActions`] inside and outside the app. The
//! session follows the contact in its window, and over another window of its
//! runtime on the same capture; when the pointer leaves for anything else, and
//! the session may reach beyond it, it asks the host for an OS session. The target side is a [`DropReceiver`], found by hit test, that
//! answers each enter and move with the [`DropFeedback`](flui_platform_api::data_transfer::DropFeedback) the OS wants, and may
//! spring-load ([`SpringLoading`]) or scroll toward a drag near its edge
//! ([`Autoscroll`]) while the drag hovers.
//!
//! A presentation reaches its runtime's drags through [`DragAndDrop`], the
//! lifecycle capability the `Draggable` and `DropTarget` widgets acquire: it
//! starts drags in that presentation's window, shows their state on its
//! cursor, and holds the transfer a keyboard or assistive cut or copy makes
//! until a paste drops it.
//!
//! One [`DragOwner`] belongs to each UI runtime. It holds the session table
//! (`DragSessions`, shared by `Rc<RefCell<..>>` with short borrows), delivers
//! the OS's drag events to the receivers under them, and keeps the Rust value of
//! a drag redeemable only by its own runtime (ADR-0211 §4).

mod context;
mod cursor;
mod item;
mod owner;
mod receiver;
mod sessions;
mod tracker;
mod view;

pub use context::DragAndDrop;
pub use flui_platform_api::drag::{
    AllowedActions, DragError, DragReach, DragSessionKey, OsSessionPlan, ProcessNonce,
};
pub use item::{DragItem, DragRepresentation, LazyPayload};
pub use owner::{
    DragHandle, DragObserver, DragOutcome, DragOwner, DragSession, DragStage, DragStart,
    MoveOutcome,
};
pub use receiver::{Autoscroll, DragPoint, DropReceiver, DropReceiverSlot, SpringLoading};
pub use sessions::PendingOsDrag;
pub use view::{DragFetch, DragIdentity, DragScope, DragView};

/// What starts a drag from a press: at once, or after the contact holds still
/// for a long press. The `Draggable` widget turns it into an arena branch
/// (ADR-0208); the session itself starts when the branch wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum DragTrigger {
    /// The drag begins as soon as the contact moves past the slop.
    #[default]
    Immediate,
    /// The drag begins after a long press (touch lists that also scroll).
    LongPress,
}
