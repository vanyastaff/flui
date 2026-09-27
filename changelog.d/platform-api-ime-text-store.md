### Added

- **The IME text-store contract** (`flui_platform_api::text_store`,
  [ADR-0090](/docs/adr/ADR-0090-ime-pull-text-store-contract.md)): a text field is a
  `TextStore` an input method locks, reads and edits in UTF-16 offsets, the shape of TSF's
  `ITextStoreACP`. A lock is a closure (`LockGrant::Read`/`ReadWrite`, so an edit under a read
  lock does not compile), granted now, queued (`Deferred`, TSF's `TS_S_ASYNC`) or refused
  (`SyncLockUnavailable`) by the shared `LockArbiter`. Commits close for the realm's frame
  transaction through a `CommitGate` the presentation installs into every store it attaches
  (`TextStore::set_commit_gate`), which the arbiter reads, so a store cannot miss it. `text_store::utf16` is the one UTF-8/UTF-16 offset converter, and
  `project_ime_event` applies a push-model `ImeEvent` to a store as edits.
- **`InMemoryTextStore`** (`flui-platform-api`): a complete store over a `String` with a fixed
  cell geometry, for tests and backends.
- **The text-store conformance kit** (`flui_testing::text_store_kit`, version 1): a field
  supplies a `TextStoreFixture` and calls `assert_conforms`; `InMemoryFixture` is the worked
  example. The built-in `EditableText` passes it, plain and obscured.
- **`TextInputClient`** (`flui-interaction`, also `flui::interaction`): what a text field
  attaches, its store and an optional session-start callback.
  `TextInputOwner::{set_transaction_open, is_transaction_open, run_deferred_grants}` carry the
  frame transaction, and `TextInputHandle::ensure_open` reports a closed presentation.
- **`RenderEditable::local_rect_for_range`** (`flui-objects`): the local rect of any byte range,
  or the caret for an empty one.

### Changed

- **`TextInputHandle::attach`** takes a `TextInputClient` instead of an `ImeEventCallback`, and
  `TextInputOwner::dispatch` projects each event onto the client's store.
- **`EditableText`** is a text store: an input method can read its text, selection, composition
  and geometry, and a platform session is one controller change and at most one `on_changed`.
  A platform selection is kept exactly, even inside a grapheme cluster; an obscured field reports
  itself protected. An IME event that arrives during a frame is applied once the frame returns.
- **Frames are text-store transactions** (`flui-app`): every runner drives a frame through
  `UiRealm::drive_frame`, which closes text-store commits from begin frame through the
  post-frame callbacks and runs the queued grants after, with the scheduler idle, so an edit
  they make schedules the next frame.

### Removed

- **`ImeEventCallback`** (`flui-interaction`, `flui::interaction`): attach a
  `TextInputClient`.
- **`TextEditingController::set_composing_text`, `commit_text` and `clear_composing`**: IME
  edits go through the field's text store (`project_ime_event` for a push event).
