//! A connection to the display server, as the display thread drives it.
//!
//! [`Session`] is what the Wayland and X11 adapters implement: they own the
//! connection, turn what the server sends into the protocol model, and say
//! when the list of outputs changed. The thread that runs a session owns
//! nothing else of the protocol.

use std::io;
use std::os::fd::BorrowedFd;

use rustix::event::{PollFd, PollFlags, poll};
use rustix::io::Errno;

use super::protocol::{Environment, Protocol};
use super::view::OutputView;

/// A display session failed, or could not start.
#[derive(Debug, thiserror::Error)]
pub(crate) enum DisplayError {
    /// No variable names a display server.
    #[error("neither WAYLAND_DISPLAY nor DISPLAY is set, so there is no display server to ask")]
    NoServer,
    /// The protocol the process speaks has no adapter in this build.
    #[cfg(not(all(feature = "wayland", feature = "x11")))]
    #[error("the {protocol} display source is not built; enable the `{feature}` feature")]
    NotBuilt {
        protocol: &'static str,
        feature: &'static str,
    },
    /// The connection could not be made or went away.
    #[error("{protocol} display connection: {message}")]
    Connection {
        protocol: &'static str,
        message: String,
    },
}

impl DisplayError {
    pub(super) fn connection(protocol: &'static str, error: impl std::fmt::Display) -> Self {
        Self::Connection {
            protocol,
            message: error.to_string(),
        }
    }
}

/// What one turn of a session found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The stop descriptor became readable.
    Stopped,
    /// Nothing visible changed; turn again.
    Idle,
    /// The list of outputs changed; read it with [`Session::views`].
    Changed,
}

/// One connection to a display server.
pub(crate) trait Session: Send {
    /// The outputs as of the last turn.
    fn views(&self) -> Vec<OutputView>;

    /// Wait until the server has something to say or `stop` is readable,
    /// handle it, and report. Runs on the display thread only.
    ///
    /// # Errors
    /// [`DisplayError::Connection`] when the connection failed; the session is
    /// finished.
    fn step(&mut self, stop: BorrowedFd<'_>) -> Result<Step, DisplayError>;
}

/// Connect to the server `protocol` names and read its outputs.
///
/// # Errors
/// `DisplayError::NotBuilt` when this build has no adapter for the protocol,
/// [`DisplayError::Connection`] when the connection or the first read fails.
pub(crate) fn connect(
    protocol: Protocol,
    environment: &Environment,
) -> Result<Box<dyn Session>, DisplayError> {
    match protocol {
        #[cfg(feature = "wayland")]
        Protocol::Wayland => Ok(Box::new(super::wayland::WaylandSession::connect(
            environment,
        )?)),
        #[cfg(not(feature = "wayland"))]
        Protocol::Wayland => {
            let _ = environment;
            Err(DisplayError::NotBuilt {
                protocol: "Wayland",
                feature: "wayland",
            })
        }
        #[cfg(feature = "x11")]
        Protocol::X11 => Ok(Box::new(super::x11::X11Session::connect()?)),
        #[cfg(not(feature = "x11"))]
        Protocol::X11 => Err(DisplayError::NotBuilt {
            protocol: "X11",
            feature: "x11",
        }),
    }
}

/// Which of the two descriptors is ready.
pub(super) struct Readiness {
    pub connection: bool,
    pub stop: bool,
}

/// Block until `connection` has data (or hung up) or `stop` is readable.
///
/// # Errors
/// The `poll` failure other than an interrupted call, which is retried.
pub(super) fn wait_readable(
    connection: BorrowedFd<'_>,
    stop: BorrowedFd<'_>,
) -> io::Result<Readiness> {
    let mut fds = [
        PollFd::new(&connection, PollFlags::IN),
        PollFd::new(&stop, PollFlags::IN),
    ];
    loop {
        match poll(&mut fds, None) {
            Ok(_) => break,
            Err(Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
    }
    // A hang-up or an error on the connection is reported by the next read.
    let connection_ready = fds[0]
        .revents()
        .intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR);
    let stop_ready = fds[1]
        .revents()
        .intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR);
    Ok(Readiness {
        connection: connection_ready,
        stop: stop_ready,
    })
}
