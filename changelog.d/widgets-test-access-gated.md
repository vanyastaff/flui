### Removed

- `flui::widgets::__test_access` is no longer part of an application build: the doc-hidden module that let `flui-widgets`' own integration tests name private navigator, overlay and hero items (among them `NavigatorHandle`, which ADR-0330 removed from the public API) is built only with the crate's `testing` feature, which only its own dev-dependency turns on.
