### Fixed

- `flui-system`'s documentation, its `ARCHITECTURE.md`, `docs/crates.md` and the facade's `services` documentation no longer restate which platform provides which service: that is stated once, in `docs/gaps.md` and the System services rows of `crates/flui-native/platform-parity.toml`. The facade's lists, which had left out the message box, the share sheet, the jump list, the system information and grants, now point at the crate's own list.
- `flui-system`'s `ARCHITECTURE.md` no longer says that no host refuses a labelled message box: Windows refuses one with `NotYetOnThisHost`.
