# Changelog fragments

A change a consumer would notice adds one file here instead of editing
[`CHANGELOG.md`](/CHANGELOG.md). At release time every fragment is merged into the
`## [Unreleased]` region of `CHANGELOG.md` and removed, so two pull requests never edit the same
lines.

## Name

`changelog.d/<branch-slug>.md`: the branch name with `/` replaced by `-`, so the branch
`tools/changelog-fragments` writes `changelog.d/tools-changelog-fragments.md`. The name is
lowercase letters, digits and single dashes, then `.md`. This README is the only other file the
directory holds.

## Body

One or more sections. Each is a `### <Section>` header, where the section is one of `Added`,
`Changed`, `Deprecated`, `Removed`, `Fixed` or `Security`, each at most once, followed by one
unordered list whose bullets start with `-` (a `*` or `+` list would render as a separate list
once merged above the existing `-` bullets). Continuation lines and nested lists inside a bullet,
indented, are fine; nothing else goes in a section (no paragraph, ordered list, code block, table,
quote, HTML or deeper heading), and nothing goes above the first header.

```markdown
### Added

- **`flui-widgets`**: what a consumer can do now.

### Fixed

- **`flui-view`**: what was wrong, and what a consumer sees now
  ([ADR-0085](/docs/adr/ADR-0085-reactive-core-placement-and-phase-subscribers.md)).
```

## Links

A link is root-relative (`/docs/testing.md`), absolute (`https:`, `mailto:`), or a code span.
The link check reads a fragment here and the same text is later pasted into the root file; a
root-relative link resolves the same way in both places, a relative one does not. An
`#anchor`-only link and a reference definition (`[x]: /docs/x.md`) change target once fragments
are merged, so leave them out.

## Checks and release

CI's `checks` job checks every fragment's name and body as above and that `CHANGELOG.md` holds
one `## [Unreleased]`; lychee checks the links. At release, each section's bullets of every
fragment, in file-name order, go at the top of that section of the region (a section the region
lacks is added in the order above), the fragments are deleted, and the release commit carries
both.
