#!/usr/bin/env python3
"""Guard for the docs-only allowlist (scripts/lib/change_scope.py DOCS_ONLY).

That allowlist decides which changed files let a `pull_request` run skip
every compiling job (see `plan`'s comment in ci.yml). If a file
under the allowlist is actually pulled into compiled/tested content via
`include_str!` (a `#[doc = include_str!(...)]` doctest, or a plain
`include_str!(...)` a test asserts against), a PR touching only that file
would wrongly skip the very jobs that would catch it breaking something.

This script re-derives, from source, every path any `include_str!(...)`
call in the workspace resolves to, and fails if any of them falls inside
the allowlist's own patterns -- defined once in scripts/lib/change_scope.py, so the two
can't drift without this failing first. Run via `just gate` (part of
`checks`), no compilation.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The docs-only allowlist itself lives in scripts/lib/change_scope.py
# (DOCS_ONLY), the one classification CI's `plan` job and
# `just check-changed` use; this script proves no compiled include_str!()
# target falls inside it.
sys.path.insert(0, str(ROOT / "scripts" / "lib"))
from change_scope import is_docs_only as is_allowlisted_docs_only  # noqa: E402

INCLUDE_STR_RE = re.compile(r'include_str!\(\s*"([^"]+)"\s*\)')


def resolve_target(rs_file: Path, literal: str) -> Path:
    return (rs_file.parent / literal).resolve()


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
            "inside the docs-only allowlist (scripts/lib/change_scope.py DOCS_ONLY), so a "
            "PR touching only them would wrongly skip every compiling job:",
            file=sys.stderr,
        )
        for rs_file, target in sorted(set(offenders)):
            print(f"  {rs_file} includes {target}", file=sys.stderr)
        print(
            "Fix: narrow DOCS_ONLY in scripts/lib/change_scope.py to exclude these paths.",
            file=sys.stderr,
        )
        return 1

    print("check-paths-filter-allowlist: docs-only allowlist matches no compiled include_str!() target")
    return 0


if __name__ == "__main__":
    sys.exit(main())
