//! The server's transport, built where it has one: a debug build, or one with the `driver`
//! feature, on a system with local sockets. The conversation itself is
//! [`flui_runtime::agent`]'s.

mod connection;
mod endpoint;
mod hook;
mod launch;

pub(crate) use self::hook::DriverAgent;
pub(crate) use self::launch::Launch;
#[cfg(test)]
pub(crate) use flui_runtime::agent::Limits;

/// Emits what `event` traces, contained: a subscriber is user code, and one
/// that panics must not take down the agent thread it runs on, nor the work
/// that thread accepted. Used for every trace on the agent's own threads.
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
/// hooks. The hello names the app by `config`'s title.
pub(crate) fn install(
    config: &mut crate::AppConfig,
    launch: Launch,
    disconnect: &std::sync::Arc<super::Disconnect>,
) {
    let app = flui_protocol::AppInfo::new(config.title.clone(), std::env::consts::OS)
        .with_pid(std::process::id());
    let agent = DriverAgent::new(launch, app, std::sync::Arc::clone(disconnect));
    config.dev_agent = Some(crate::app::dev_agent::DevAgent::with(
        config.dev_agent.take(),
        agent,
    ));
}

#[cfg(test)]
mod tests;
