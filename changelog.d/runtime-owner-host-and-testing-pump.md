### Added

- **`flui_testing::HeadlessRealm`**: a headless host for a `UiRealm` — a
  `HeadlessWindow`, a `HeadlessSink` that keeps the last scene, and one
  `ManualClock` the realm reads as its clock source — that drives every frame
  through `UiRealm::pump` and raises a frame failure the realm contained after
  the pump, the first failure of a pump authoritative over a later unwind.

### Changed

- **The widget harness is `flui_testing::widgets`** (`lay_out`, `LaidOut`,
  `harness::mount`, `SignalProbe`); `flui::testing::widgets` names it as
  before. Its frames are the realm's own `UiRealm::pump`, the mount included:
  the tree sits under the realm's root `MediaQuery`, `VsyncScope`,
  `FocusRoot` and `GestureArenaScope`, post-frame callbacks registered in
  `init_state` run at the end of the mount frame, `mount_with_ime` frames are
  text-store transactions with the realm's commit gate, and a contained frame
  failure is raised after the pump.
- **`LaidOut::build_owner_mut` is `LaidOut::with_build_owner_mut`**, a closure
  accessor: the realm keeps the owner behind its widgets binding.
- **`UiRealm::new` takes a `flui_scheduler::ClockSource`**: the realm's frame
  time, gesture deadlines and produce gate read it. Hosts pass
  `ClockSource::Platform`.
- **`flui-testing` sits above `flui-runtime` and `flui-widgets`** (tier K,
  order 6); `flui-runtime` admits it as a normal dependent beside `flui-app`.

### Removed

- **`flui-widgets`' `testing` feature and `flui_widgets::testing`**: the
  harness moved to `flui_testing::widgets`.
- **`lay_out_with_pipeline_owner`, `Harness::set_transaction_open`,
  `mount_with_capabilities`, `PostFrameCapability` and `TextInputCapability`**:
  a realm owns its pipeline and its text-store transaction and always installs
  its post-frame lanes; a test of their absence mounts on `HeadlessBinding`.
