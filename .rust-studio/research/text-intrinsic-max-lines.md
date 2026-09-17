QUESTION: Is the text intrinsic/max_lines scratch failure a real flui issue or an invalid Flutter-parity oracle?

ANSWER: The original equality oracle was too strong because Flutter's own maxLines intrinsic behavior is skipped and tracked by flutter/flutter#13512. A narrower flui invariant is confirmed: non-empty visible text with `max_lines(Some(1))` can report `min_intrinsic_width() == 0.0`, and that value flows to `RenderParagraph`. Filed #1085.

VERSIONS: flui current HEAD 3eeb3322241dd8ea5ad68ffbf42118135a4f89a2; Flutter reference 3.44.0.

SOURCES:
- /tmp/flui-text-layout-audit/tests/contracts.rs:21 contains the public scratch repro.
- Scratch command `cargo nextest run --manifest-path /tmp/flui-text-layout-audit/Cargo.toml --test contracts -E 'test(max_lines_does_not_erase_widest_unbreakable_run)' --no-fail-fast` failed 1/1 with `full=13.02, clipped=0`.
- crates/flui-painting/src/text_painter/measure.rs:51 shows intrinsic widths are precomputed through `compute_layout_metrics(text, 0.0, 0.0)` and `compute_layout_metrics(text, 0.0, f32::INFINITY)`.
- crates/flui-painting/src/text_painter/measure.rs:263 shows `min_intrinsic_width()` also calls `compute_layout_metrics(text, 0.0, 0.0)` when no layout cache exists.
- crates/flui-objects/src/text/paragraph.rs:247 forwards `RenderParagraph::compute_min_intrinsic_width` directly to `TextPainter::min_intrinsic_width`.
- .flutter/packages/flutter/test/painting/text_painter_test.dart:1016 has skipped `maxLines` intrinsic expectations; .flutter/packages/flutter/test/painting/text_painter_test.dart:1059 links the skip to flutter/flutter#13512.
- https://github.com/flutter/flutter/issues/13512 is open, labelled `P2`, `a: typography`, and `c: tech-debt`.
- https://github.com/vanyastaff/flui/issues/1085 tracks the confirmed flui issue.

OPEN: Exact FLUI max_lines/ellipsis intrinsic semantics still need design; do not use the old scratch equality assertion as the full oracle.

ANSWERED
