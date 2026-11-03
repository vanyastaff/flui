### Changed

- **A frame that keeps failing backs off.** After a dropped frame (a layout or paint
  panic, a pipeline error) a presentation waits its display's refresh period, doubled for
  each further consecutive drop, up to `flui_runtime::frame_failure::FAILURE_RETRY_CAP`
  (1 s), instead of retrying on every wake. A key press or pointer button press retries at
  once, once per wait.
- **Every debug FLUI app hosts the driver agent** (`flui-app`, ADR-0239): launched with
  `FLUI_AGENT_ENDPOINT` and `FLUI_AGENT_TOKEN`, a debug build (or one with the new `driver`
  feature) serves its windows in protocol 0.9 over a current-user named pipe or a `0700` Unix
  socket, with no application code. `FLUI_AGENT_EXIT_ON_DISCONNECT=1` makes the app exit with
  `flui_protocol::launch::EXIT_DISCONNECTED` when its driver's connection closes.
- **`AppConfig::with_dev_agent` appends** a hook instead of replacing the installed one; each
  hook is contained on its own.
- **`flui run --agent`** needs no feature of the application in a debug build, accepts
  `--release` (building with the `driver` feature of `flui` or `flui-app`, under the key the
  application names it by), and `flui mcp` speaks protocol 0.9.
- **An agent's `wait.until` without `timeout_ms`** waits `WaitParams::DEFAULT_TIMEOUT_MS`
  (5 s) and none waits past `WaitParams::MAX_TIMEOUT_MS` (24 h); the window answers it
  unsatisfied rather than the connection timing out, and a wait whose asker has gone gives
  up its place under the window's cap.
- **`flui_protocol::capture::BlobRef::id`** is a `BlobId`.
- **The driver agent serves every older client** its protocol version does not break
  (`ProtocolVersion::serves`): it greets one with its own version and capabilities, and only
  a newer client, or one across a listed break, is refused `version`. An error that answers no
  request carries no `id` instead of `"id": null`.
- **`flui mcp` reads past events and late answers** on its connection, and treats an error
  that answers no request as its current request's.

### Added

- **`HeadlessHost::time_to_next_frame`, `pump_next_frame` and `set_window_display`**: wait
  as a host's loop does before its next frame, and deliver the display a window is on.
- **`LaidOut::dispatch_ime`** delivers an input-method event to the focused field, and the
  testing kit's window now offers a push-model input method, as winit does.
- **`flui_protocol::launch` and `flui_protocol::framing`**: the driver agent's launch contract
  (variable names, Android extras, the shortest token, the exit code) and its framing limits
  and binary-payload header. `Hello::new`, `HelloReply::new`, `AppInfo::new` build the
  handshake.
- **`flui_protocol::envelope`**: the lines of a conversation as types, `Greeting`,
  `Request<P>`, `RequestId` and `ServerLine<R>`, the last read by its keys (`hello`, `event`,
  `error`, `result`) and published in the protocol 0.9 schema. `framing::LineReader` splits a
  byte stream into lines of at most a given length, refusing a longer one with
  `FramingError::LineTooLong`.
- **The facade's `driver` and `devtools` features**: `driver` keeps the agent in a release
  build; `devtools` re-exports `flui-devtools` as `flui::devtools`.

### Removed

- **`flui-devtools`' `agent` feature and `AgentServer`**, and the facade's `dev-agent` feature:
  the driver agent is `flui-app`'s. `flui create` templates no longer install a hook.
