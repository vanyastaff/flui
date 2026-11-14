### Added

- `Tester::relaunch_after_kill`: the process is killed and the application opened again over the
  same storage, so what the run had not written is lost, a write `MemoryStorage` holds for it
  included, while another run's writes and the test's own are kept; `Tester::restart` stays the
  quit that writes. Under a write hold a newer value replaces a waiting one only from the same run;
  a quit write the hold kept lands ahead of the next run's.
- `Tester::set_window_visible`: the window is hidden or shown again, as when the user minimizes it;
  its focus stays as it was, and the frame that reports it runs. `HeadlessHost::set_window_visible`
  reports visibility alone and no longer marks the window focused.
- `flui::testing::{DropLedger, DropSentinel}`: a test counts the states, pages and listeners still
  alive after a close, beside `Tester` rather than under the deprecated `flui::testing::harness`.
