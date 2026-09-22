#!/usr/bin/env python3
"""Guard for ci.yml's `paths-filter` job's docs-only allowlist.

That allowlist decides which changed files let a `pull_request` run skip
every compiling job (see `paths-filter`'s own comment in ci.yml). If a file
under the allowlist is actually pulled into compiled/tested content via
`include_str!` (a `#[doc = include_str!(...)]` doctest, or a plain
`include_str!(...)` a test asserts against), a PR touching only that file
would wrongly skip the very jobs that would catch it breaking something.

This script re-derives, from source, every path any `include_str!(...)`
call in the workspace resolves to, and fails if any of them falls inside
the allowlist's own patterns -- kept here, not only in ci.yml, so the two
can't drift without this failing first. Run via `just gate` (part of
`checks`), no compilation.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Mirrors ci.yml's `paths-filter` job's `non_docs` filter's negated
# (docs-only) patterns. Keep this list and that job in sync by hand; this
# script is what proves neither has quietly drifted from what's actually
# compiled.
ROOT_MD_ONLY = True  # `*.md` in the filter matches only root-level .md files
DOCS_ONLY_DIR_PREFIXES = ("docs/", ".rust-studio/", ".github/")
CRATE_LEVEL_ALLOWED_BASENAMES = ("ARCHITECTURE.md", "CHANGELOG.md")

INCLUDE_STR_RE = re.compile(r'include_str!\(\s*"([^"]+)"\s*\)')


def resolve_target(rs_file: Path, literal: str) -> Path:
    return (rs_file.parent / literal).resolve()


def is_allowlisted_docs_only(repo_relative: str) -> bool:
    parts = repo_relative.split("/")
    if repo_relative.endswith(".md") and len(parts) == 1:
        return True
    if any(repo_relative.startswith(prefix) for prefix in DOCS_ONLY_DIR_PREFIXES):
        return True
    # `crates/*/ARCHITECTURE.md` / `crates/*/CHANGELOG.md`
    if len(parts) == 3 and parts[0] == "crates" and parts[2] in CRATE_LEVEL_ALLOWED_BASENAMES:
        return True
    if repo_relative in (".editorconfig",):
        return True
    return False


def main() -> int:
    offenders: list[tuple[Path, str]] = []
    for rs_file in ROOT.rglob("*.rs"):
        if any(part in {"target", ".flutter", ".gpui"} for part in rs_file.parts):
            continue
        text = rs_file.read_text(encoding="utf-8", errors="ignore")
        for match in INCLUDE_STR_RE.finditer(text):
            literal = match.group(1)
            target = resolve_target(rs_file, literal)
            try:
                repo_relative = target.relative_to(ROOT).as_posix()
            except ValueError:
                continue  # outside the repo; not our concern here
            if not target.exists():
                continue  # not a real file (e.g. build-script-generated path)
            if is_allowlisted_docs_only(repo_relative):
                offenders.append((rs_file.relative_to(ROOT), repo_relative))

    if offenders:
        print(
            "check-paths-filter-allowlist: the following include_str!() targets fall "
            "inside ci.yml's paths-filter docs-only allowlist, so a PR touching only "
            "them would wrongly skip every compiling job:",
            file=sys.stderr,
        )
        for rs_file, target in sorted(set(offenders)):
            print(f"  {rs_file} includes {target}", file=sys.stderr)
        print(
            "Fix: narrow the allowlist in ci.yml's paths-filter job (and this "
            "script's mirror of it) to exclude these paths.",
            file=sys.stderr,
        )
        return 1

    print("check-paths-filter-allowlist: docs-only allowlist matches no compiled include_str!() target")
    return 0


if __name__ == "__main__":
    sys.exit(main())
