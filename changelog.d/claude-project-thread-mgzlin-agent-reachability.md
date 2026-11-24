### Changed

- **A release application that installs no development-agent hook links none of the agent**
  (ADR-0402): the UI runtime reaches the agent only through the side a vended window installs,
  and only `AppConfig::dev_agent` (and the driver agent's own installation) names the vending.
  About 0.58 MB of a generated counter's Linux x86_64 release binary. An application that installs
  its own hook keeps a working agent in every build.
- **`flui_runtime::dev_agent::DevAgentHost::attach` takes a `WindowSource`** beside the hooks:
  `WindowSource::dev_agent()` where a host installs a hook.
