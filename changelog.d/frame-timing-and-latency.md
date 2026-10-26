### Added
- `FrameLatency` (`Balanced`, the default, or `Low`) and `AppConfig::with_frame_latency` choose how many frames each window's swapchain holds ahead of the display; `Renderer::set_frame_latency` changes it for one window.
- `FrameRequest` carries a frame's vsync and, where the platform names it, its target present time.

### Changed
- `PlatformWindow::on_request_frame` callbacks receive a `FrameRequest`. The frame pump runs a frame at its delivered vsync, held between the previous frame and the wake, and idle-priority work stops at the request's target present time.
- `HostInput::Frame` gains `target_present`, and `UiRuntime::note_frame_vsync` becomes `note_frame_request`. A recording's `frame` row carries an optional `target_present`.
- The Web backend stamps each frame with the browser's animation-frame time, and the Win32 backend with the compositor's last refresh and its next one as the target present time, both rebased onto the event clock; other backends keep the reading at admission.
