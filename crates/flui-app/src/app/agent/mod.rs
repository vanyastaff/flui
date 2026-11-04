//! The driver agent (ADR-0239): every debug FLUI app, and every build with
//! the `driver` feature, serves its windows to a driver over a local
//! endpoint, with no code in the application.
//!
//! The runner installs it beside the application's own development hooks
//! when the launch names an endpoint, read through the platform's launch
//! values (ADR-0288 §7): the environment on the desktop and the iOS
//! Simulator, the launching intent's extras on Android, the page's launch
//! object on the web. With none, nothing is
//! created: no thread, no socket, no global. The agent is a
//! [`DevAgentHook`](flui_view::dev_agent::DevAgentHook): the host attaches it
//! when the loop starts and hands it each window with content. The
//! conversation is [`flui_runtime::agent`]'s (ADR-0288): this module keeps the
//! launch and the local-socket transport, which hands each connection's bytes
//! to a [`Session`](flui_runtime::agent::Session) and writes what it answers.
//! Every operation goes through the window's owner inbox, so nothing here
//! touches a tree, and no thread waits on an answer: the owner's answer wakes
//! the connection's thread.
//!
//! # The endpoint
//!
//! A named pipe on Windows (`\\.\pipe\<name>`, the current user's only,
//! remote clients refused) and a Unix domain socket elsewhere (a path whose
//! parent directory the current user owns with mode `0700`; the socket file
//! is removed when the agent stops). On Linux and Android an address `@name`
//! is the abstract socket `name`, the only kind on Android, where a driver
//! reaches it through `adb forward`: any process of the system reaches an
//! abstract socket, so a connection whose peer is not the app's user, root,
//! or on Android the shell is closed unread. No TCP port. A bind failure is
//! logged, and the application keeps running without an agent.
//!
//! In a page the endpoint is a global the launch names (`__flui_agent`, say):
//! `connect(onLine)` opens a connection whose lines reach `onLine`, and
//! returns `send(line)` and `close()` (flui-native's
//! `page_export`, built for wasm32 alone). A driver calls it through the
//! browser's remote protocol: `flui run --agent` passes a WebDriver BiDi
//! channel as `onLine` in Firefox and a DevTools binding in the Chromium
//! family. Each connection's conversation runs as a task of the page's
//! executor.
//!
//! The token keeps out other users and remote callers. It does not keep out
//! other processes of the same user, which can read the application's
//! environment just as they can reach the endpoint. In a page it keeps out
//! other origins and other tabs; a script of the page's own origin that runs
//! before the application's module starts can read the launch object, as a
//! process of the same user can read the environment.
//!
//! # Exit on disconnect
//!
//! A launch with [`EXIT_ON_DISCONNECT_ENV`](flui_protocol::launch::EXIT_ON_DISCONNECT_ENV)
//! makes the connection admitted as the app's driver (ADR-0288 §8) the one it exits with: when it
//! closes, the app quits, past every close guard, and the process exits with
//! [`EXIT_DISCONNECTED`](flui_protocol::launch::EXIT_DISCONNECTED) once the
//! teardown has written what the application persisted. A page cannot close
//! its tab: there the loop quits and the agent stops, and the driver closes
//! the tab.

// The server is built where it has a transport (`cfg(driver_agent)`, set by
// `build.rs`): a debug build, or one with the `driver` feature, on a system
// with local sockets, or in a page.
#[cfg(driver_agent)]
mod served;

// `build.rs` reads the profile's debug assertions from Cargo; a build it
// misread fails here rather than shipping, or leaving out, the agent.
#[cfg(any(
    all(driver_agent, not(any(debug_assertions, feature = "driver"))),
    all(
        not(driver_agent),
        any(debug_assertions, feature = "driver"),
        any(unix, windows, target_arch = "wasm32")
    )
))]
compile_error!("BUG: build.rs set `driver_agent` against this build's profile and features");

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
    /// Ask the loop to quit with `quit` when the driver goes; at once when it
    /// went before the loop could arm this.
    // The served agent's tests arm it on iOS too.
    #[cfg_attr(
        all(target_os = "ios", not(test)),
        expect(dead_code, reason = "an iOS loop does not exit when its driver goes")
    )]
    pub(crate) fn arm(&self, quit: impl Fn() + Send + Sync + 'static) {
        *self.quit.lock() = Some(Box::new(quit));
        // Read after the quit is stored: `fire` sets the flag before it takes
        // the quit, so one of the two takes it, and only one.
        if self.happened() {
            let quit = self.quit.lock().take();
            if let Some(quit) = quit {
                quit();
            }
        }
    }

    /// Whether the driver went and the loop was asked to quit for it.
    pub(crate) fn happened(&self) -> bool {
        self.happened.load(Ordering::SeqCst)
    }

    /// The driver went: ask the loop to quit, once.
    #[cfg_attr(
        not(driver_agent),
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
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a subscriber's panic must not keep the app past its driver"
        )]
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
/// when this build carries it and the launch values `value` reads name an endpoint
/// (ADR-0288 §7); otherwise `config` is left alone and nothing is created.
/// `disconnect` is what the driver's going does under `EXIT_ON_DISCONNECT`, and
/// `records_native` whether the loop journals its backend, so a recording can hold the
/// `native` layer (§11). Whether the agent was installed.
pub(crate) fn install_from_launch(
    config: &mut crate::AppConfig,
    value: impl Fn(flui_platform::LaunchKey) -> Option<String>,
    disconnect: &Arc<Disconnect>,
    records_native: bool,
) -> bool {
    #[cfg(driver_agent)]
    if let Some(launch) = served::Launch::read(value) {
        served::install(config, launch, disconnect, records_native);
        return true;
    }
    #[cfg(not(driver_agent))]
    let _ = (config, value, disconnect, records_native);
    false
}

#[cfg(all(test, driver_agent))]
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
    fn a_driver_that_went_before_the_loop_armed_still_ends_it_once() {
        let disconnect = Disconnect::default();
        disconnect.fire();
        let quits = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&quits);
        disconnect.arm(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(quits.load(Ordering::SeqCst), 1, "arming asks the quit");
        disconnect.fire();
        assert_eq!(quits.load(Ordering::SeqCst), 1, "and only once");
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

    // A page has no agent thread to trace on.
    #[cfg(any(unix, windows))]
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
