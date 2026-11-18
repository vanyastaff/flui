//! The screen's compositing manager: whether one owns the screen's
//! `_NET_WM_CM_S<n>` selection (EWMH), and the events that say its owner
//! changed.
//!
//! Under a compositing manager the server reads every redirected window
//! unobscured, so a window tells a covering only while none runs (ADR-0357
//! §1). A manager that starts announces itself with a `MANAGER` message on the
//! root window; one that quits, crashes or gives the selection up announces
//! nothing, so the platform asks XFixes for every change of the selection's
//! owner: a new owner, the owner's window destroyed, its client gone.

use x11rb::connection::RequestConnection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{self, ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{self, ConnectionExt as _};

/// The XFixes version the connection asks for. Selection events are in every
/// version; the extension answers nothing else before a version is asked.
const XFIXES_VERSION: (u32, u32) = (5, 0);

/// The screen's compositing-manager selection and whether its changes of
/// owner are heard.
#[derive(Debug, Clone, Copy)]
pub(super) struct CompositingManager {
    selection: xproto::Atom,
    /// XFixes reports the selection's changes of owner to the root window.
    heard: bool,
}

impl CompositingManager {
    /// The compositing manager of `selection` (`_NET_WM_CM_S<n>`), its every
    /// change of owner heard on `root` from here on where the server has
    /// XFixes. Without XFixes only a manager that starts is heard, by its
    /// `MANAGER` message.
    pub(super) fn watch(
        xcb: &x11rb::xcb_ffi::XCBConnection,
        root: xproto::Window,
        selection: xproto::Atom,
    ) -> Self {
        let present = matches!(
            xcb.extension_information(xfixes::X11_EXTENSION_NAME),
            Ok(Some(_))
        );
        let initialized = present
            && xcb
                .xfixes_query_version(XFIXES_VERSION.0, XFIXES_VERSION.1)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .is_some();
        let heard = initialized
            && xcb
                .xfixes_select_selection_input(
                    root,
                    selection,
                    SelectionEventMask::SET_SELECTION_OWNER
                        | SelectionEventMask::SELECTION_WINDOW_DESTROY
                        | SelectionEventMask::SELECTION_CLIENT_CLOSE,
                )
                .ok()
                .and_then(|cookie| cookie.check().ok())
                .is_some();
        if !heard {
            tracing::debug!(
                "a compositing manager that quits goes unheard: the X server answered no XFixes selection input"
            );
        }
        Self { selection, heard }
    }

    /// Whether a compositing manager owns the selection now. A server that
    /// does not answer reads as none.
    pub(super) fn runs(self, xcb: &x11rb::xcb_ffi::XCBConnection) -> bool {
        xcb.get_selection_owner(self.selection)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.owner != x11rb::NONE)
    }

    /// Whether `event` says the selection's owner changed: a manager took it,
    /// gave it up, or went with its window or its client.
    pub(super) fn changed_by(self, event: &Event) -> bool {
        matches!(event, Event::XfixesSelectionNotify(notify) if self.heard && notify.selection == self.selection)
    }
}
