### Added

- `FrameRequest::presented`: a frame request carries `FramePresented { at, refresh }`, when the
  platform says the window's newest earlier frame reached the screen, on the `EventTime` timeline
  input is stamped on. `WindowCapabilities::reports_presentation` and `presentation_feedback()`
  say whether a window reports it, and why not as `Unsupported`.
- Win32 reports presentation from the DXGI frame statistics of the swap chain the renderer
  presents through; the native Wayland host from `wp_presentation_time`. The web and X11 answer
  `NotOnThisPlatform`; winit, AppKit, UIKit and Android `NotYetOnThisHost`.
- `FrameClock::record_presentation`, `last_presentation` and `next_presentation_after`
  (`flui-scheduler`, `PresentationReport`, `PRESENTATION_GRID_HORIZON`): a presentation's frame
  clock keeps the newest report, and a frame whose request names no present time bounds its idle
  work by the next refresh on the reported grid.
- `HeadlessHost::report_presented` and `MockWindow::simulate_presented` inject a presentation on
  the headless hosts; recordings carry it in their frame rows.
- The window conformance kit checks `presentation_is_reported_exactly_when_claimed`, with
  `WindowFixture::present_and_request_frame`.

### Changed

- `SurfaceTarget::pre_present_notify` takes the `SwapChainStatistics` the presenter read before
  the present, and `flui_engine::PrePresentHook` passes them (DXGI frame statistics on DX12).
- `HostInput::Frame` has a `presented` field.
