//! The web's transport (ADR-0288 §6): a page script's connection, through flui-native's page
//! export, holds one conversation, driven on the page's event loop with no thread.
//!
//! Each connection is a task of the page's executor that drives its
//! [`Session`]: a line the script sends is handed to the session at once, which bounds what
//! it holds, and the task polls the session whenever its waker is woken (by a line, an owner's
//! answer, an event, or the session's deadline through the page's timer). It writes what the
//! session transmits to the script with nothing borrowed, since the script's `onLine` is
//! code the application does not own, and may send again from inside it.

#[cfg(target_arch = "wasm32")]
pub(super) mod web;

use std::cell::{Cell, RefCell};
use std::future::poll_fn;
use std::rc::Rc;
use std::task::{Poll, Waker};

use flui_runtime::agent::{Session, Status};
use web_time::Instant;

/// The script's end of a connection, as the conversation writes to it: the page line and the
/// page's timer on the web.
pub(super) trait Peer {
    /// Hand `line`, without its newline, to the script; `false` when it no longer listens.
    fn send(&self, line: &str) -> bool;

    /// End the connection from the application's side.
    fn close(&self);

    /// Poll the conversation again once `deadline` passes, in place of an earlier one; `None`
    /// drops the earlier one. `false` when no timer could be set.
    fn wake_at(&self, deadline: Option<Instant>) -> bool;
}

/// One connection's conversation: its session, and the waker of the task that drives it.
pub(super) struct Conversation {
    /// `None` once the conversation ended.
    session: RefCell<Option<Session>>,
    /// The driving task's waker, as of its last poll.
    waker: RefCell<Option<Waker>>,
    /// A line arrived while the session was running code that sent it: the conversation is
    /// closed as for a client that broke it.
    broken: Cell<bool>,
}

impl Conversation {
    pub(super) fn new(session: Session) -> Rc<Self> {
        Rc::new(Self {
            session: RefCell::new(Some(session)),
            waker: RefCell::new(None),
            broken: Cell::new(false),
        })
    }

    /// The script sent `line`.
    pub(super) fn receive(&self, line: &str) {
        let now = Instant::now();
        match self.session.try_borrow_mut() {
            Ok(mut session) => {
                if let Some(session) = session.as_mut() {
                    let mut bytes = Vec::with_capacity(line.len() + 1);
                    bytes.extend_from_slice(line.as_bytes());
                    bytes.push(b'\n');
                    session.receive(&bytes, now);
                }
            }
            // Only code the session runs (a trace's subscriber) can send while it runs.
            Err(_) => self.broken.set(true),
        }
        self.wake();
    }

    /// The script closed the connection, or the export went.
    pub(super) fn peer_closed(&self) {
        match self.session.try_borrow_mut() {
            Ok(mut session) => {
                if let Some(session) = session.as_mut() {
                    session.peer_closed();
                }
            }
            Err(_) => self.broken.set(true),
        }
        self.wake();
    }

    /// Ask the driving task to poll again.
    pub(super) fn wake(&self) {
        let waker = self.waker.borrow().clone();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Poll the session as the task woken by `waker`, and take what it transmits.
    fn turn(&self, waker: &Waker) -> Turn {
        // Kept before the poll, so a wake during it is not lost; the one it replaces is
        // dropped with nothing borrowed.
        let replaced = self.waker.replace(Some(waker.clone()));
        drop(replaced);
        let mut slot = self.session.borrow_mut();
        let Some(session) = slot.as_mut() else {
            return Turn::Ended;
        };
        if self.broken.replace(false) {
            session.peer_closed();
        }
        session.poll(Instant::now(), waker);
        let lines: Vec<Vec<u8>> = std::iter::from_fn(|| session.transmit()).collect();
        if session.status() == Status::Closed {
            Turn::Closing { lines }
        } else {
            Turn::Open {
                lines,
                deadline: session.next_deadline(),
            }
        }
    }

    /// End the conversation: the session is dropped (when it was the driver's, the driver's
    /// going runs) with nothing of this connection borrowed.
    fn end(&self) {
        let session = self.session.borrow_mut().take();
        drop(session);
        let waker = self.waker.borrow_mut().take();
        drop(waker);
    }
}

/// What one poll of the session left to do.
enum Turn {
    /// Write `lines`, and poll again when woken or at `deadline`.
    Open {
        lines: Vec<Vec<u8>>,
        deadline: Option<Instant>,
    },
    /// Write `lines`, then close the connection.
    Closing { lines: Vec<Vec<u8>> },
    /// The conversation had ended.
    Ended,
}

/// Drive `conversation` over `peer` until it ends: the task a connection runs on the page's
/// executor.
pub(super) async fn converse(conversation: Rc<Conversation>, peer: impl Peer) {
    poll_fn(|ctx| {
        let (lines, deadline) = match conversation.turn(ctx.waker()) {
            Turn::Ended => return Poll::Ready(()),
            Turn::Open { lines, deadline } => (lines, Some(deadline)),
            Turn::Closing { lines } => (lines, None),
        };
        // Written with nothing borrowed: the script may send from inside `onLine`.
        let delivered = lines.iter().all(|line| {
            let line = line.strip_suffix(b"\n").unwrap_or(line);
            peer.send(&String::from_utf8_lossy(line))
        });
        match deadline {
            Some(deadline) if delivered && peer.wake_at(deadline) => Poll::Pending,
            // The script stopped listening, no timer could keep the session's deadline, or
            // the session closed: the connection closes and the conversation ends.
            _ => {
                conversation.end();
                peer.wake_at(None);
                peer.close();
                Poll::Ready(())
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests;
