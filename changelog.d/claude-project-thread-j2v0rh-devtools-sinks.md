### Changed

- `flui_devtools::Capture::on_frame` returns a `CaptureSubscription`. Keep it for as long as the sink should run: the sink is removed when the handle drops, so `let _ = capture.on_frame(...)` removes it at once. The handle does not keep the capture alive.
- A frame sink that panics is reported as one `ERROR` event on target `flui.devtools` per frame, naming the first panic, and is not raised; the sinks after it still see the frame. The docs used to say the panic was resumed, which it never was.
- `TimelineEvent::runtime` is an `Option<UiRuntimeId>` and `TimelineEvent::presentation` an `Option<PresentationId>` instead of their packed `u64`s; compare them with the ids you hold, or call `as_u64()` for the number. `Timeline::export_json` still writes the packed numbers, and reading back a number that is no valid id is an error.

### Removed

- `Capture::on_frame_open` and `FrameOpening`: nothing called them. Read a frame when it closes with `Capture::on_frame`.

### Fixed

- A dropped `Profiler` no longer leaves its sink on the capture, where it ran on every frame; making and dropping profilers over one capture no longer grows its sink list.
- Frame-snapshot events from `Timeline::record_frame_snapshots` carry their runtime and presentation, so the Chrome trace shows each in its runtime's process instead of the process for events that name no runtime. Their `args.presentation` in the Chrome trace is now the packed number, as on every other event of the presentation; `args.ui_runtime` and the event name keep the ids as text.
- A `Capture` created after another was dropped no longer reads the state the dropped capture left in a span that was still open, such as a frame it opened, which it could take for a frame of its own.
