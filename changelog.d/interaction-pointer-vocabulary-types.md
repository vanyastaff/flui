### Added

- **`flui-platform-api`**: FLUI's own input vocabulary, `pointer` (pointer, scroll and trackpad
  pan/zoom events with pen tools, sensors that read `None` when a device has none, cancel
  reasons and device changes), `keyboard` (`KeyEvent`, `Key`, `NamedKey`, `Code`, `Location`,
  `Modifiers`) and `EventTime`. Constructors refuse or neutralize non-finite input. The
  pipeline still carries the `ui-events` types; it switches to these in a later release
  ([ADR-0143](/docs/adr/ADR-0143-flui-owned-input-event-vocabulary.md)).
- **`cargo xtask key-vocabulary`**: generates `NamedKey` and `Code` from the pinned
  `keyboard-types`; `cargo xtask checks` fails when they drift.
