#!/usr/bin/env python3
"""Measure how many line-number citations in the ADRs still resolve (issue #993).

WHAT THIS PROVES, AND WHAT IT DOES NOT
--------------------------------------
It answers one question only: does the cited path still exist, and does the file
still have that many lines? That is a LOWER BOUND on rot. A citation whose line
is in range may still point at something entirely different -- lines move under
edits far more often than files shrink -- so "resolves" here means "not provably
broken", never "correct".

The reverse direction is sound: an out-of-range line, or a path that no longer
exists, IS stale. Those are the ones this reports.

That asymmetry is the point. #993 asks for the number that decides whether a bulk
conversion earns its diff, and a lower bound is honest input to that decision in a
way a guess is not.

Exit status is 0 unless --strict is passed: this is a measurement, not a gate.
"""
from __future__ import annotations
import argparse, pathlib, re, sys
from collections import defaultdict

ROOT = pathlib.Path(__file__).resolve().parent.parent
# `path.rs:12`, `12-34`, `12,40-42`, and the messier real ones this repo
# actually contains: `184, 192-194`, `279+283`, `209/221`. The separator class is
# deliberately wide — a narrower one silently TRUNCATES those to their first
# number and quietly shrinks the denominator, which is the same unverified-count
# failure #993 exists to stop (it cost 11 citations on the first pass here).
CITE = re.compile(r"`([A-Za-z_][A-Za-z_/0-9.-]*\.rs):([0-9][0-9,+/ -]*?)`")
# The form a converted citation takes: `path.rs`'s `Symbol`. This MUST be
# matched too. Without it, converting a citation removes it from the corpus and
# "provably stale" falls by shrinking the denominator rather than by fixing
# anything -- measured at 77 -> 60 matchable in ADR-0039 across one such commit.
# The predicate here is the project's settled one, from
# scripts/check-runtime-conformance.sh's `check_citation`: the file exists AND
# contains the cited string. That survives a line move, and catches a rename.
SYMBOL_CITE = re.compile(r"`([A-Za-z_][A-Za-z_/0-9.-]*\.rs)`'s `([A-Za-z_][A-Za-z_0-9:!]*)`")
# A relative `:NNN` ref inherits whatever path precedes it. Nothing here can
# resolve that, so they are counted and reported as UNANCHORED rather than
# silently omitted -- there are more of them than absolute citations, and a
# conversion that deletes their anchor orphans them invisibly.
RELATIVE = re.compile(r"`:[0-9][0-9,+/ -]*`")

def rust_files() -> dict[str, list[pathlib.Path]]:
    """Every .rs file, indexed by each of its path suffixes, so a partial
    citation like `windows/platform.rs` can be resolved the way a reader would."""
    index: dict[str, list[pathlib.Path]] = defaultdict(list)
    for p in ROOT.rglob("*.rs"):
        if any(part in {"target", ".flutter", ".gpui", ".git"} for part in p.parts):
            continue
        rel = p.relative_to(ROOT)
        parts = rel.parts
        for i in range(len(parts)):
            index["/".join(parts[i:])].append(rel)
    return index

def max_line(spec: str) -> int:
    """Highest line number a citation names (`12,40-42` -> 42)."""
    return max(int(n) for n in re.findall(r"[0-9]+", spec))

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--strict", action="store_true",
                    help="exit non-zero when any citation is provably stale")
    ap.add_argument("--list", action="store_true", help="print every stale citation")
    args = ap.parse_args()

    index = rust_files()
    counts = defaultdict(int)
    stale: list[str] = []
    per_adr: dict[str, int] = defaultdict(int)

    for adr in sorted((ROOT / "docs" / "adr").glob("*.md")):
        text = adr.read_text(encoding="utf-8")
        counts["relative"] += len(RELATIVE.findall(text))
        for m in SYMBOL_CITE.finditer(text):
            path, sym = m.group(1), m.group(2)
            counts["symbol_total"] += 1
            hits = index.get(path, [])
            if len(hits) != 1:
                counts["symbol_unresolvable"] += 1
                continue
            body = (ROOT / hits[0]).read_text(encoding="utf-8", errors="replace")
            if sym.split("::")[-1].rstrip("!") in body:
                counts["symbol_ok"] += 1
            else:
                counts["symbol_broken"] += 1
                per_adr[adr.name] += 1
                stale.append(f"{adr.name}: `{path}`'s `{sym}` -- symbol not in file")
        for m in CITE.finditer(text):
            path, spec = m.group(1), m.group(2)
            counts["total"] += 1
            hits = index.get(path, [])
            if not hits:
                counts["path_gone"] += 1
                per_adr[adr.name] += 1
                stale.append(f"{adr.name}: `{path}:{spec}` -- no such file")
                continue
            if len(hits) > 1:
                counts["ambiguous"] += 1
                continue
            # A BARE filename ("runner.rs") that happens to match exactly one
            # file is not a confident resolution: the file the author meant may
            # simply be gone, leaving an unrelated same-named file as the only
            # survivor. That is not hypothetical -- ADR-0039's `runner.rs:2290`
            # means flui-app's runner, which was split into a `runner/`
            # directory; the only remaining `runner.rs` belongs to flui-cli, so
            # the line check runs against a file the record never referred to.
            # The verdict lands stale either way, but for the wrong reason, and
            # a reader deserves to know which.
            bare = "/" not in path
            n = len(( ROOT / hits[0]).read_text(encoding="utf-8", errors="replace").splitlines())
            note = " (bare name -- may not be the file meant)" if bare else ""
            if max_line(spec) > n:
                counts["out_of_range"] += 1
                counts["out_of_range_bare"] += 1 if bare else 0
                per_adr[adr.name] += 1
                stale.append(f"{adr.name}: `{path}:{spec}` -- file has {n} lines{note}")
            else:
                counts["in_range"] += 1
                counts["in_range_bare"] += 1 if bare else 0

    total = counts["total"]
    broken = counts["path_gone"] + counts["out_of_range"] + counts["symbol_broken"]
    print(f"adr-citations: {total} line-number citations across docs/adr/")
    print(f"  provably stale : {broken}"
          f"  ({counts['path_gone']} path gone, {counts['out_of_range']} line past EOF"
          f" -- {counts['out_of_range_bare']} of those from a bare filename)")
    print(f"  not disproved  : {counts['in_range']}  (in range -- NOT the same as correct;"
          f" {counts['in_range_bare']} resolved from a bare filename)")
    print(f"  unresolvable   : {counts['ambiguous']}  (path suffix matches several files)")
    if total:
        print(f"  lower-bound rot: {100 * broken / total:.1f}%")
    if per_adr:
        worst = sorted(per_adr.items(), key=lambda kv: -kv[1])[:5]
        print("  worst files    : " + ", ".join(f"{k} ({v})" for k, v in worst))
    print(f"  symbol-cited   : {counts['symbol_total']}"
          f"  ({counts['symbol_ok']} verified present, {counts['symbol_broken']} missing,"
          f" {counts['symbol_unresolvable']} path unresolvable)")
    print(f"  UNANCHORED     : {counts['relative']}  (bare `:NNN` inheriting a nearby path --"
          f" nothing here can resolve them, and deleting their anchor orphans them silently)")
    if args.list:
        for line in stale:
            print("    " + line)
    return 1 if (args.strict and broken) else 0

if __name__ == "__main__":
    sys.exit(main())
