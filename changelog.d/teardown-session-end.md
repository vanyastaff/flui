### Added

- `flui_platform::SessionEndSupport` (`Hold`, `Delay { max }`, `NotifyOnly`) and
  `OwnerPlatform::session_end_support`: the platform says whether it holds a session end for a
  block (Windows, macOS, headless), waits a bounded time (Linux, through logind's delay
  inhibitor) or only tells (Android, iOS, the web).
- `RuntimeStatus::checked_out`.
- `flui_view::__runtime::SESSION_END_BUDGET`, which `flui_testing::SESSION_END_BUDGET` now
  re-exports.

### Changed

- A session-end query asks every window's close handler and guard with
  `CloseReason::SessionEnd` and blocks when one refuses; a window whose UI runtime is checked out
  is not asked. A cancelled session end withdraws only the session-end closes it recorded. The
  ending phase writes the flush registry inline within `SESSION_END_BUDGET` and asks the loop to
  quit (ADR-0260).
- Whether a guard may refuse a session end follows the platform's `SessionEndSupport` instead of
  the target: only a platform that holds the session lets it.
- `RuntimeStatus` is `#[non_exhaustive]`.
