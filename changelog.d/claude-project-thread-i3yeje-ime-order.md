### Added

- `flui_testing::backend_conformance` pins the IME event order: `Step::Composition` (a preedit, a
  picked candidate, a commit, a cancel) and three rows that hold the focused field's text, composing
  range and caret after each step, each step as one edit of the field. The Linux hosts'
  `text-input-v3` client, the AppKit and UIKit client and Android's `FluiView` mirror run them;
  `RowOutcome::Gap` reports a path FLUI has not driven through the table yet by its `docs/gaps.md` entry.

### Changed

- `BackendUnderTest` requires `focus_text_store`, which answers `TextInputPath::Focused`, `Gap`
  or `None`: a backend names its input method path instead of passing the IME rows by default.
