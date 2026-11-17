### Changed

- `flui_foundation::notifier_generic` is part of `flui_foundation::notifier`, and its
  `ArgCallback<Arg>` is `NotifierCallback<Arg>`.
- `flui_foundation::slot_index` is `slot_table`, `claim_slot` is `claim` and `slot` is
  `child_slot`; `text_sizing` and `TextDirection` are one module, `text`.
- `AsyncSnapshot` and `ConnectionState` moved from `flui::foundation` to `flui::widgets`, beside
  `FutureBuilder` and `StreamBuilder`, their only users.
- `Matrix4` is indexed by `(row, column)`: `m[(0, 3)]` replaces `m.get(0, 3)` and
  `*m.get_mut(0, 3)`.
- `Snapshot::get_fact` is `Snapshot::fact_as`.

### Removed

- `flui::foundation::EPSILON` and its `consts` module: layout's rounding tolerance is
  `flui::geometry::PRECISION_ERROR_TOLERANCE`.
- `ValueNotifier::set_value_force`, which nothing called.
