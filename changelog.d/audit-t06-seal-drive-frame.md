### Removed

- **Breaking:** `flui_scheduler::OwnerFrame::drive_frame` is no longer a public method. The frame
  drive moves to the sealed `#[doc(hidden)]` trait `flui_scheduler::__runtime::DriveFrame`, which
  the crates that own a scheduler's owner frame (`flui-runtime`, `flui-testing`) import. Drive a
  frame through `UiRuntime` or the headless binding's `pump_frame` instead. `__runtime` carries no
  semver promise.
