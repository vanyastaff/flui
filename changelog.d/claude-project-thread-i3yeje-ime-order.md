### Added

- `flui_testing::backend_conformance` pins the IME event order: `Step::Composition` (a preedit, a
  picked candidate, a commit, a cancel) and three rows that hold the focused field's text, composing
  range and caret after each step, each step as one edit of the field. The Linux hosts'
  `text-input-v3` client, TSF's document on Windows, the AppKit and UIKit client, Android's
  `FluiView` mirror and the Web's
  `EditContext` and hidden textarea run them;
  `RowOutcome::Gap` reports a path FLUI has not driven through the table yet by its `docs/gaps.md` entry.

### Changed

- `BackendUnderTest` requires `focus_text_store`, which answers `TextInputPath::Focused`, `Gap`
  or `None`: a backend names its input method path instead of passing the IME rows by default.

### Fixed

- On the Web, a hidden textarea's composition keeps its start when a character is typed into the
  preedit or deleted from it: the composing range grows or shrinks over the edit instead of
  covering only the changed characters. An empty composition, as after a cancel, ends.
