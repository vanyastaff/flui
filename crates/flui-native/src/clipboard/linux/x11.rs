//! The X11 host's clipboard: the `CLIPBOARD` selection, on a connection and a
//! thread of its own.
//!
//! An X11 selection is owned by a window, and its owner answers every
//! client that asks for it, whenever it asks: the owner has to be reading
//! its connection all the time it holds the selection, and a reader waits
//! for the owner's answer across several events. The clipboard therefore
//! keeps a connection of its own with an unmapped window on it, read by a
//! thread of its own, as the display source keeps its own for RandR. The
//! loop's connection and its windows never see a selection event, and an
//! owner that answers slowly never holds up a frame.
//!
//! The thread is [`selection::Selections`]: what it owns, the transfers it
//! writes and the read it waits for. The handle here sends it commands and
//! rings its bell; a read answers its resolver from the thread, as Wayland's
//! pipe reads do.

#![forbid(unsafe_code)]

mod selection;

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use flui_platform::{ClipboardRead, ClipboardWrite, Concealment, capability::Resolver};
use rustix::event::{EventfdFlags, eventfd};
use x11rb::rust_connection::RustConnection;

use self::selection::{Command, Selections};
use crate::error::PlatformError;
use crate::host;

/// The X11 clipboard: a handle on the thread that owns and reads the
/// `CLIPBOARD` selection.
pub(crate) struct X11Clipboard {
    commands: Sender<Command>,
    bell: Arc<OwnedFd>,
    thread: parking_lot::Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for X11Clipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X11Clipboard").finish_non_exhaustive()
    }
}

impl X11Clipboard {
    /// Connect to the X server `DISPLAY` names, make the selection window
    /// and start the thread that reads the connection.
    ///
    /// # Errors
    /// [`PlatformError::Init`] when no X server answers, the window or its
    /// atoms cannot be made, or the thread cannot start.
    pub(crate) fn start() -> Result<Self, PlatformError> {
        let init = |message: String| PlatformError::Init { message };
        let (connection, screen) = RustConnection::connect(None)
            .map_err(|error| init(format!("the clipboard cannot reach the X server: {error}")))?;
        let selections = Selections::try_new(connection, screen)
            .map_err(|error| init(format!("the clipboard cannot make its window: {error}")))?;
        let bell = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)
            .map_err(|error| init(format!("cannot create the clipboard's bell: {error}")))?;
        let bell = Arc::new(bell);
        let (commands, received) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("flui-x11-clipboard".into())
            .spawn({
                let bell = Arc::clone(&bell);
                move || selection::run(selections, received, &bell)
            })
            .map_err(|error| init(format!("cannot start the clipboard's thread: {error}")))?;
        Ok(Self {
            commands,
            bell,
            thread: parking_lot::Mutex::new(Some(thread)),
        })
    }

    /// Send `command` and wake the thread for it. A thread that has ended
    /// drops the command, and a read's resolver with it, which answers
    /// nothing rather than waiting forever.
    fn send(&self, command: Command) {
        if self.commands.send(command).is_ok() {
            ring(&self.bell);
        }
    }
}

/// Wake the thread: add one to its bell.
fn ring(bell: &OwnedFd) {
    // A full counter is already readable, which is all a ring is for.
    let _ = rustix::io::write(bell, &1_u64.to_ne_bytes());
}

impl host::NativeClipboard for X11Clipboard {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        self.send(Command::Read(answer));
    }

    fn write(&self, write: ClipboardWrite) {
        self.send(Command::Write(write));
    }

    /// A concealed write is offered with KDE's password-manager hint, which
    /// only a clipboard manager that reads it honours.
    fn concealment(&self) -> Concealment {
        Concealment::HintOnly
    }
}

impl Drop for X11Clipboard {
    /// Stops the thread and waits for it: its connection closes, and with it
    /// the selection the application owned.
    fn drop(&mut self) {
        self.send(Command::Stop);
        if let Some(thread) = self.thread.get_mut().take()
            && let Err(payload) = thread.join()
        {
            // The thread contains its own panics; one that escaped anyway
            // is kept rather than dropped here, where its destructor could
            // fail during an unwind (ADR-0127).
            tracing::error!("the X11 clipboard's thread panicked");
            std::mem::forget(payload);
        }
    }
}

#[cfg(test)]
mod tests;
