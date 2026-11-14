### Added

- **`flui-engine`**: `RasterExecutor`, the one mutator that renders every presentation's slot
  ([ADR-0305](/docs/adr/ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)).
  The owner mints a `SlotId`, attaches the slot and sends closed `RasterCommand`s through a
  `RasterSender`; each frame settles by its stamp into the slot's record, which the owner drains.
  A full lifecycle queue, a stale slot and a stopped executor hand the command back
  (`RasterSendError`). `GpuContext` is the executor's backend for offscreen slots, and the
  `testing` feature adds `flui_engine::testing::ScriptedSlots`, a backend without a GPU.
- **`flui-platform`**, **`flui-native`**: `DisplayConnection` and `OwnerPlatform::display_connection`,
  the host's connection to its window system for its whole life (the X11 or Wayland connection, a
  pointerless handle on Win32, AppKit, UIKit, Android and the web, none on headless), which the
  application's one GPU instance is made on
  ([ADR-0305](/docs/adr/ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md) §2).
- **`flui-runtime`**: `SubmitVerdict::Queued`, `FrameSettlement` and `UiRuntime::settle_frame`
  ([ADR-0305](/docs/adr/ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md) §4).
  A sink that settles a frame later answers `Queued`; the runtime keeps what the frame commits,
  captured at submit (its tree revision, input regions, layer reads and haptics), and the
  settlement the host hands back commits that frame by its address and epoch. A settlement no
  pending frame matches answers `Settled::Stale`.

### Changed

- **`flui-interaction`**: `HapticsHost::frame_presented` takes the `HapticCut` that
  `HapticsHost::frame_cut` gave when the frame was submitted, and plays only the requests queued
  before it.
- **`flui-runtime`**: `FrameSink::submit` takes the frame's `FrameEpoch` beside its publication. The
  UI runtime numbers the frames its presentations submit, and the host stamps each frame with
  that number instead of keeping a count of its own
  ([ADR-0305](/docs/adr/ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md) §4).
- **`flui-engine`**: `HeadlessRenderer` is a GPU context with one offscreen slot
  ([ADR-0305](/docs/adr/ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)):
  captures reuse one device, one pipeline set and one glyph atlas, and a capture of a new size
  resizes the slot instead of building a target per call. The renderer is `Send` and no longer
  `Sync`; a capture started on it while another is running answers
  `EngineError::FrameAlreadyActive` instead of waiting on a lock.

### Deprecated

- **`flui-engine`**: the per-window `RasterBackend`, replaced by `RasterExecutor` and its slots;
  it goes once the web draws through slots.

### Fixed

- **`flui-engine`**: an antialiased rounded-rectangle or superellipse clip samples only its
  corners: the clip mask skips the cross of both arms, not one arm, so a window-sized clip at
  sixteen leaves samples 8,188 mask pixels instead of 69,500 and fits a software adapter's
  readback deadline.
