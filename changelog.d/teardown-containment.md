### Changed

- **A failed owner operation no longer stops the owner's turn.** A panic out of one frame,
  input batch, metrics change or close is reported once and contained to that operation;
  the operations queued behind it still run (ADR-0258 §1). Only a failure that is terminal
  (the close of the last presentation of the last running runtime, or one raised after the
  owner closed) still ends the application with the first panic.
- **No application panic leaves a platform handler method** (ADR-0258 §4). Every method of
  the runner's application and window handlers runs under one catch: a panic is held as the
  loop's terminal failure, the loop quits, and the process exits 101 after the flush,
  instead of unwinding into the platform, which on Win32, AppKit and UIKit is an abort. A
  window's frame is the exception: its failure is answered `FrameOutcome::Failed` and the
  application keeps running.

### Added

- **`FrameOutcome::Failed`**: a window handler's frame ran and failed, and the handler
  contained the failure. The platform counts it as a frame that ran. `FrameOutcome` is now
  `#[non_exhaustive]`.
