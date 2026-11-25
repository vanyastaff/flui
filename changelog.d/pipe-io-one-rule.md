### Added

- `flui_protocol::endpoint::PolledStream`: a local endpoint's pipe or socket, polled, read and
  written alike on every host. The app's agent, the driver and the CLI's own endpoints all use
  it in place of three copies of the Windows named-pipe rule.

### Fixed

- On Windows, `flui run --agent`'s run endpoint and the page relay send a reply longer than the
  pipe's 512-byte buffer to a client that polls (the driver) instead of offering it whole forever.
- On Windows, a line a client wrote just before it closed its end of a local endpoint is read
  before the end, by the app's agent, the driver and the CLI's endpoints alike.
