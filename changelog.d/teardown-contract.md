### Added

- `flui::view::CloseReason` (`User`, `Program`, `SessionEnd`; also `flui::app::CloseReason`) and `CloseRequest::reason`. Every request reads as `User` for now.
- `flui::view::CloseGuard`, `CloseHold`, `PendingClose`, `CloseChanged` and `StaleClose`, acquired through `LifecycleContext::close_guard`, for holding a window's close while work finishes. They stay on the owner thread (`!Send`). No host installs a guard yet, so `close_guard` answers `None`.
- `FrameFailureKind::CallbackPanic`, the report for an application callback panic contained outside a frame. Nothing reports it yet.
- `flui::testing::widgets::LaidOut::request_close` and `LaidOut::end_session`; both close the presentation as the host does today, without consulting a close guard or flushing.

### Changed

- `LifecycleContext::storage` documents that a value written there directly is not flushed when the application or the session ends.
