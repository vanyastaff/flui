### Fixed

- **Win32 wake deadlines** (`flui-platform`): a wake-deadline hook that answers the same instant
  for several obligations now gets one frame per obligation; the third and later were stranded
  until unrelated input. An answer no frame could service still never spins the message loop
  ([ADR-0044](/docs/adr/ADR-0044-driver-loop-hybrid.md)).
- **Win32 system keys** (`flui-platform`): a consumed Alt+Space no longer opens the system menu
  when the input handler pumps messages (for example through a modal API) before it returns.
