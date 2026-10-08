### Added

- Nested clamping scrollables transfer the remaining boundary velocity to the nearest enclosing scrollable that can move on the same axis, preserving reversed directions. Replaced or unmounted owners and canceled animation runs cannot receive a delayed impulse.

### Fixed

- Ordinary rebuilds with the same scroll controllers preserve an accepted nested fling when using default scroll physics.
