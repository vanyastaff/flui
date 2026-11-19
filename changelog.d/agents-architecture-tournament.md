### Added

- The Claude Code workflow `architecture-tournament` (`.claude/workflows/architecture-tournament.js`)
  runs rival designs for a problem with a measurable target before a spec's `design.md` is written:
  research on the code, prior art and its recorded failures; one design per school; every author
  attacks the others and a red team attacks all; each author answers every attack by number and
  revises; a second red-team round reads the fixes; and a judge scores, synthesizes and lists the
  experiments and the technical debt found. Its branches are checked by
  `node .claude/workflows/architecture-tournament.test.mjs`.
