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
- **`flui-runtime`**: `SubmitVerdict::Queued` and `QueuedReplacing`, `FrameSettlement` and
  `UiRuntime::settle_frame`
  ([ADR-0305](/docs/adr/ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md) §4).
  A sink that settles a frame later answers `Queued`; the runtime keeps what the frame commits,
  captured at submit (its tree revision, input regions, layer reads and haptics), and the
  settlement the host hands back commits that frame by its address and epoch. A settlement no
  pending frame matches answers `Settled::Stale`. A sink that replaces a queued frame it has not
  started answers `QueuedReplacing` with that frame's stamp, and its record folds.
- **`flui-engine`**: `SendReceipt::Replaced` names the stamp of the unrendered frame a new frame
  replaced in its slot's inbox.
- **`flui-runtime`**: `SubmitVerdict::Refused`, for a window that cannot show a frame until it
  changes (a surface or size the device refuses, a renderer that panicked on the scene): nothing
  is retried, and the window's next change asks for the frame
  ([ADR-0334](/docs/adr/ADR-0334-a-failed-frame-is-painted-again-after-the-dropped-frame-backoff.md) §4).
  `SubmitVerdict` is matched without a wildcard, so code that matches it adds the arm.

### Changed

- **`flui-scheduler`**: `FrameClock::record_frame` and `record_frame_retaining_epochs` take the
  `FrameTelemetryCut` that `FrameClock::telemetry_cut` gave when the frame was submitted, so a
  frame recorded after it settles keeps its own frame id and only the inputs that came before it
  (`FrameTelemetryCut::after` leaves out the inputs an earlier pending frame covers).
  `FrameClock::frames_since` follows the order frames were recorded, so a frame recorded after a
  later one still reaches a consumer whose cursor is the later frame.
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
- **`flui-runtime`**: a frame whose sink answers `SubmitVerdict::Failed` is painted again after the
  dropped-frame backoff instead of leaving the application on its last frame with its pointer input
  held ([ADR-0334](/docs/adr/ADR-0334-a-failed-frame-is-painted-again-after-the-dropped-frame-backoff.md)).
  The backoff doubles while frames keep failing at submit, and a frame that commits the current
  tree resets it.
- **`flui-scheduler`**: a host frame whose pipeline panics after a presentation completed its
  segment, or inside it, no longer leaves that segment active: the completed one's callbacks run
  on the next frame, which the abort asks for, and a segment left running is suspended for the
  presentation's next segment to resume. Before, the
  runtime's next frame panicked on the retained segment and the application stopped drawing.
