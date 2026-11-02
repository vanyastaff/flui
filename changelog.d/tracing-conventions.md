### Changed

- **Tracing targets are `flui.<subsystem>`**: frame phase spans are under `flui.frame`,
  `element_rebuild` and the reconcile events under `flui.tree`, recognizer spans under
  `flui.input`, animation spans under `flui.animation`, signals and accessibility fallbacks under
  `flui.signals` and `flui.a11y`. `flui::reconcile`, `flui_animation`, `flui::signals` and
  `flui::a11y` keep working in a `RUST_LOG` directive for one release (`flui_log::TARGET_ALIASES`).
  `flui_view::RECONCILE_TARGET` is now `"flui.tree.reconcile"`.
- **Frame spans carry primitives**: ids are `u64` and `element_rebuild` moves from INFO to TRACE
  with its reasons as a bitmask (`RebuildReasons::bits`). The `semantics`, `commit` and `submit`
  phases get DEBUG spans, relayout roots get a TRACE `layout_root` span, and the recognizer spans
  that were created but never entered are now entered TRACE spans.
- **`flui-devtools`' frame profiler recognizes a phase by target and name**, so an application
  span named `paint` is no longer counted as the paint phase.
