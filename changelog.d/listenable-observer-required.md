### Changed

- **`Listenable` has one required registration method** (`flui-foundation`): implementors write
  `add_observer(ListenerObserver)`, which receives the notifying delivery's `RecoveryScope`;
  `add_listener` is now a provided adapter over it. `RecoveryScope` and `ListenerObserver` are
  documented public API (`ListenerObserver` is re-exported at the crate root), and
  `RecoveryScope::boundary` opens the outermost scope for a source that notifies outside a
  foundation channel. A custom `Listenable` that only implemented `add_listener` must add
  `add_observer`, usually forwarding to its `ChangeNotifier`.

### Fixed

- **Relays on controllers lost failure custody** (`flui-widgets`, `flui-rendering`,
  `flui-cupertino`): `TextEditingController`, `WidgetStatesController`,
  `TransformationController`, `RefreshController`, `ScrollPosition` and `CupertinoTabController`
  now forward framework relays to their notifier, so an observer such as `AnimatedBuilder` runs
  inside the enclosing delivery's recovery scope instead of a fresh one after an earlier listener
  panicked.
