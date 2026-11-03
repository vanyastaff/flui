### Changed

- `flui_view::persist::Persisted` changes its state through `flui_foundation::panic::commit`
  (ADR-0287). A `set`, a finished load and a fresh start publish and refresh before the value they
  replaced is destroyed, each step contained. When both panic, the first failure propagates (a
  frame hook's), and the replaced value's panic is discarded behind it (ADR-0281 §4). Before, the
  destructor's panic won.

### Fixed

- A `Persisted` refresh whose released close hold panicked while it signalled the owner no longer
  leaves the document marked as watched with no watch running, which kept later writes from being
  told and their close hold from being released.
- A `Persisted::set` or fresh start whose encode panicked, and whose failure then woke a panicking
  frame hook, no longer aborts when the refused value's destructor panics too. The value is
  destroyed contained after the failure is recorded, and the hook's panic propagates.
