### Changed

- Listener registries now issue non-reused `ListenerId` values through `IdMint`; `ListenerId::get` returns `Option<u64>` to distinguish registered identities from inert admission results.
- `ConstantAnimation` returns `ListenerId::inert()` for value listeners and observers, so an unregistered result cannot remove a live callback from another source.

### Removed

- Removed numeric `ListenerId` constructors and the `usize::MAX` sentinel used by fixed animations.
