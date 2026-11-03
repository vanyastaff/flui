//! The server's transport, built where it has one: a debug build, or one with the `driver`
//! feature, on a system with local sockets, or in a page. The conversation itself is
//! [`flui_runtime::agent`]'s.

#[cfg(any(unix, windows))]
mod connection;
#[cfg(any(unix, windows))]
mod endpoint;
mod hook;
mod launch;
// The page's transport; its conversation is built on every host's tests.
#[cfg(any(target_arch = "wasm32", test))]
mod page;

/// The transport this target serves on: a local socket, or a page export.
#[cfg(any(unix, windows))]
use self::endpoint as transport;
#[cfg(target_arch = "wasm32")]
use self::page::web as transport;

pub(crate) use self::hook::DriverAgent;
pub(crate) use self::launch::Launch;
#[cfg(all(test, any(unix, windows)))]
pub(crate) use flui_runtime::agent::Limits;

/// Emits what `event` traces, contained: a subscriber is user code, and one
/// that panics must not take down the agent thread it runs on, nor the work
/// that thread accepted. Used for every trace on the agent's own threads.
#[cfg(any(unix, windows))]
pub(super) fn trace(event: impl FnOnce()) {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a tracing subscriber's panic must not take down the agent thread"
    )]
    let traced = std::panic::catch_unwind(std::panic::AssertUnwindSafe(event));
    if let Err(payload) = traced {
        flui_foundation::panic::discard_payload(payload);
    }
}

/// Install an agent for `launch` on `config`, after the application's own
/// hooks. The hello names the app by `config`'s title, and the system it runs on.
pub(crate) fn install(
    config: &mut crate::AppConfig,
    launch: Launch,
    disconnect: &std::sync::Arc<super::Disconnect>,
) {
    let agent = DriverAgent::new(
        launch,
        app_info(&config.title),
        std::sync::Arc::clone(disconnect),
    );
    config.dev_agent = Some(crate::app::dev_agent::DevAgent::with(
        config.dev_agent.take(),
        agent,
    ));
}

/// What the hello says of the app titled `title`: its system, and its process where it has one.
fn app_info(title: &str) -> flui_protocol::AppInfo {
    #[cfg(target_arch = "wasm32")]
    {
        // A page has no process ID to give.
        flui_protocol::AppInfo::new(title, "web")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        flui_protocol::AppInfo::new(title, std::env::consts::OS).with_pid(std::process::id())
    }
}

#[cfg(all(test, any(unix, windows)))]
mod tests;
