//! OLE drag and drop, as source and as target (ADR-0038, ADR-0211).
//!
//! - [`ole_model`]: what crosses OLE, decided without it — the clipboard
//!   format and bytes of each representation, `DROPEFFECT` and when the drop
//!   source ends the modal loop; every host checks it;
//! - `ole`: the COM side on Windows — the transport, a window's drop target
//!   and drag source, and the session's modal loop run from the owner window.

mod ole_model;

#[cfg(windows)]
mod ole;

#[cfg(windows)]
pub(crate) use ole::{
    DropTargetRegistration, OleTransfer, Win32DragSource, WindowDropTarget, revoke_target,
};
