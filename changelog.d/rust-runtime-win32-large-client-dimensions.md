### Fixed

- Windows resize delivery and size getters preserve native client dimensions above 32767 instead of interpreting unsigned `WM_SIZE` dimensions as signed coordinates.
