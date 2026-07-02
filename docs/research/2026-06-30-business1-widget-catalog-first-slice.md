# Business.1 — widget-catalog first-slice plan

flui-widgets is "spine ~85%, catalog ~2%" (ROADMAP.md:27). The machinery + 14
families exist and are render-backed; the gap is breadth. This plans the first
actionable slice — widget-layer-only wins over render objects that already exist.

## Inventory (have)
14 families in `crates/flui-widgets/src/lib.rs`, each a thin config over an
existing `flui-objects` render object: container, flex (Row/Column/Flexible/
Expanded), text (Text/EditableText/TextField), scroll (ListView/Viewport/
Scrollable/Scrollbar/SingleChildScrollView/SliverList/SliverFixedExtentList/
SliverPadding/SliverOpacity/SliverToBoxAdapter), animated, stack, clip, wrap,
image, transitions, paint, layout (Align/Center/Padding/SizedBox/ConstrainedBox/
AspectRatio/IntrinsicWidth/Height/FractionallySizedBox/…), interaction
(GestureDetector/Listener/AbsorbPointer/IgnorePointer/Offstage), app (MediaQuery/
Theme). The layout family is unusually complete.

## First slice (widget-layer-only; each gets a tests/parity/ port)
Templates: eager sliver widget = `scroll/sliver_fixed_extent_list.rs`; scroll-view
composition = `scroll/list_view.rs` (StatelessView → `Viewport::new((sliver,))`).

1. **SliverGrid** (eager) — `RenderView` over `RenderSliverGrid::new(delegate)`,
   Arity=Variable. RO ✅ ungated (`flui-objects/src/sliver/sliver_grid.rs:39`;
   delegates on default build `flui-rendering/src/lib.rs:178-183`). Oracle
   `sliver.dart:739`.
2. **GridView** (`GridView.count`, `GridView.extent`; eager) — **START HERE** —
   StatelessView → `Viewport::new((SliverGrid…,))` with axis + programmatic offset.
   The single named Business.1 RO blocker, now unblocked. Oracle
   `scroll_view.dart:1976`; parity `grid_view_layout_test.dart` (tile geometry).
3. **Spacer** — `Expanded::flex(flex).child(SizedBox::shrink())`. ~15 lines.
   Oracle `spacer.dart`; parity `spacer_test.dart`.
4. **SafeArea** — reads `MediaQueryData.padding` (`app/media_query.rs:69`) → `Padding`
   with per-edge toggles + `minimum`. Oracle `safe_area.dart`.
5. **Visibility** — `visible`→child else replacement; `maintainState`→`Offstage`;
   `maintainInteractivity`→`IgnorePointer`. Defer `maintainAnimation` (needs
   `TickerMode`, absent — document divergence). Oracle `visibility.dart`.

Re-export new widgets from `crates/flui-widgets/src/lib.rs`; register each parity
port in `crates/flui-widgets/tests/parity/main.rs`.

## Deferred (need render/element work or an ADR — NOT first slice)
- **GridView.builder (lazy)** — needs `RenderSliverGridLazy` (analog of
  `sliver_list_lazy.rs`) + lazy-grid element wiring. Core.2-shaped.
- **CustomScrollView** — widget-only (Viewport over a ViewSeq of slivers); a good
  SECOND slice (unlocks composing the Sliver* widgets). Plus the eager Sliver fill
  wrappers (RenderSliverFillViewport/Remaining/Offstage/IgnorePointer exist).
- **CustomPaint / Flow / Table / Custom{Single,Multi}ChildLayout** — ROs don't
  exist; delegates gated (`delegates/mod.rs:31-51`). Core.2 render work.
- **LayoutBuilder** — build-during-layout re-entrancy → ADR (locked phase order).
- **Focus / FocusScope** — Cross.H D-10 → ADR/foundation work.

## Approach
Each widget = a small StatelessView/RenderView mirroring an existing template +
an oracle-cited parity test in the live `tests/parity/` harness (screen() +
pump_widget). Gate per widget: parity geometry test green + zero regressions.
