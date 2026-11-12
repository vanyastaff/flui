### Changed

- `open_window`, `open_secondary_window` and `WindowPolicy` (at `flui::`, `flui::prelude`,
  `flui::app` and `flui_app::`) are published only under the new additive `unstable-windows`
  feature of `flui` and `flui-app`. Without it an application has one window: a `Shared` window is
  still refused and an open answers only that it was admitted, until ADR-0091 §3–§5 are built.
  An application that opens a second window adds `features = ["unstable-windows"]`.
