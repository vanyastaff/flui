# Widget catalog

FLUI is not yet published to crates.io, so there is no live docs.rs page to link to yet — until
there is, this page points at the source tree and the runnable catalog instead of listing every
widget by name here (a hand-maintained list would go stale the moment a widget is added; the
source directories and the gallery example do not). Screenshots are planned for a later pass of
this book (see the beta roadmap's H2 tracking).

## See it running

```bash
cargo run --example widgets_gallery
```

opens a window walking the catalog live — see [Getting Started](../getting-started/installation.md) for
how to build and run it. [`examples/README.md`](https://github.com/vanyastaff/flui/blob/main/examples/README.md)
indexes every bundled example by category, several of which exercise a specific widget or widget
group in isolation.

## Source, by crate

- **`flui-widgets`** — the core, theme-independent catalog: layout (flex, stack, wrap, clip,
  physical model), scrolling, text, images, icons, animation/transitions, overlays, navigation, and
  the app-level `container`/`app` scaffolding.
  [`crates/flui-widgets/src/`](https://github.com/vanyastaff/flui/tree/main/crates/flui-widgets/src)
- **`flui-material`** — Material Design widgets (buttons, app bar, scaffold, cards, dialogs,
  navigation, form controls) built on `flui-widgets` plus `ColorScheme`/`ThemeData`, an
  official package on `flui-sdk`.
  [`packages/flui-material/src/`](https://github.com/vanyastaff/flui/tree/main/packages/flui-material/src)
- **`flui-cupertino`** — iOS-style widgets (buttons, nav bar, page/tab scaffold, theming) built on
  `flui-widgets` plus `CupertinoTheme`.
  [`crates/flui-cupertino/src/`](https://github.com/vanyastaff/flui/tree/main/crates/flui-cupertino/src)

Once the framework crates publish (see `docs/ROADMAP.md`), this page becomes a thin index into
docs.rs instead of the source tree directly.
