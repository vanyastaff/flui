//! The X11 host's clipboard and primary selection: the `CLIPBOARD` and
//! `PRIMARY` selections, on a connection and a thread of its own.
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
//!
//! The application's primary selection is a closure the owner thread alone
//! may call (ADR-0152), so the handle keeps it on that thread and the
//! thread holds only the offer's number. When another client asks for its
//! text, the thread tells the owner through a [`selection::Notice`], which
//! rings the loop; the loop's turn calls [`LoopClipboard::deliver`], which
//! asks the closure with nothing held and sends the text back.

#![forbid(unsafe_code)]

mod selection;

use std::os::fd::OwnedFd;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use flui_platform::capability::Resolver;
use rustix::event::{EventfdFlags, eventfd};
use x11rb::rust_connection::RustConnection;

use self::selection::{Command, Generation, Name, Notice, Selections};
use crate::clipboard::linux::offered::{SelectionText, answer_with, ask, drop_retired};
use crate::error::PlatformError;
use crate::host;
use crate::owner::linux::x11::LoopClipboard;
use crate::owner::slot::OwnerCell;

/// The X11 clipboard: a handle on the thread that owns and reads the
/// `CLIPBOARD` and `PRIMARY` selections.
pub(crate) struct X11Clipboard {
    commands: Sender<Command>,
    bell: Arc<OwnedFd>,
    notice: Arc<Notice>,
    /// The application's primary selection, on the owner thread.
    offer: OwnerCell<Offer>,
    thread: parking_lot::Mutex<Option<JoinHandle<()>>>,
}

/// The application's primary selection, as the owner thread holds it.
#[derive(Default)]
struct Offer {
    /// The offer that is the application's selection, and its closure.
    current: Option<(Generation, SelectionText)>,
    /// The last offer made.
    made: Generation,
    /// The closures of offers another client took the selection from,
    /// dropped at the next turn with nothing held.
    retired: Vec<SelectionText>,
}

impl std::fmt::Debug for X11Clipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X11Clipboard").finish_non_exhaustive()
    }
}

impl X11Clipboard {
    /// Connect to the X server `DISPLAY` names, make the selection window
    /// and start the thread that reads the connection. `wake` rings the
    /// owner's loop when another client asks for the application's primary
    /// selection or takes it; the calling thread is the owner, which alone
    /// offers the primary selection and answers for it.
    ///
    /// # Errors
    /// [`PlatformError::Init`] when no X server answers, the window or its
    /// atoms cannot be made, or the thread cannot start.
    pub(crate) fn start(wake: Box<dyn Fn() + Send + Sync>) -> Result<Self, PlatformError> {
        let init = |message: String| PlatformError::Init { message };
        let (connection, screen) = RustConnection::connect(None)
            .map_err(|error| init(format!("the clipboard cannot reach the X server: {error}")))?;
        let notice = Arc::new(Notice::new(wake));
        let selections = Selections::try_new(connection, screen, Arc::clone(&notice))
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
            notice,
            offer: OwnerCell::new(),
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

    /// The application's closure, while its offer is the primary selection.
    fn current(&self) -> Option<SelectionText> {
        let lost = self.notice.lost();
        self.offer
            .with(|offer| match &offer.current {
                Some((made, text)) if *made > lost => Some(Rc::clone(text)),
                _ => None,
            })
            .flatten()
    }
}

/// The loop's turn answers for the primary selection.
impl LoopClipboard for X11Clipboard {
    /// The owner's turn: drop the closures of offers another client took
    /// the selection from, then answer the thread's request for the current
    /// offer's text, asking the closure with nothing held.
    fn deliver(&self) {
        let lost = self.notice.lost();
        let retired = self
            .offer
            .with(|offer| {
                if offer
                    .current
                    .as_ref()
                    .is_some_and(|(made, _)| *made <= lost)
                    && let Some((_, text)) = offer.current.take()
                {
                    offer.retired.push(text);
                }
                std::mem::take(&mut offer.retired)
            })
            .unwrap_or_default();
        drop_retired(retired);
        let Some(asked) = self.notice.take_asked() else {
            return;
        };
        let text = self
            .offer
            .with(|offer| match &offer.current {
                Some((made, text)) if *made == asked => Some(Rc::clone(text)),
                _ => None,
            })
            .flatten();
        // The requests waiting are answered whatever the closure does: a
        // closure that panics answers them with nothing as it unwinds.
        let unanswered = Unanswered {
            clipboard: self,
            offer: asked,
        };
        let text = text.and_then(ask);
        unanswered.answer(text);
    }
}

/// The thread's request for an offer's text, which is answered with nothing
/// unless [`answer`](Self::answer) answers it.
struct Unanswered<'a> {
    clipboard: &'a X11Clipboard,
    offer: Generation,
}

impl Unanswered<'_> {
    fn answer(self, text: Option<String>) {
        let this = std::mem::ManuallyDrop::new(self);
        this.clipboard.send(Command::Answer(this.offer, text));
    }
}

impl Drop for Unanswered<'_> {
    fn drop(&mut self) {
        self.clipboard.send(Command::Answer(self.offer, None));
    }
}

/// Wake the thread: add one to its bell.
fn ring(bell: &OwnedFd) {
    // A full counter is already readable, which is all a ring is for.
    let _ = rustix::io::write(bell, &1_u64.to_ne_bytes());
}

impl host::NativeClipboard for X11Clipboard {
    fn read_text(&self, answer: Resolver<Option<String>>) {
        self.send(Command::Read(Name::Clipboard, answer));
    }

    fn write_text(&self, text: String) {
        self.send(Command::Write(text));
    }
}

impl host::NativePrimarySelection for X11Clipboard {
    fn offer(&self, text: SelectionText) {
        let made = self.offer.with(|offer| {
            offer.made = offer.made.next();
            (offer.made, offer.current.replace((offer.made, text)))
        });
        let Some((made, replaced)) = made else {
            tracing::error!("the X11 primary selection is offered on the owner thread only");
            return;
        };
        self.send(Command::Offer(made));
        // The closure replaced is dropped with nothing held.
        drop(replaced);
    }

    fn withdraw(&self) {
        let withdrawn = self.offer.with(|offer| offer.current.take()).flatten();
        if let Some((made, text)) = withdrawn {
            self.send(Command::Withdraw(made));
            drop(text);
        }
    }

    fn owns(&self) -> bool {
        self.current().is_some()
    }

    fn read_text(&self, answer: Resolver<Option<String>>) {
        match self.current() {
            // Asked with nothing held: the closure may call back in.
            Some(text) => answer_with(text, answer),
            None => self.send(Command::Read(Name::Primary, answer)),
        }
    }
}

impl Drop for X11Clipboard {
    /// Stops the thread and waits for it: its connection closes, and with it
    /// the selections the application owned. The application's closures
    /// drop after, in one protected sequence (ADR-0127).
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
        let held = self
            .offer
            .with(|offer| {
                let current = offer.current.take().map(|(_, text)| text);
                current
                    .into_iter()
                    .chain(std::mem::take(&mut offer.retired))
                    .collect()
            })
            .unwrap_or_default();
        drop_retired(held);
    }
}

#[cfg(test)]
mod tests;
