### Changed

- `flui-platform` and `flui-foundation` no longer link `parking_lot`; their synchronization uses `std::sync`. `flui-platform` now forbids reaching `parking_lot` again, and a poisoned `DisplayRegistry` reports its internal invariant instead of minting from potentially partial state.
