# Changelog fragments

A change a consumer would notice adds one file here instead of editing
[`CHANGELOG.md`](/CHANGELOG.md). At release time `cargo xtask changelog --write` merges every fragment
into the `## [Unreleased]` region of `CHANGELOG.md` and removes the fragments, so two pull
requests never edit the same lines.

## Name

`changelog.d/<branch-slug>.md`: the branch name with `/` replaced by `-`, so the branch
`tools/changelog-fragments` writes `changelog.d/tools-changelog-fragments.md`. The name is
lowercase letters, digits and single dashes, then `.md`. This README is the only other file the
directory holds.

## Body

One or more sections. Each is a `### <Section>` header, where the section is one of `Added`,
`Changed`, `Deprecated`, `Removed`, `Fixed` or `Security`, each at most once, followed by one
unordered list whose bullets start with `-` (a `*` or `+` list would render as a separate list
once merged above the existing `-` bullets). Continuation lines and nested lists inside a bullet are fine; nothing else goes
in a section (no paragraph, ordered list, code block, table, quote, HTML or deeper heading), and
nothing goes above the first header.

```markdown
### Added

- **`cargo xtask changelog`**: merges `changelog.d/` fragments into `CHANGELOG.md`.

### Fixed

- **`flui-view`**: what was wrong, and what a consumer sees now
  ([ADR-0085](/docs/adr/ADR-0085-reactive-core-placement-and-phase-subscribers.md)).
```

## Links

A link is root-relative (`/docs/testing.md`), absolute (`https:`, `mailto:`), or a code span.
The link check reads a fragment here and the same text is later pasted into the root file; a
root-relative link resolves the same way in both places, a relative one does not. An
`#anchor`-only link and a reference definition (`[x]: /docs/x.md`) are refused, since their
target changes once fragments are merged.

## Checks and release

`cargo xtask changelog` (or `--check`) validates every fragment and `CHANGELOG.md` and writes
nothing; it runs in `cargo xtask checks`. At release, `cargo xtask changelog --dry-run` prints the
merged region, `cargo xtask changelog --write` writes it and deletes the fragments, and the
release commit carries both. Within one release, fragments merge in file-name order, each section's new bullets above
the ones already there.
