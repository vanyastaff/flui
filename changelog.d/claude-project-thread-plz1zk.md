### Changed

- **`flui-engine`**: `HeadlessRenderer` is a GPU context with one offscreen slot
  ([ADR-0305](/docs/adr/ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)):
  captures reuse one device, one pipeline set and one glyph atlas, and a capture of a new size
  resizes the slot instead of building a target per call. The renderer is `Send` and no longer
  `Sync`; a capture started on it while another is running answers
  `EngineError::FrameAlreadyActive` instead of waiting on a lock.
