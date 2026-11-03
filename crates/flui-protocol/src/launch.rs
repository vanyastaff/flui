//! The launch contract between a driver and the app it starts or attaches to (ADR-0239 §3,
//! §5): where the app's agent listens, the token a client presents, and how a launched app
//! ends when its driver goes.
//!
//! Desktop and iOS Simulator apps read the environment variables; the iOS Simulator passes
//! them through `simctl launch` as `SIMCTL_CHILD_` variables. An Android app reads the intent
//! extras `am start` carried. A web app has neither: the driver's preload script installs the
//! token before the module starts, and no URL carries it.
//!
//! With no endpoint, the app's agent creates nothing: no thread, no socket, no tap.

/// The environment variable that names the agent's endpoint: a pipe name on Windows
/// (`flui-agent-1234` or `\\.\pipe\flui-agent-1234`), a socket path elsewhere.
pub const ENDPOINT_ENV: &str = "FLUI_AGENT_ENDPOINT";

/// The environment variable that carries the token a client presents in its hello.
pub const TOKEN_ENV: &str = "FLUI_AGENT_TOKEN";

/// The environment variable that, set to `1`, makes a launched app exit with
/// [`EXIT_DISCONNECTED`] when its driver's connection closes. A driver that attaches to a
/// running app never sets it.
pub const EXIT_ON_DISCONNECT_ENV: &str = "FLUI_AGENT_EXIT_ON_DISCONNECT";

/// The Android intent extra that carries [`ENDPOINT_ENV`]'s value: the name of an abstract
/// Unix socket.
pub const ENDPOINT_EXTRA: &str = "flui.agent.endpoint";

/// The Android intent extra that carries [`TOKEN_ENV`]'s value.
pub const TOKEN_EXTRA: &str = "flui.agent.token";

/// The Android intent extra that carries [`EXIT_ON_DISCONNECT_ENV`]'s value.
pub const EXIT_ON_DISCONNECT_EXTRA: &str = "flui.agent.exit_on_disconnect";

/// The shortest token an app accepts, in bytes. A shorter one leaves the agent inert.
pub const MIN_TOKEN_LEN: usize = 32;

/// The exit code of an app that ended because its driver's connection closed under
/// [`EXIT_ON_DISCONNECT_ENV`]: apart from a normal exit (`0`) and a panic (`101`), so a
/// driver and CI tell the three apart.
pub const EXIT_DISCONNECTED: i32 = 75;

/// Whether `value`, read from [`EXIT_ON_DISCONNECT_ENV`] or its extra, turns the exit on:
/// exactly `1`.
#[must_use]
pub fn exit_on_disconnect(value: &str) -> bool {
    value == "1"
}
