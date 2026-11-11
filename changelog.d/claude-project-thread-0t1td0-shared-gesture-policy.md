### Changed

- `GesturePolicy` is one value its `GestureArenaScope` resolves and every
  consumer beneath the scope shares. `GesturePolicy::arena()` and
  `GesturePolicy::settings()` replace the public fields, and holding or
  cloning a policy costs one reference in place of a copy of the arena handle
  and the settings.
- `GestureArenaScope` keeps what it publishes in its state
  (`GestureArenaScopeState`), so a rebuild that changes neither its arena nor
  its policies hands consumers the policy they already hold.
- A mounted `RawButton` adds 40 allocations and 4966 bytes to its child
  (41 and 5262 before): its tap keeps one cell for its callbacks, the source
  its press group reads and the factory of that group.
