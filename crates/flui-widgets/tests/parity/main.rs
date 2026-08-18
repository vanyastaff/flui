//! Flutter parity tests — geometry assertions ported from the Flutter widget
//! test suite, run against FLUI's headless layout harness.
//!
//! Each sub-module cites the Flutter source file and line number it mirrors,
//! documents the widget → render-object type mapping, and records any
//! intentional divergences from Flutter behaviour.
//!
//! Phase covered: C1.13 (Core.1 exit gate) — geometry assertions only.
//! Paint, semantics, and the wider ~150-test corpus are Phase 3 (deferred).
//!
//! The suite is inventoried executably by `manifest.toml` in this directory,
//! validated by the sibling `parity_inventory` test target (see its module
//! doc for every checked invariant and the pinned-reference refresh
//! workflow). Adding a module here means adding a manifest row — the
//! inventory test fails until the new file is classified.

#[path = "../common/mod.rs"]
mod common;

mod harness;

// ── Phase-2 ports (no new finders needed) ────────────────────────────────────
mod column_no_overflow_fp_test;
mod container_test;
mod list_view_test;
mod stateful_test;

// ── Phase-2 ports (use find_by_render_type / pump_widget) ────────────────────
mod center_test;
mod flex_test;
mod harness_self_test;
mod sized_box_test;
mod text_test;

// ── Business.1 slice — widget-catalog first five ──────────────────────────────
mod grid_view_test;
mod icon_test;
mod safe_area_test;
mod sliver_grid_test;
mod sliver_main_axis_group_test;
mod sliver_persistent_header_test;
mod spacer_test;
mod visibility_test;

// ── Business.1 slice — CustomPaint ───────────────────────────────────────────
mod custom_paint_test;

// ── Business.1 slice — CustomSingleChildLayout ───────────────────────────────
mod custom_single_child_layout_test;

// ── Business.1 slice — CustomMultiChildLayout ─────────────────────────────
mod custom_multi_child_layout_test;

// ── Business.1 slice 2 — CustomScrollView + eager sliver-fill wrappers ───────
mod custom_scroll_view_test;
mod sliver_fill_remaining_test;
mod sliver_fill_viewport_test;
mod sliver_fixed_extent_list_test;
mod sliver_ignore_pointer_test;
mod sliver_list_constructors_test;
mod sliver_list_test;
mod sliver_offstage_test;
mod sliver_padding_test;

// ── Core.2 — RenderFlow / Flow ───────────────────────────────────────────────
mod flow_test;

// ── Core.2 — RenderTable / Table ─────────────────────────────────────────────
mod table_test;

// ── Core.1 exit gate — slice-widget parity ports (padding, gestures, scroll,
//    implicit animations) ─────────────────────────────────────────────────
mod gesture_detector_test;
mod gesture_timing_test;
mod implicit_animations_test;
mod padding_test;
mod scroll_controller_test;
mod scrollable_test;
mod single_child_scroll_view_test;

// ── Business.1 fidelity front — flex/stack parity (family 2) ────────────────
mod stack_test;

// ── Business.1 fidelity front — Navigator/routes parity (family 3) ──────────
mod navigator_test;
mod pop_scope_test;

// ── Business.1 fidelity front — Hero parity (family 3, heroes) ──────────────
mod heroes_test;

// ── Business.1 fidelity front — Overlay parity (Navigator prerequisite,
//    ADR-0036) ────────────────────────────────────────────────────────────
mod overlay_test;

// ── Catalog.1 — theming + localizations substrate ────────────────────────────
mod localizations_test;

// ── Business.1 fidelity front — Focus/FocusScope parity (family 4) ──────────
mod focus_test;

// ── Business.1 fidelity front — Shortcuts/Actions parity (family 5) ─────────
mod shortcuts_test;

// ── Business.1 fidelity front — Scrollbar parity (family 6) ─────────────────
mod scrollbar_test;

// ── Business.1 fidelity front — implicit-animation family parity (family 7:
//    AnimatedContainer/Size/Align/Padding; AnimatedOpacity stays in
//    implicit_animations_test.rs, its own oracle's home) ────────────────────
mod animated_align_test;
mod animated_container_test;
mod animated_padding_test;
mod animated_size_test;
mod animated_switcher_test;

// ── Business.1 fidelity front — Clip family parity (family 8) ───────────────
mod clip_test;

