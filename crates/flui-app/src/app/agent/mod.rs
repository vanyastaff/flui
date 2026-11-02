//! The driver agent (ADR-0239): every debug FLUI app, and every build with
//! the `driver` feature, serves its windows to a driver over a local
//! endpoint, with no code in the application.
//!
//! The runner installs it beside the application's own development hooks
//! when the launch names an endpoint ([`Launch::from_env`]); with none,
//! nothing is created: no thread, no socket. The agent is a
//! [`DevAgentHook`]: the host attaches it when the loop starts and hands it
//! each window with content, and the agent answers drivers from threads of
//! its own. Every operation goes through the window's owner inbox, so
//! nothing here touches a tree: the owner answers at its next drain, a frame
//! boundary, and the agent only waits for the answer, a bounded time.
//!
//! # The endpoint
//!
//! A named pipe on Windows (`\\.\pipe\<name>`, the current user's only,
//! remote clients refused) and a Unix domain socket elsewhere (a path whose
//! parent directory the current user owns with mode `0700`; the socket file
//! is removed when the agent stops). No TCP port. A bind failure is logged,
//! and the application keeps running without an agent.
//!
//! The token keeps out other users and remote callers. It does not keep out
//! other processes of the same user, which can read the application's
//! environment just as they can reach the endpoint.
//!
//! # The conversation
//!
//! Protocol [`PROTOCOL_VERSION`](flui_protocol::version::PROTOCOL_VERSION)
//! (ADR-0238): newline-delimited JSON lines of at most
//! [`MAX_LINE`](flui_protocol::framing::MAX_LINE) bytes.
//!
//! ```text
//! → {"hello":{"token":"…","protocol":"0.8","client":"flui-driver/0.1.0"}}
//! ← {"hello":{"protocol":"0.8","app":{…},"capabilities":{…}}}
//! → {"id":7,"method":"semantics.locate","params":{…}}
//! ← {"id":7,"result":{…}}  |  {"id":7,"error":{"code":…,"message":…,"retry":…}}
//! ← {"event":"frame","params":{…}}   (after events.subscribe)
//! ```
//!
//! A wrong token, a first line that is not a hello, or no hello within the
//! handshake timeout closes the connection without a word; a hello in
//! another protocol is answered `version` and closed. Requests run in order,
//! one at a time; a connection holds at most [`Limits::queue`] of them, and
//! one beyond that is answered `busy` at once. A reply the owner has not sent
//! within the reply timeout answers `timeout`: with retry `soon` for a
//! read-only method, and with the `may_have_run` effect for one that acts,
//! which is still queued and runs at the owner's next drain.
//!
//! What the agent traces carries the method, window and element ids, error
//! codes and timings, never a label, a value or a request line.
//!
//! # Exit on disconnect
//!
//! A launch with [`EXIT_ON_DISCONNECT_ENV`](flui_protocol::launch::EXIT_ON_DISCONNECT_ENV)
//! makes the first connection that passed its hello the app's driver: when it
//! closes, the app quits, past every close guard, and the process exits with
//! [`EXIT_DISCONNECTED`](flui_protocol::launch::EXIT_DISCONNECTED) once the
//! teardown has written what the application persisted.

// The server is built where it has a transport: a debug build, or one with
// the `driver` feature, on a system with local sockets. The web and Android
// transports are not built yet.
#[cfg(all(
    any(debug_assertions, feature = "driver"),
    any(unix, windows),
    not(target_os = "android")
))]
mod served;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;

/// What a driver's disconnect does under `EXIT_ON_DISCONNECT`: records that
/// it happened, and asks the loop to quit with what [`Self::arm`] gave it.
/// One per loop, made before the loop starts so its end can read it.
#[derive(Default)]
pub(crate) struct Disconnect {
    happened: AtomicBool,
    quit: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl Disconnect {
    /// Ask the loop to quit with `quit` when the driver goes.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "the page's runner installs no agent yet")
    )]
    pub(crate) fn arm(&self, quit: impl Fn() + Send + Sync + 'static) {
        *self.quit.lock() = Some(Box::new(quit));
    }

    /// Whether the driver went and the loop was asked to quit for it.
    pub(crate) fn happened(&self) -> bool {
        self.happened.load(Ordering::SeqCst)
    }

    /// The driver went: ask the loop to quit, once.
    #[cfg_attr(
        not(all(
            any(debug_assertions, feature = "driver"),
            any(unix, windows),
            not(target_os = "android")
        )),
        expect(dead_code, reason = "only a served agent's driver disconnects")
    )]
    fn fire(&self) {
        if self.happened.swap(true, Ordering::SeqCst) {
            return;
        }
        tracing::info!("the driver disconnected; the app exits");
        // Taken out, so the quit runs with no lock held.
        let quit = self.quit.lock().take();
        if let Some(quit) = quit {
            quit();
        }
    }
}

impl fmt::Debug for Disconnect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Disconnect")
            .field("happened", &self.happened())
            .finish_non_exhaustive()
    }
}

/// Install the driver agent on `config` after the application's own hooks,
/// when this build carries it and the environment names an endpoint;
/// otherwise `config` is left alone and nothing is created. `disconnect` is
/// what the driver's going does under `EXIT_ON_DISCONNECT`.
#[cfg_attr(
    target_arch = "wasm32",
    expect(dead_code, reason = "the page's runner installs no agent yet")
)]
pub(crate) fn install_from_env(config: &mut crate::AppConfig, disconnect: &Arc<Disconnect>) {
    #[cfg(all(
        any(debug_assertions, feature = "driver"),
        any(unix, windows),
        not(target_os = "android")
    ))]
    if let Some(launch) = served::Launch::from_env() {
        served::install(config, launch, disconnect);
    }
    #[cfg(not(all(
        any(debug_assertions, feature = "driver"),
        any(unix, windows),
        not(target_os = "android")
    )))]
    let _ = (config, disconnect);
}
