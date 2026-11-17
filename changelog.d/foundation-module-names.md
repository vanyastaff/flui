### Changed

- `flui::foundation` holds what an application names: keys, listenables, the tree ids and errors,
  text direction and sizing, `AppId`, `inspect` and `observe`. The frame arena, claim slots, id
  mints, slot tables, tickets, the system clock, the signal graph's read side and
  panic containment other than `RecoveryScope` left it; a package reaches them through
  `flui_sdk::foundation`. The arity markers are named at `flui::rendering`, geometry at
  `flui::geometry`.
- `flui_foundation` names each item at one path. An item of a single-concept module is named at
  the crate root (`flui_foundation::AsViewKey`, `flui_foundation::GenId`,
  `flui_foundation::MonotonicClock`); the modules `notifier`, `key`, `id`, `sequence`, `clock`,
  `claim`, `text` and the rest of that kind are private. `geometry`, `inspect`, `panic`,
  `memory`, `observe`, `generation`, `arena`, `diagnostics` and `read_scope` stay modules, with
  no copy of their items at the root.
- `Notifier<Arg>`'s callback `ArgCallback<Arg>` is `NotifierCallback<Arg>`.
- `AsyncSnapshot` and `ConnectionState` moved from `flui::foundation` to `flui::widgets`, beside
  `FutureBuilder` and `StreamBuilder`, their only users.
- `Matrix4` is indexed by `(row, column)`: `m[(0, 3)]` replaces `m.get(0, 3)` and
  `*m.get_mut(0, 3)`.
- `Snapshot::get_fact` is `Snapshot::fact_as`.

### Removed

- `flui_foundation::prelude`, which nothing imported.
- `flui::foundation::EPSILON` and its `consts` module: layout's rounding tolerance is
  `flui::geometry::PRECISION_ERROR_TOLERANCE`.
- `ValueNotifier::set_value_force`, which nothing called.

### Fixed

- `FrameEpoch::next` and `SurfaceGeneration::next` refuse to wrap in a release build too; a wrap
  would make an earlier frame or surface current again.
- A notifier's events (a listener's panic, use after dispose) carry the target `flui.notifier`
  instead of the module path, so a log filter on them does not move with the source.
