# AGENTS.md — flui-painting

The recorder (`Canvas` → `DisplayList`) and the text stack (`TextLayout`,
`TextPainter` over cosmic-text). Nothing is rasterised here.

## What lives here

- `Canvas` — the `dart:ui` drawing surface; user-facing through `CustomPaint`,
  so its method set follows `dart:ui`, not the workspace's current callers.
- `DisplayList` / `DrawCommand` — the closed wire vocabulary `flui-engine`
  matches exhaustively (no `#[non_exhaustive]`, no wildcard arm there).
- `TextPainter` / `TextLayout` / `SharedFontSystem` — shaping through the
  process-wide font system the engine shares (ADR-0016).
- `paint_box_decoration`, `paint_table_border`.

## Key constraints

- `#![forbid(unsafe_code)]`.
- `DisplayList` has no `&mut` surface; `bounds()` is `Option<Rect>` (a list of
  only clips has no extent).
- The font system's lock is non-reentrant: never call a text API inside
  `SharedFontSystem::with_mut`.
- `Paint`/`Shader`/`Path` live in `flui-types`; consumers import them from
  there (this crate re-exports the paint vocabulary at its root only).
- `testing` feature: `crate::testing::record` and
  `text_layout::init_font_system_with_faces`; enabled for this crate's own
  tests via the self dev-dependency.

See `ARCHITECTURE.md` for the module map, mapping decisions, and open items
(shaped-IR text, font-context handle, command size).
