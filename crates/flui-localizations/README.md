# flui-localizations

Global (multi-language) localized resources for [FLUI](https://github.com/vanyastaff/flui)'s
widget catalog — the Rust analog of Flutter's `flutter_localizations` package.
`GlobalWidgetsLocalizations` resolves a correct [`TextDirection`] (RTL for Arabic, Farsi,
Hebrew, Pashto and Urdu; LTR otherwise) for any [`Locale`], and
`GlobalWidgetsLocalizationsDelegate` wires that into `flui-widgets`'
`LocalizationsDelegate`/`Localizations` machinery. Per-language translated strings are not yet
ported — every locale currently gets English button labels alongside its resolved direction;
see the crate's own module docs for the tracked gap.

## Enable via the `flui` facade

```toml
[dependencies]
flui = { version = "0.2", features = ["localizations"] }
```

To depend on it directly instead:

```toml
[dependencies]
flui-localizations = "0.2"
```

This crate isn't published to crates.io yet; until the first release, depend
on it via a git tag or path — see the [flui facade's README](../../README.md).

## Example

```rust
use flui_localizations::GlobalWidgetsLocalizations;
use flui_types::platform::Locale;

let ar = Locale::new("ar", None::<&str>);
assert!(GlobalWidgetsLocalizations::is_rtl_language(ar.language()));
```

## See also

- [`flui-widgets`](../flui-widgets) — owns the `LocalizationsDelegate`/`WidgetsLocalizations` contracts this crate implements
- [`flui-material`](../../packages/flui-material), [`flui-cupertino`](../flui-cupertino) — the catalogs whose own localization implementations (when they land) will be independent of this crate, per its module docs
