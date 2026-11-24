### Added

- **`flui_runtime::dev_agent::AgentWindowSource`**: what a `DevAgentHost` vends its windows
  through; `AgentWindowSource::dev_agent()` where a host installs a hook (ADR-0402).

### Changed

- **A release application that installs no development-agent hook links none of the agent**
  (ADR-0402): the UI runtime reaches the agent only through the side a vended window installs,
  and only `AppConfig::dev_agent` (and the driver agent's own installation) names the vending.
  About 0.6 MB of a generated counter's release binary, on Linux x86_64 and on wasm32 alike.
  An application that installs its own hook keeps a working agent in every build.
- **`flui_runtime::dev_agent::DevAgentHost::attach` takes an `AgentWindowSource`** before the
  hooks.

### Fixed

- **A presentation's close holds the panic of an agent reply it delivers** as it holds any other
  step's: the close goes on, preserving, and an earlier failure stays the one resumed.
