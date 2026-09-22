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

To depend on it directly instead (or if you've turned off `flui`'s default features):

```toml
[dependencies]
flui = { version = "0.2", default-features = false, features = ["material"] }
```

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
