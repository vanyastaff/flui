# ADR-0270: The emoji generic starts with the platform's colour emoji face

- **Status:** Accepted (2026-10-30)
- **Date:** 2026-10-30
- **Supersedes:** [ADR-0092](ADR-0092-per-realm-text-over-parley.md) §7, its clause "The common
  list is the emoji generic" only: the emoji generic is now the platform's colour emoji faces, then
  the common list.
- **Related:** [ADR-0092](ADR-0092-per-realm-text-over-parley.md) §7 (one fallback order over
  FLUI's per-platform lists)

## Context and problem statement

For an emoji cluster, Parley tries the style's family and then the collection's emoji generic.
`flui-painting`'s `install_into` set that generic to the platform's common list
(`fallback_tables::common`), which was ported from cosmic-text 0.19. Each common list puts text
faces before the emoji face:

| Platform | Common list, in order (abridged) | First face that maps U+1F600 |
|----------|----------------------------------|------------------------------|
| Linux and the BSDs | Noto Sans, DejaVu Sans, …, Noto Color Emoji | DejaVu Sans (monochrome outlines) |
| Windows | Segoe UI, Segoe UI Emoji, … | Segoe UI Emoji |
| macOS | .SF NS, Menlo, Apple Color Emoji, … | Apple Color Emoji |

On a Linux host with both DejaVu Sans and Noto Color Emoji installed, `😀` in a style that names
no emoji family shaped in DejaVu Sans and painted black. The engine's readback row
`colour_emoji_on_line_two` showed it once CI ran the readback suites on a real adapter. Order
within the common list also decides ordinary script fallback, and a test pins that list to the
recorded cosmic-text lists. Reordering the common list is therefore not free. Which faces should
the emoji generic name, and in what order?

## Decision drivers

- An emoji renders in colour wherever the host has a colour emoji face, whatever the style's family.
- Script fallback for text stays as it is: the common lists keep their recorded order
  (`platform_tables_match_the_recorded_lists`).
- One fallback order: the lists are FLUI's data (`fallback_tables`), not a scan of every face.

## Considered options

1. **Keep the common list as the emoji generic.**
   - Good: no change.
   - Bad: on Linux emoji paint monochrome whenever DejaVu Sans is installed, which is the common case.
2. **Move the emoji face to the front of each common list.**
   - Good: one list per platform.
   - Bad: text fallback would then try the emoji face first for every script. The face maps digits,
     `#`, `*` and some symbols, so those would shape in the emoji face, the defect `font_resolve`
     exists to prevent (issue #927).
3. **A per-platform list of colour emoji faces, tried before the common list in the emoji generic only.**
   - Good: emoji reach the colour face first, and text fallback is unchanged.
   - Bad: one more table to keep in step with the platforms' face names.
4. **Pick the emoji face by scanning for a face with colour tables.**
   - Good: no names to keep.
   - Bad: it is a walk over every face, which ADR-0092 §7 rejects, and the order of the scan is the
     host's, not FLUI's.

## Decision

Chosen option: 3, because it fixes emoji without changing a single text fallback.

`fallback_tables::emoji(Platform) -> &'static [&'static str]` names each platform's colour emoji
faces: `Segoe UI Emoji` on Windows, `Apple Color Emoji` on macOS, and `Noto Color Emoji` then
`Apple Color Emoji` on the unix table, which iOS shares. Android and wasm name none.
`FallbackChain` carries the list, and `install_into` sets the emoji generic to:

1. the faces of that list the collection holds;
2. then the common list;
3. each family once, in order.

A platform whose two lists are both empty keeps the collection's own emoji generic, as before.

### Consequences

- Good, because a style that names no emoji family paints emoji in colour on a Linux host with
  DejaVu Sans installed.
- Good, because script fallback and the recorded common lists are unchanged.
- Bad, because a platform that renames its emoji face needs a table edit.
- Neutral, because a style that names a family which itself maps emoji still shapes them in that
  family: Parley tries the style's family before the generic.

### Confirmation

- `the_emoji_generic_starts_with_the_colour_faces` (`flui-painting`'s `fallback_chain`): the
  generic is the held colour faces, then the common list, each once; with no colour face held, the
  common list alone.
- `each_platform_names_its_colour_emoji_face_first` (same module) pins every platform's table.
- `colour_emoji_on_line_two`, a row of `parley_runs_read_back` (`flui-engine`): on a host with a
  colour emoji face, the default style shapes `😀` in a face with colour tables and paints it in
  colour. CI runs it on lavapipe and llvmpipe.
