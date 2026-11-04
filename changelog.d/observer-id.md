### Removed

- **`flui_foundation::ObserverId`**: a plain `Id<Observer>` with no caller anywhere in the
  workspace (code, tests, docs, facade) — a public, forgeable identity type that named nothing:
  no `ObserverList` exists on main, and no observer registration hands ids out. `ListenerId`, a
  listener's id, stays a minted id of its channel.
