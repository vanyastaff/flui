### Added

- **`flui-app`**: `AppConfig::with_dev_agent` installs a development agent hook; the desktop
  and iOS runners attach it once per event loop and hand it each window that mounts a root
  view, so a tool can read the window's semantics tree and act on its elements
  ([ADR-0095](/docs/adr/ADR-0095-agent-protocol-schema-crate.md) §3).
- **`flui-view`**: `dev_agent::{DevAgentHook, AgentWindow, AgentAnswer, AgentFault,
  HandleKind}`, the seam between a host and such a tool, also reached as
  `flui_sdk::view::dev_agent`. An `AgentWindow` holds its window weakly: once the window
  closes, every call answers `gone`. A window collects semantics only while its hook keeps an
  `AgentWindow`, and a hook whose `attach` answers `false` is handed none.
- **`flui-devtools`**: the `agent` feature (off by default) adds `agent::AgentServer`, which
  serves the application's windows over a named pipe or Unix socket with a launch token, as
  newline-delimited JSON, in debug builds only; `cargo run -p flui-devtools --example
  agent_counter --features agent` runs the counter behind it.
- **`flui-testing`**: `HeadlessDevAgent` attaches a development agent hook the way a runner's
  loop does, and `HeadlessRealm::with_dev_agent` hands it the realm's window.
