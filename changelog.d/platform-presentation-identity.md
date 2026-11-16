### Changed

- A presentation report names the frame it describes: `FramePresented` carries the `FrameStamp`
  its present carried (`frame`), and its `refresh` is a checked `RefreshPeriod`; `new` takes the
  frame, and `refresh` replaces `with_refresh`. Windows, Wayland, macOS and headless report a shown
  present as its own frame, not the newest one presented, and report nothing for a present they
  cannot tell apart.
- `SurfaceTarget::pre_present_notify` takes the stamp of the frame the present shows (`None` for a
  present that shows no frame), and `RasterBackend::render` takes the frame's stamp.
- `SwapChainStatistics::Dxgi` holds `DxgiStatistics` (the swap chain's `GetLastPresentCount` and
  what its frame statistics name, `DxgiShown`); `SwapChainStatistics::DxgiDisjoint` became
  `DxgiShown::Disjoint`.
- `HostInput::Frame` carries a `ShownFrame`, the report named within its presentation. A frame
  request whose report names another presentation's frame has it dropped.
- `HeadlessHostWindow::simulate_presented(at, refresh)` shows the oldest present the simulated
  window system holds and returns its frame; `simulate_discarded` drops one unreported.
  `HeadlessHost::report_presented(frame, at, refresh)` reports a frame its sink presented.
- Agent protocol 0.16: a recorded presentation report carries the frame it names (`frame`, an
  optional field); a 0.15 client ignores it.

### Added

- `FrameEpoch::from_raw`, `SurfaceGeneration::from_raw` and `ResourceGeneration::from_raw`, to read
  a recorded counter back.
- `flui_testing::window_conformance::fixture_frame`, the frame a window fixture's present carries.
