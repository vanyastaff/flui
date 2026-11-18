//! The steps of an owner turn that answer the platform's and the
//! application's requests to end something: a window's close, the
//! application's quit and the user's session end (ADR-0259, ADR-0260,
//! ADR-0350).
//!
//! Every host runs them through this module: `flui-app`'s runners and
//! `flui-testing`'s headless host register their presentations with one
//! [`CloseRequestRouter`], keep a quit in a [`QuitRequest`] and answer the
//! session's end through a [`SessionEnd`], so a headless test proves the
//! order a runner keeps. What differs between hosts is what a step does to
//! the host's own state (writing the documents, tearing a runtime down,
//! holding a phone's process), which a host supplies through
//! [`SessionEndHost`]; which steps run, in which order and with which
//! budget is decided here, once.
//!
//! The closes a platform lets an application refuse are
//! [`close_vetoes`]'s, the same on every host.

mod close;
mod quit;
mod session_end;

pub use close::{
    ClosePublication, CloseRequest, CloseRequestError, CloseRequestHandler, CloseRequestRouter,
    CloseResponse, PreparedCloseRequest,
};
pub use quit::{QuitAnswer, QuitRequest, QuitVerdict, SettledStep, ask_quit, settled_steps};
pub use session_end::{
    NoticeDelivery, SESSION_END_USER_CODE_THRESHOLD, SessionEnd, SessionEndHost, SessionEndStep,
    session_end_budget,
};

use flui_platform::SessionEndSupport;
use flui_view::CloseReason;

/// The closes a platform lets the application refuse. `windows_ask` is
/// whether its window system asks before a window closes: a desktop's does,
/// for the user and for the program; Android's, iOS's and the web's never
/// do. `support` is how it lets the session end (`None` for a host with no
/// platform installed): the session's end is refusable only where the
/// platform holds the session for a block (Windows, macOS); one that only
/// delays it (logind) or only tells cannot be refused, so a guard's holds
/// never refuse it there.
#[must_use]
pub fn close_vetoes(windows_ask: bool, support: Option<SessionEndSupport>) -> Vec<CloseReason> {
    let mut vetoes = Vec::with_capacity(3);
    if windows_ask {
        vetoes.extend([CloseReason::User, CloseReason::Program]);
    }
    if support == Some(SessionEndSupport::Hold) {
        vetoes.push(CloseReason::SessionEnd);
    }
    vetoes
}
