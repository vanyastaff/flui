### Added

- **Android reports the Activity's visibility, execution state and memory pressure**
  (`flui-platform`, `flui-app`): `onStart`/`onStop` reach `on_visibility_status_change`,
  `onResume`/`onPause` reach `on_execution_state_change`, and window focus reaches only
  `on_active_status_change`. `onLowMemory` delivers `MemoryPressureLevel::Critical`; the
  process's `ActivityManager` trim level is read through JNI on `LowMemory`, `Pause`, `Stop` and
  `Resume` and delivers `Moderate` or `Critical`, once per change. `onSaveInstanceState` ends the
  session as a notice (no veto): subscribers are told the process may be killed. The notice repeats
  after every `onStop`, so it asks for saving, not for closing.

### Changed

- **The Android runner no longer derives the application lifecycle from focus** (`flui-app`):
  `AppLifecycleState` comes from the same visible, running and focused signals as on iOS, so a
  dialog or the notification shade that takes focus leaves a running Activity `Inactive`, not
  `Paused`.
