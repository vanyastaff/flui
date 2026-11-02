### Added

- **`flui-platform`**: `ProviderContext::runtime_local::<T>()` and `RuntimeLocals`, the one place a
  capability provider keeps state for one UI runtime, shared by every handle it hands out there
  and retired with the runtime after its pending completions
  ([ADR-0152 §5](/docs/adr/ADR-0152-capability-seam-revised.md)).

### Changed

- **`flui-interaction`, `flui-platform`**: a clipboard or primary-selection read answers
  `ClipboardRead`, `Result<Option<Arc<str>>, ClipboardError>`, in place of `Option<String>`.
  `Ok(None)` is a clipboard without text; a host that could not read answers why
  (`Unavailable`, `Denied`, `Timeout`, `Malformed`, `TooLarge`), so a failed read no longer looks
  like an empty clipboard. Reads asked in one UI runtime while one is in flight share it: the host
  is asked once, and every reader holds the same copy of the text
  ([ADR-0152 §5](/docs/adr/ADR-0152-capability-seam-revised.md)).
- **`flui-interaction`**: `ClipboardHandle::new` and `PrimarySelectionHandle::new` take the
  provider's `&ProviderContext` in place of a `CompletionSource`; `ProviderContext::new` takes the
  runtime's `RuntimeLocals`.
- **`flui-testing`**: `ScriptedClipboard` scripts a failing host with `fail_reads_with` and
  `stop_failing_reads`, and counts the reads it was asked with `reads_asked`.
