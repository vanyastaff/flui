# Icon widget (B1.1) — plan

**Verdict: PARTIAL GO.** IconData + IconTheme + the Icon composition (bounded box +
codepoint plumbing) are a clean widget-layer slice buildable TODAY (zero cross-crate
change). Faithful **glyph rendering is NO-GO now** — cross-crate infra gap (see §Gap),
deferred to a font-registration ADR. Ship the widget-layer slice honestly; do NOT
assert a rendered glyph the engine can't shape.

## Oracle (`.flutter/.../widgets/icon.dart`, `icon_data.dart`, `icon_theme_data.dart`)
- `Icon.build` (icon.dart:260-357): resolve `IconTheme.of(ctx)`; `size = self.size ?? theme.size ?? 24.0`; null icon → `SizedBox.square(size)`; else build a `TextStyle{ font_family: icon.font_family, font_size: size, color: self.color ?? theme.color, height: 1.0, font_variations: fill/wght/GRAD/opsz }` over `RichText(TextSpan(String.fromCharCode(codePoint)))`, wrapped `SizedBox.square(size) → Center → RichText`; RTL flip via Transform when `match_text_direction`; `ExcludeSemantics`.
- `IconData{ code_point:u32, font_family:Option<String>, font_package:Option<String>, match_text_direction:bool=false, font_family_fallback:Vec<String> }`; value type; Display `U+{:05X}`.
- `IconThemeData::fallback()`: size 24.0, color black (0xFF000000), opacity 1.0, fill 0.0, weight 400.0, grade 0.0, optical_size 48.0, apply_text_scaling false.

## FLUI infra (exists → widget-layer GO)
- `TextStyle` has color/font_size/font_family/font_family_fallback/font_variations/shadows/height (`flui-types/src/typography/text_style.rs:198-231`). `TextSpan.text: Option<String>` (`text_spans.rs:144`).
- `RichText` composes TextSpan→RenderParagraph (`flui-widgets/src/text/rich_text.rs`). `SizedBox::square` (`layout/sized_box.rs:33`). `Center` exported. InheritedView + `impl_inherited_view!` (template: `app/media_query.rs`, `app/theme.rs`). No IconData/IconTheme today (greenfield).

## THE GAP (glyph rendering → NO-GO, needs ADR)
No bundled icon font (only Arial/Roboto in `flui-engine/assets/fonts/`). TWO disjoint FontSystems: layout/measure `static FONT_SYSTEM` (`flui-painting/src/text_layout/layout.rs:48`, private `FontSystem::new()`, no injection API) and render `font_system` (`flui-engine/src/wgpu/text.rs:360`, loads only Roboto). `FontLoader` (`wgpu/font_loader.rs`) exists but has ZERO callers and can't reach the private layout singleton. So a private-use codepoint shapes to tofu. → ADR: bundle an OFL icon font + a public registration API feeding BOTH FontSystems (or unify them) + call it at startup. `flui-assets` (`src/assets/font.rs`) may host the registry.

## Build (widget-layer slice, this task)
`crates/flui-widgets/src/icon/{icon_data.rs, icon_theme_data.rs, icon.rs, mod.rs}`:
- A. `IconData` value type (fields above; Debug/Clone/PartialEq/Eq/Hash; `const fn new(code_point)` + `with_font_family`; Display `U+{:05X}`; helper `char::from_u32(code_point).map(String::from)`).
- B. `IconThemeData` + `IconTheme` inherited widget (impl_inherited_view!, template media_query.rs); `IconThemeData::fallback()` = oracle constants; `IconTheme::of(ctx)` merges to fallback, `maybe_of`.
- C. `Icon` StatelessView (build per icon.dart:260-357): resolve theme → size → null branch (SizedBox::square) → TextStyle (font_family/size/color/height=1.0/font_variations) → RichText(TextSpan::styled(codepoint_string, style)).direction → `SizedBox::square(size).child(Center::new().child(rich_text))`. DEFER match_text_direction RTL flip (no Transform widget — document) + Semantics wrapper (defer/partial).
- D. `pub mod icon;` in lib.rs + re-export Icon/IconData/IconTheme/IconThemeData + prelude.

## Tests (`tests/parity/icon_test.rs`, honest)
- default size 24×24 (SizedBox forces the square, font-independent, exact) — oracle icon_theme_data.dart:52.
- explicit size overrides theme (36×36).
- codepoint reaches RenderParagraph: `find_text(codepoint_string)` == Some (e.g. U+E87D).
- null icon → 24×24 SizedBox, NO RenderParagraph.
- unit: IconData defaults/==/Display; IconThemeData::fallback() constants; Icon TextStyle construction (font_size==size, height==Some(1.0), color resolution).
- DO NOT assert a rendered glyph / non-degenerate paragraph width (dishonest with no icon font).

## Deferred: match_text_direction (needs Transform widget), Semantics wrapper, glyph rendering (font-registration ADR).
