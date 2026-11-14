### Removed

- **`flui-engine`**: `RasterOwner`, `RasterHandle` and their mailbox types (`RasterAck`,
  `RasterCompletion`, `RasterSubmitError`, `PumpOutcome`, `FrameDropReason`, `SurfaceState`), which
  no window runner drives any more; `RasterExecutor` and its slots replace them.
