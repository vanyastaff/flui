//! The winit host's check of Linux's floor (ADR-0244 §3): winit opens the
//! display connection itself and reports a missing `xdg_wm_base` or RandR 1.5
//! as no requirement, so the host asks the display server on a connection of
//! its own first, and reads the answer through `version::requirement`.

use crate::PlatformError;
use crate::version::requirement::{randr_version, wayland_globals};

/// Checks the floor on the display server `environment` names, on a
/// connection of its own that is closed when it returns.
///
/// Only an answer below the floor refuses: a server that cannot be reached,
/// or an environment that names none, is the window system's to report.
pub(crate) fn probe(environment: &super::protocol::Environment) -> Result<(), PlatformError> {
    match super::protocol::Protocol::from_environment(environment) {
        Some(super::protocol::Protocol::Wayland) => probe_wayland(environment),
        Some(super::protocol::Protocol::X11) => probe_x11(),
        None => Ok(()),
    }
}

fn probe_wayland(environment: &super::protocol::Environment) -> Result<(), PlatformError> {
    use wayland_client::{
        Connection, Dispatch, QueueHandle,
        globals::{GlobalListContents, registry_queue_init},
        protocol::wl_registry::{self, WlRegistry},
    };

    /// The registry's events after the first roundtrip are not read.
    struct Globals;

    impl Dispatch<WlRegistry, GlobalListContents> for Globals {
        fn event(
            _: &mut Self,
            _: &WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    let Some(path) = environment.wayland_socket_path() else {
        return Ok(());
    };
    let Ok(stream) = std::os::unix::net::UnixStream::connect(path) else {
        return Ok(());
    };
    let Ok(connection) = Connection::from_socket(stream) else {
        return Ok(());
    };
    let Ok((globals, _queue)) = registry_queue_init::<Globals>(&connection) else {
        return Ok(());
    };
    globals
        .contents()
        .with_list(|list| wayland_globals(list.iter().map(|global| global.interface.as_str())))
}

fn probe_x11() -> Result<(), PlatformError> {
    use x11rb::protocol::randr::ConnectionExt as _;

    let Ok((connection, _screen)) = x11rb::rust_connection::RustConnection::connect(None) else {
        return Ok(());
    };
    randr_version(
        connection
            .randr_query_version(1, 5)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|version| (version.major_version, version.minor_version)),
    )
}
