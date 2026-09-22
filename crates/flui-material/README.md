# flui-material

Material Design theming foundation for [FLUI](https://github.com/vanyastaff/flui): the M3
`ColorScheme`, the 2021 type scale and `TextTheme`, `ThemeData`/`Theme`, the `MaterialApp`
shell, and the M3 widget catalog built on them — `Scaffold`/`AppBar`, the button family
(`ElevatedButton`, `FilledButton`, `OutlinedButton`, `TextButton`, `IconButton`,
`FloatingActionButton`), `Card`, `Dialog`/`AlertDialog`, `TextField`/`InputDecoration`,
`ListTile`, `NavigationBar`, `Chip`, and tabs — the Rust analog of
`package:flutter/material.dart`'s theming and widget surface (see the crate's own
[module docs](src/lib.rs) for per-widget Flutter oracle citations).

## Enable via the `flui` facade

`flui`'s `material` feature is enabled by default, so most consumers need nothing extra:

```toml
[dependencies]
flui = "0.2"
```

With default features off, request it explicitly:

```toml
[dependencies]
flui = { version = "0.2", default-features = false, features = ["material"] }
```

This crate isn't published to crates.io yet; until the first release, depend
on it via a git tag or path — see the [flui facade's README](../../README.md).

## Example

```rust
use flui_material::{Theme, ThemeData};
use flui_widgets::SizedBox;

let _themed = Theme::new(ThemeData::dark(), SizedBox::shrink());
```

## See also

- [`flui-cupertino`](../flui-cupertino) — the iOS-style theming counterpart
- [`flui-widgets`](../flui-widgets) — the design-neutral widget catalog this crate themes
- [`docs/adr/ADR-0042-theming-ownership.md`](../../docs/adr/ADR-0042-theming-ownership.md) — why theming lives in the catalog crates, not the facade
