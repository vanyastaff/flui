### Added

- `flui_platform::SessionEndSupport` (`Hold`, `Delay { max }`, `NotifyOnly`) and
  `OwnerPlatform::session_end_support`: the platform says whether it holds a session end for a
  block (Windows, macOS, headless), waits a bounded time (Linux, through logind's delay
  inhibitor) or only tells (Android, iOS, the web).
- `RuntimeStatus::checked_out`.
- `flui_view::__runtime::SESSION_END_BUDGET`, which `flui_testing::SESSION_END_BUDGET` now
  re-exports.
- `HeadlessPlatform::with_session_end_support`, so a test stands in for a phone or a browser tab.

### Changed

- A session-end query asks every window's close handler, then its guard, with
  `CloseReason::SessionEnd` and blocks when one refuses; a window whose UI runtime is checked out
  is not asked. A cancelled session end withdraws only the session-end closes it recorded. The
  ending phase writes the flush registry inline within `SESSION_END_BUDGET` and, unless the
  platform only tells (`NotifyOnly`: an Android stop, a web pagehide), asks the loop to quit
  (ADR-0260). A window that refused holds the session until it carries its close out. Before
  the flush, the ending phase commits each window's composition and tells its `Detached` observers
  (only where the application ends), and window placements are written within the same budget.
- Whether a guard may refuse a session end follows the platform's `SessionEndSupport` instead of
  the target: only a platform that holds the session lets it.
- `RuntimeStatus` is `#[non_exhaustive]`.
