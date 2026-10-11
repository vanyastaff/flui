### Fixed

- Pointer-route coordinate conversion preserves every admitted measured and predicted reading when distinct source positions round to the same local position. `PointerMove::try_map_positions` supports checked coordinate conversion without changing timestamps, sensors or history membership.
