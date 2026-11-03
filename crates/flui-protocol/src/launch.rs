//! The launch contract between a driver and the app it starts or attaches to (ADR-0239 §3,
//! §5): where the app's agent listens, the token a client presents, and how a launched app
//! ends when its driver goes.
//!
//! Desktop and iOS Simulator apps read the environment variables; the iOS Simulator passes
//! them through `simctl launch` as `SIMCTL_CHILD_` variables. An Android app reads the intent
//! extras `am start` carried. A web app has neither: the driver's preload script installs an
//! object of the extras' names and values as [`PAGE_LAUNCH_GLOBAL`] before the module starts
//! (its endpoint is the name of the global the agent defines its export as, `__flui_agent`, in
//! the shape [`PAGE_EXPORT_VERSION`] versions), and no URL carries the token.
//!
//! Each variable is its extra's name in the environment's spelling, uppercase with `_` for `.`,
//! so an app reads both under one key (ADR-0288 §7).
//!
//! With no endpoint, the app's agent creates nothing: no thread, no socket, no tap.

/// The environment variable that names the agent's endpoint: a pipe name on Windows
/// (`flui-agent-1234` or `\\.\pipe\flui-agent-1234`), a socket path elsewhere, and on Linux
/// `@name` for the abstract socket `name`.
pub const ENDPOINT_ENV: &str = "FLUI_AGENT_ENDPOINT";

/// The environment variable that carries the token a client presents in its hello.
pub const TOKEN_ENV: &str = "FLUI_AGENT_TOKEN";

/// The environment variable that, set to `1`, makes a launched app exit with
/// [`EXIT_DISCONNECTED`] when its driver's connection closes. A driver that attaches to a
/// running app never sets it.
pub const EXIT_ON_DISCONNECT_ENV: &str = "FLUI_AGENT_EXIT_ON_DISCONNECT";

/// The Android intent extra that carries [`ENDPOINT_ENV`]'s value: `@name`, the abstract
/// socket `name` (`@flui-agent-1234`), which a driver reaches through
/// `adb forward tcp:0 localabstract:name`.
pub const ENDPOINT_EXTRA: &str = "flui.agent.endpoint";

/// The Android intent extra that carries [`TOKEN_ENV`]'s value.
pub const TOKEN_EXTRA: &str = "flui.agent.token";

/// The Android intent extra that carries [`EXIT_ON_DISCONNECT_ENV`]'s value.
pub const EXIT_ON_DISCONNECT_EXTRA: &str = "flui.agent.exit_on_disconnect";

/// The global a web app's launch object is installed as: an object whose properties are the
/// extras' names ([`ENDPOINT_EXTRA`], …) with their string values, installed before the app's
/// module runs. The app removes it as it starts.
pub const PAGE_LAUNCH_GLOBAL: &str = "__FLUI_LAUNCH__";

/// The version of the export a web app's agent defines as the global its endpoint names:
/// `{ version, connect(onLine) }`. `connect` returns `{ version, send(line), close() }` or
/// throws when the agent refuses the connection; each line the app writes reaches `onLine`
/// without its newline, and `onLine(null)` says the app ended the connection.
pub const PAGE_EXPORT_VERSION: u32 = 1;

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
