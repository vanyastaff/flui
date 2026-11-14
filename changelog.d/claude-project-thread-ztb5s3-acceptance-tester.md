### Added

- `Tester::relaunch_after_kill`: the process is killed and the application opened again over the
  same storage, so what the run had not written is lost; `Tester::restart` stays the quit that
  writes.
- `Tester::set_window_visible`: the window is hidden or shown again, as when the user minimizes it,
  and the frame that reports it runs.
- `flui::testing::{DropLedger, DropSentinel}`: a test counts the states, pages and listeners still
  alive after a close, beside `Tester` rather than under the deprecated `flui::testing::harness`.