// ── Business.1 fidelity front — Transform family parity (family 9) ──────────
mod transform_test;

// ── Business.1 fidelity front — layout-trio parity (Wrap / FittedBox /
//    ConstrainedBox) ─────────────────────────────────────────────────────
mod constrained_box_test;
mod fitted_box_test;
mod wrap_test;

// ── Business.1 fidelity front — ValueListenableBuilder parity ───────────────
mod value_listenable_builder_test;

// ── Business.1 fidelity front — Dismissible parity (gesture-heavy widget) ───
mod dismissible_test;

// ── Business.1 implementation-gated fidelity unit — Draggable/DragTarget ────
mod draggable_test;

// ── Business.1 implementation-gated fidelity unit — InteractiveViewer ───────
mod interactive_viewer_test;

// ── Paging (ADR-0037) — PageView ─────────────────────────────────────────────
mod page_view_test;

// ── Business.1 fidelity — EditableText / TextEditingController parity ───────
mod editable_text_test;
mod text_editing_controller_test;

// ── Business.1 fidelity — LayoutBuilder parity (build-during-layout seam) ───
mod layout_builder_test;

// ── Business.1 fidelity — AspectRatio parity ────────────────────────────────
mod aspect_ratio_test;

// ── Business.1 fidelity — OverflowBox / SizedOverflowBox parity ────────────
mod overflow_box_test;

// ── Business.1 fidelity — Opacity parity ────────────────────────────────────
mod opacity_test;

// ── Business.1 fidelity — FractionallySizedBox parity ───────────────────────
mod fractionally_sized_box_test;

// ── Business.1 fidelity — Offstage parity ───────────────────────────────────
mod offstage_test;

// ── Business.1 fidelity — RotatedBox parity ─────────────────────────────────
mod rotated_box_test;

// ── Business.1 fidelity — pointer hit-test parity ──
mod pointer_hit_test_test;
mod pointer_local_position_test;

// ── Business.1 fidelity — MouseRegion parity ────────────────────────────────
mod mouse_region_test;

// ── Business.1 fidelity — Align parity ──────────────────────────────────────
mod align_test;

// ── Business.1 fidelity — Viewport parity (slivers_test.dart Viewport
//    subject) ────────────────────────────────────────────────────────────
mod viewport_test;

// ── Business.1 fidelity front — SliverList scrollOffsetCorrection parity
//    (slivers_test.dart SliverList scroll-offset-correction subject) ──────
mod sliver_list_correction_test;

// ── Business.1 fidelity front — slivers_test.dart closeout (ErrorWidget
//    override contract + the SliverOpacity 'painting & semantics' subject) ─
mod error_widget_test;
mod sliver_opacity_test;

// ── Physical-layer widgets — PhysicalModel / PhysicalShape parity ──────────
mod physical_model_test;

// ── Business.1 fidelity — Row parity (row_test.dart) ────────────────────────
mod row_baseline_test;
mod row_test;

// ── Flex ambient-Directionality fix — Column cross-axis RTL (rendering/
//    flex_test.dart's 'Flex RTL' case, the vertical-direction half of the
//    same RenderFlex.textDirection gap row_test.rs's RTL cases close) ──────
mod column_test;

// ── Business.1 fidelity — GridView interaction parity (grid_view_test.dart,
//    distinct oracle from grid_view_test.rs's grid_view_layout_test.dart) ──
mod grid_view_interaction_test;

// ── Business.1 fidelity — ColoredBox parity (basic_test.dart ColoredBox
//    group) ───────────────────────────────────────────────────────────────
mod colored_box_test;

// ── Business.1 fidelity — UnconstrainedBox parity (basic_test.dart
//    UnconstrainedBox group) ───────────────────────────────────────────────
mod unconstrained_box_test;

// ── Business.1 fidelity — ListBody parity (list_body_test.dart) ────────────
mod list_body_test;

// ── Business.1 fidelity — RichText parity (rich_text_test.dart) ────────────
mod rich_text_test;

// ── FadeTransition parity (fade_transition_test.dart) ──────────────────────
mod fade_transition_test;

// ── DefaultTextStyle parity (default_text_style_test.dart) ─────────────────
mod default_text_style_test;

// ── Directionality parity (directionality_test.dart) ────────────────────────
mod directionality_test;
