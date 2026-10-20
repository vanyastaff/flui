### Added

- **`flui run --agent` and `flui mcp`** (`flui-cli`): an AI agent can read and drive a running
  FLUI app. `flui run --agent` starts the app (this desktop, debug builds) with its `dev-agent`
  feature, a private local endpoint and a launch token, writes `target/flui-agent.json` for the
  session and emits `run.agent.ready` on `--json`. `flui mcp` is an MCP server over stdio for the
  agent's client: `flui_windows`, `flui_read`, `flui_act` and `flui_motion` reach the app's
  semantics tree through that endpoint, connecting on the first call and again after the app
  restarts. `--release`, other targets and an app without the `dev-agent` feature are refused
  with the reason.
- **`flui create` templates declare `dev-agent`** (`flui-cli`): the basic, empty and counter
  templates carry a `dev-agent` feature (the facade's new `dev-agent` feature, which re-exports `flui-devtools` as `flui::devtools`) that installs
  `AgentServer::from_env()`. An ordinary build links neither.
- **`flui` facade `dev-agent` feature** (`flui`): re-exports `flui-devtools` as `flui::devtools`
  with its agent server enabled; install `flui::devtools::agent::AgentServer` with
  `AppConfig::with_dev_agent`.
