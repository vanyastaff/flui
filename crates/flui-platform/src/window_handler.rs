//! What a window asks of the code that opened it (ADR-0254 §4).
//!
//! A window is opened with one [`WindowHandler`], passed to the open call
//! itself, so the handler exists before the window's first event: there is no
//! registration made after the window is open, no event that arrives before
//! one, and no default that drops what the window hands over. It has one method
//! per obligation, not per fact; a fact of the window is read from its
//! [`WindowState`](crate::WindowState) snapshot when
//! [`state_changed`](WindowHandler::state_changed) says it moved.

use crate::{DispatchEventResult, FrameRequest, PlatformInput};

/// What a [`WindowHandler`] answers when its window is asked to close.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(
    clippy::exhaustive_enums,
    reason = "a close request is granted or refused; a host matches both"
)]
pub enum CloseDecision {
    /// The window closes: [`WindowHandler::closed`] follows once it has.
    Close,
    /// The window stays open, as if nothing had asked.
    KeepOpen,
}

/// What a [`WindowHandler`] did with a frame request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FrameOutcome {
    /// A frame ran: whatever was due when it was asked for had its chance.
    Produced,
    /// No frame ran (nothing to run one with yet): the request serviced
    /// nothing, and work due at the same instant is not asked for again
    /// because of it.
    Declined,
    /// The frame ran and failed, and the handler contained the failure
    /// instead of unwinding into the platform (ADR-0258 §4). The platform
    /// counts it as a frame that ran: it had its chance at what was due, and
    /// when the next one comes is the handler's to ask, with the backoff a
    /// frame that keeps failing gets.
    Failed,
}

/// Which way the surface behind a window changes, as
/// [`WindowHandler::surface_changed`] reports it (ADR-0063 decision 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(
    clippy::exhaustive_enums,
    reason = "a surface is released or rebuilt; a renderer matches both"
)]
pub enum SurfaceChange {
    /// The surface is about to become invalid: a handler that renders into it
    /// releases it before the call returns, because the platform takes the
    /// native window away right after.
    Release,
    /// A surface valid for the window's current native handle must exist: a
    /// handler builds one, whatever it held before. It is a request about the
    /// present, not a report about the past, so a handler may receive more of
    /// either than it needs.
    Rebuild,
}

/// The obligations of the code that opened a window, called by the window's
/// host (ADR-0254 §4).
///
/// The host keeps the handler as an `Rc` on the thread that opened the window
/// and calls every method there, outside any lock or borrow it holds, so a method may drive the
/// window, open another one or quit. A method that re-enters the window
/// (a command that closes it, an input that requests a frame) does not run
/// the nested delivery inside itself: it is queued behind the running method
/// and delivered after it returns, in order. A method's panic is contained by
/// the host: what was queued behind it is still delivered, and the first
/// panic resumes afterwards.
///
/// None of the methods has a default body: a handler that does not care for
/// an obligation says so in code, and nothing the window hands over is
/// dropped because a method was left out.
///
/// A window keeps its handler until it has closed: the host drops it after
/// [`closed`](Self::closed), on the owner thread.
pub trait WindowHandler {
    /// A platform input event for this window.
    ///
    /// The first call is the window's capability snapshot,
    /// [`PlatformInput::Capabilities`]; later capability changes arrive in
    /// order with the input around them (ADR-0213). The answer tells the
    /// platform whether the event was taken and whether its own default runs.
    /// An event that arrives while another method of this handler runs is
    /// queued behind it and answered at once with one of the deferred forms
    /// of [`DispatchEventResult`] (ADR-0207): its outcome is not known when the
    /// platform needs the answer.
    fn input(&self, event: PlatformInput) -> DispatchEventResult;

    /// The window published a new [`WindowState`](crate::WindowState): one
    /// call per revision, whichever facts changed, and no payload: the handler
    /// reads the snapshot and compares it with the one it saw last. A host
    /// calls it inside the OS callback that reported the change where the OS
    /// gives one (each step of a Win32 live resize, inside its modal loop).
    fn state_changed(&self);

    /// The platform wants a frame for this window, with what it reported
    /// about the frame (ADR-0257). The answer says whether a frame ran: a
    /// platform that redelivers work due at one instant (a wake deadline)
    /// asks again only after a frame that ran, so a handler with nothing to
    /// run a frame with answers [`FrameOutcome::Declined`] and the platform
    /// does not spin on it.
    fn frame_requested(&self, request: FrameRequest) -> FrameOutcome;

    /// The user or the operating system asks the window to close. The answer
    /// decides: [`CloseDecision::Close`] closes it, and
    /// [`closed`](Self::closed) follows; [`CloseDecision::KeepOpen`] keeps it
    /// (ADR-0123). A close the application itself decided, a
    /// [`WindowCommand::Close`](crate::WindowCommand::Close), is not asked
    /// again. Asked again while it runs, the window answers `KeepOpen` for it.
    fn close_requested(&self) -> CloseDecision;

    /// The window has closed and left its host's tracking: called once, as
    /// the last obligation. The host drops the handler afterwards.
    fn closed(&self);

    /// The surface behind the window is about to become invalid, or a valid
    /// one must exist again.
    ///
    /// It is not the snapshot's focus or execution state under another name:
    /// those change when a window loses focus or is suspended, and the surface
    /// must not be dropped for that. Android releases at `TerminateWindow` and
    /// `Pause` and asks for a rebuild at `InitWindow` and `Resume`; iOS at a
    /// true move to the background and back, never at a passing loss of focus.
    /// A host whose surface never goes away while its window is open never
    /// calls it, which costs nothing; a host that releases must also ask for a
    /// rebuild, or the window stays blank.
    fn surface_changed(&self, change: SurfaceChange);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The handler a window keeps is an `Rc`, so it must be a trait object.
    const _: Option<std::rc::Rc<dyn WindowHandler>> = None;
}
