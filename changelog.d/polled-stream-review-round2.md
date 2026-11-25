### Added

- `flui_protocol::testing::ScriptedPipe::written`: the bytes the pipe's writes took, in order.

### Fixed

- `flui run`'s page relay closes the page connection of a client that falls more than its
  bound behind, instead of leaving it open in the app's agent, a driver's seat and held input
  included, until the page reloads. Every page connection the relay lets go of, one the page
  ended included, is released, so the page may drop it.
- `flui run --agent`'s run endpoint writes nothing more to a client it gave up on for falling
  behind, so the answer to its reload no longer follows a gap in what it read.
