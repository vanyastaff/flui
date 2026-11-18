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
pub use quit::{
    QuitAnswer, QuitConsultation, QuitRequest, QuitVerdict, SettledStep, settled_steps,
};
pub use session_end::{NoticeDelivery, SessionEnd, SessionEndHost, SessionEndStep};

use flui_platform::{SessionEndSupport, TargetPlatform};
use flui_view::CloseReason;

/// The closes `platform` lets the application refuse, where it lets the
/// session end with `support` (`None` for a host with no platform
/// installed). Every host builds its UI runtimes and registers its close
/// handlers from this one answer: a runner with the platform it was built for
/// and the support its platform reports, a headless host with the platform
/// its window names.
///
/// A desktop's window system asks before a window closes, for the user and
/// for the program; Android's, iOS's and the browser's never do, so nothing
/// there is refusable but the session's end. The browser is the platform
/// [`TargetPlatform::current`] names `Unknown` on `wasm32`. The session's end
/// is refusable only where the platform holds the session for a block
/// (Windows, macOS); one that only delays it (logind) or only tells cannot be
/// refused, so a guard's holds never refuse it there.
#[must_use]
pub fn close_vetoes(
    platform: TargetPlatform,
    support: Option<SessionEndSupport>,
) -> Vec<CloseReason> {
    let in_browser = cfg!(target_arch = "wasm32") && platform == TargetPlatform::Unknown;
    let windows_ask =
        !in_browser && !matches!(platform, TargetPlatform::Android | TargetPlatform::iOS);
    let mut vetoes = Vec::with_capacity(3);
    if windows_ask {
        vetoes.extend([CloseReason::User, CloseReason::Program]);
    }
    if support == Some(SessionEndSupport::Hold) {
        vetoes.push(CloseReason::SessionEnd);
    }
    vetoes
}
