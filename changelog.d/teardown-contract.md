### Added

- `flui::view::CloseReason` (`User`, `Program`, `SessionEnd`; also `flui::app::CloseReason`) and `CloseRequest::reason`. Every request reads as `User` for now.
- `flui::view::CloseGuard`, `CloseHold`, `PendingClose` and `CloseChanged`, acquired through `LifecycleContext::close_guard`, for holding a window's close while work finishes. No host installs a guard yet, so `close_guard` answers `None`.
- `LifecycleContext::flush_registry`, the host's registry of published document bytes; no host keeps one yet, so it answers `None`.
- `FrameFailureKind::CallbackPanic`, the report for an application callback panic contained outside a frame. Nothing reports it yet.
- `flui::testing::widgets::LaidOut::request_close` and `LaidOut::end_session`; both close the presentation as the host does today, without consulting a close guard or flushing.
