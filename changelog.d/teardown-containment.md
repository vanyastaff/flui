### Changed

- **A failed owner operation no longer stops the owner's turn.** A panic out of one frame,
  input batch, metrics change or close is reported once and contained to that operation;
  the operations queued behind it still run (ADR-0258 §1). Only a failure that is terminal
  (the close of the last presentation of the last running runtime, or one raised after the
  owner closed) still ends the application with the first panic.
