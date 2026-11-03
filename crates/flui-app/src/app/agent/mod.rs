//! The driver agent (ADR-0239): every debug FLUI app, and every build with
//! the `driver` feature, serves its windows to a driver over a local
//! endpoint, with no code in the application.
//!
//! The runner installs it beside the application's own development hooks
//! when the launch names an endpoint (`served::Launch::from_env`); with none,
//! nothing is created: no thread, no socket. The agent is a
//! [`DevAgentHook`](flui_view::dev_agent::DevAgentHook): the host attaches it
//! when the loop starts and hands it each window with content. The
//! conversation is [`flui_runtime::agent`]'s (ADR-0288): this module keeps the
//! launch and the desktop transport, which hands each connection's bytes to a
//! [`Session`](flui_runtime::agent::Session) and writes what it answers. Every
//! operation goes through the window's owner inbox, so nothing here touches a
//! tree, and no thread waits on an answer: the owner's answer wakes the
//! connection's thread.
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
    // The served agent's tests arm it on iOS too.
    #[cfg_attr(
        any(
            target_arch = "wasm32",
            target_os = "android",
            all(target_os = "ios", not(test))
        ),
        expect(dead_code, reason = "only the desktop loop exits when its driver goes")
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
        // Taken out, so the quit runs with no lock held.
        let quit = self.quit.lock().take();
        if let Some(quit) = quit {
            quit();
        }
        // Traced once the quit is asked: a subscriber is user code, and one
        // that panics must not keep the app running past its driver.
        let traced = std::panic::catch_unwind(|| {
            tracing::info!("the driver disconnected; the app exits");
        });
        if let Err(payload) = traced {
            flui_foundation::panic::discard_payload(payload);
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
    any(target_arch = "wasm32", target_os = "android"),
    expect(
        dead_code,
        reason = "the page's and Android's runners install no agent yet"
    )
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

#[cfg(all(
    test,
    any(debug_assertions, feature = "driver"),
    any(unix, windows),
    not(target_os = "android")
))]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    use super::Disconnect;

    /// A payload whose destructor panics too.
    struct Bomb;

    impl Drop for Bomb {
        fn drop(&mut self) {
            panic!("the payload panics as it is dropped");
        }
    }

    /// A subscriber that panics at every event, with a [`Bomb`].
    struct Panicking;

    impl Subscriber for Panicking {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, _: &Event<'_>) {
            std::panic::panic_any(Bomb);
        }
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }

    #[test]
    fn a_subscriber_that_panics_does_not_keep_the_app_running_past_its_driver() {
        let disconnect = Disconnect::default();
        let quits = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&quits);
        disconnect.arm(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        tracing::subscriber::with_default(Panicking, || disconnect.fire());
        assert_eq!(quits.load(Ordering::SeqCst), 1, "the quit was asked");
        assert!(disconnect.happened());
    }

    #[test]
    fn a_trace_on_an_agent_thread_survives_a_subscriber_that_panics() {
        let mut after = false;
        tracing::subscriber::with_default(Panicking, || {
            super::served::trace(|| tracing::info!("an agent event"));
            after = true;
        });
        assert!(after, "the thread went on past the trace");
    }
}
