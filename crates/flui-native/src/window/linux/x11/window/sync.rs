//! The XSync counters a window names in `_NET_WM_SYNC_REQUEST_COUNTER`, and
//! the extension's initialisation a connection makes before it asks for them.

use x11rb::connection::Connection as _;
use x11rb::protocol::sync::{self as xsync, ConnectionExt as _};

/// The two XSync counters a window names in `_NET_WM_SYNC_REQUEST_COUNTER`,
/// in that order: the one a window manager without frame synchronisation
/// waits on, and the extended one (`protocols`).
#[derive(Debug, Clone, Copy)]
pub(super) struct SyncCounters {
    pub(super) basic: xsync::Counter,
    pub(super) extended: xsync::Counter,
}

impl SyncCounters {
    /// Two counters made on `xcb` at 0, or `None` when the connection has no
    /// ids left or failed. Both ids are taken before either counter is made,
    /// and the first is destroyed again when the second cannot be: none is
    /// left on the server for nobody.
    pub(super) fn create(xcb: &x11rb::xcb_ffi::XCBConnection) -> Option<Self> {
        let zero = xsync::Int64 { hi: 0, lo: 0 };
        let basic = xcb.generate_id().ok()?;
        let extended = xcb.generate_id().ok()?;
        xcb.sync_create_counter(basic, zero).ok()?;
        if xcb.sync_create_counter(extended, zero).is_err() {
            let _: Result<(), _> = xcb
                .sync_destroy_counter(basic)
                .map(x11rb::cookie::VoidCookie::ignore_error);
            return None;
        }
        Some(Self { basic, extended })
    }
}

/// The SYNC version a connection asks for: 3.1, what libXext asks for GDK and
/// Qt. The counters a window makes are in every version.
const SYNC_VERSION: (u8, u8) = (3, 1);

/// Initialise the SYNC extension on `xcb`, as the extension asks before any
/// other of its requests: whether the server has it and answered.
pub(super) fn initialize_sync(xcb: &x11rb::xcb_ffi::XCBConnection) -> bool {
    use x11rb::connection::RequestConnection as _;

    let present = matches!(
        xcb.extension_information(xsync::X11_EXTENSION_NAME),
        Ok(Some(_))
    );
    present
        && xcb
            .sync_initialize(SYNC_VERSION.0, SYNC_VERSION.1)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some()
}
