# flui-cupertino

iOS-style (Cupertino) theming foundation for [FLUI](https://github.com/vanyastaff/flui):
`CupertinoDynamicColor`/`CupertinoColors`, `CupertinoTextThemeData`, `CupertinoTheme`/
`CupertinoThemeData`, the `CupertinoApp` application shell, and the Cupertino widget family —
`CupertinoButton`, `CupertinoNavigationBar`, `CupertinoTabBar`/`CupertinoTabScaffold`,
`CupertinoPageScaffold`, and `cupertino_page_route`. It is the Rust analog of
`package:flutter/cupertino.dart`'s theming and widget surface.

## Enable via the `flui` facade

`flui`'s default feature set is Material-only, so Cupertino needs an explicit opt-in:

```toml
[dependencies]
flui = { version = "0.2", features = ["cupertino"] }
```

To depend on it directly instead:

```toml
[dependencies]
flui-cupertino = "0.2"
```

Neither crate is published to crates.io yet; until the first release, depend
on it via a git tag or path — see the [flui facade's README](../../README.md).

## Example

```rust
use flui_cupertino::{CupertinoTheme, CupertinoThemeData};
use flui_widgets::SizedBox;

let _themed = CupertinoTheme::new(CupertinoThemeData::default(), SizedBox::shrink());
```

## See also

- [`flui-material`](../flui-material) — the Material Design theming counterpart
- [`flui-widgets`](../flui-widgets) — the design-neutral widget catalog this crate themes
- [`docs/adr/ADR-0042-theming-ownership.md`](../../docs/adr/ADR-0042-theming-ownership.md) — why theming lives in the catalog crates, not the facade
