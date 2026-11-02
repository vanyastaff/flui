//! The server, built where it has a transport: a debug build, or one with
//! the `driver` feature, on a system with local sockets.

mod dispatch;
mod endpoint;
mod hook;
mod launch;
mod registry;
mod session;

pub(crate) use self::hook::DriverAgent;
pub(crate) use self::launch::Launch;
#[cfg(test)]
pub(crate) use self::launch::Limits;

/// Emits what `event` traces, contained: a subscriber is user code, and one
/// that panics must not take down the agent thread it runs on, nor the work
/// that thread accepted. Used for every trace on the agent's own threads.
pub(super) fn trace(event: impl FnOnce()) {
    let traced = std::panic::catch_unwind(std::panic::AssertUnwindSafe(event));
    drop(traced);
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
