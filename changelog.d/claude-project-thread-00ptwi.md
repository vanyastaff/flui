### Added

- **`flui-interaction`**: `TextInputOwner::holds_failure`, and **`flui-platform`**:
  `CommitGate::holds_failure` is public: whether a failure owner code raised after a grant waits
  for its owner's next turn.

### Fixed

- **`flui-runtime`**: a press anywhere in a presentation commits the focused field's composition
  before the press is held, hit-tested or handled, so a "Save" button's handler reads the text the
  user sees ([ADR-0142](/docs/adr/ADR-0142-text-store-commit-gate-and-owner-code-containment.md)
  item 4).
- **`flui-runtime`**: an input method's grant queued in a frame that unwound runs at the UI
  runtime's next owner turn (an inbox drain, a background pump, or a pump before its frame)
  instead of waiting for a frame that may never come, and a failure an `on_changed` raised after a
  grant the platform asked for directly is reported at that turn
  ([ADR-0142](/docs/adr/ADR-0142-text-store-commit-gate-and-owner-code-containment.md) item 5).
- **`flui-runtime`**: while an owner lease holds a UI runtime, a text service that reaches a
  field's store from outside the runtime's own calls (a message loop nested in a handler) gets no
  synchronous lock and runs no application code; its asynchronous lock runs at the end of that
  owner turn ([ADR-0142](/docs/adr/ADR-0142-text-store-commit-gate-and-owner-code-containment.md)
  item 6). `CommitGate::set_checked_out`, `CommitGate::owner_entry`, `CommitGate::is_shut` and
  `TextInputOwner::set_checked_out` carry it.
