"""Which workspace packages a change touches -- one answer for CI and local.

Used by `scripts/affected-crates.sh` (CI's `plan` job, `just check-changed`)
and by `scripts/check-paths-filter-allowlist.py` (the docs-only allowlist
guard), so the classification below exists exactly once.

A change is classified into one of four modes:

  docs      every changed file is documentation (DOCS_ONLY): nothing compiles
  none      no package source changed (tooling, scripts, config the `checks`
            job already covers): nothing compiles
  packages  the changed packages plus every workspace package that depends on
            them (normal, dev or build edge, transitively): compile and test
            exactly that set
  full      something every package depends on changed (FULL_TRIGGERS), or a
            file nobody here knows how to attribute: the whole workspace

Python >= 3.9 on purpose (no tomllib): it must run on a stock macOS python3.
"""
from __future__ import annotations

import fnmatch
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

# Documentation: a change made only of these compiles nothing. Crate README.md
# files are deliberately NOT here -- several are pulled into doctests with
# `#[doc = include_str!(...)]` (scripts/check-paths-filter-allowlist.py proves
# no include_str! target falls inside these patterns).
DOCS_ONLY = (
    "*.md",  # root-level only: `*` does not cross `/` (see _match)
    "docs/**",
    "book/**",
    ".rust-studio/**",
    ".github/**/*.md",
    ".github/ISSUE_TEMPLATE/**",
    ".github/CODEOWNERS",
    ".editorconfig",
    "crates/*/ARCHITECTURE.md",
    "crates/*/CHANGELOG.md",
)

# Every package depends on these: the whole workspace is in scope.
FULL_TRIGGERS = (
    "Cargo.toml",  # the root manifest: workspace deps, lints, profiles
    "Cargo.lock",
    ".cargo/**",
    "rust-toolchain.toml",
    "clippy.toml",
    ".config/nextest.toml",
    ".github/workflows/**",
)

# Repository tooling that no package compiles: covered by the `checks` job
# (and deny.toml by `deny`), so it adds no package to the scope.
TOOLING = (
    "justfile",
    "scripts/**",
    "typos.toml",
    ".taplo.toml",
    "rustfmt.toml",
    "deny.toml",
    ".gitignore",
    ".gitattributes",
    ".githooks/**",
    "llms.txt",
    "LICENSE*",
    "NOTICE*",
    ".github/dependabot.yml",
)


def _match(path: str, pattern: str) -> bool:
    """Glob match where `*` stays inside one path segment and `**` spans any."""
    if "/" not in pattern and "**" not in pattern:
        return "/" not in path and fnmatch.fnmatchcase(path, pattern)
    if pattern.endswith("/**"):
        return path.startswith(pattern[:-2])
    if "**" in pattern:
        head, tail = pattern.split("**", 1)
        return path.startswith(head) and fnmatch.fnmatchcase(path.rsplit("/", 1)[-1], tail.lstrip("/"))
    return len(path.split("/")) == len(pattern.split("/")) and fnmatch.fnmatchcase(path, pattern)


def is_docs_only(path: str) -> bool:
    return any(_match(path, p) for p in DOCS_ONLY)


def _any(path: str, patterns) -> bool:
    return any(_match(path, p) for p in patterns)


def _git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def changed_files(base: str, worktree: bool) -> list[str]:
    merge_base = _git("merge-base", base, "HEAD").strip()
    files = set(_git("diff", "--name-only", f"{merge_base}..HEAD").split())
    if worktree:  # also staged, unstaged and untracked work, for `just check-changed`
        files |= set(_git("diff", "--name-only", "HEAD").split())
        files |= set(_git("ls-files", "--others", "--exclude-standard").split())
    return sorted(files)


def workspace_graph():
    """(package name -> manifest dir relative to ROOT, name -> set of workspace dependents)."""
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked"],
            cwd=ROOT, check=True, capture_output=True, text=True,
        ).stdout
    )
    members = set(meta["workspace_members"])
    by_id = {p["id"]: p for p in meta["packages"] if p["id"] in members}
    dirs = {p["name"]: os.path.relpath(os.path.dirname(p["manifest_path"]), ROOT) for p in by_id.values()}
    dependents: dict[str, set[str]] = {p["name"]: set() for p in by_id.values()}
    for node in meta["resolve"]["nodes"]:
        if node["id"] not in by_id:
            continue
        user = by_id[node["id"]]["name"]
        for dep in node["deps"]:
            if dep["pkg"] in by_id:  # normal, dev and build edges all count
                dependents[by_id[dep["pkg"]]["name"]].add(user)
    return dirs, dependents


def owning_package(path: str, dirs: dict[str, str]) -> str | None:
    best = None
    for name, d in dirs.items():
        if d == ".":
            continue  # the root package is handled below: it does not own the repo
        if path == d or path.startswith(d + "/"):
            if best is None or len(d) > len(dirs[best]):
                best = name
    if best:
        return best
    root = [n for n, d in dirs.items() if d == "."]
    if root and (path.startswith(("src/", "tests/", "benches/")) or path == "build.rs"
                 or (path.startswith("examples/") and path.count("/") == 1)):
        return root[0]
    return None


def classify(files: list[str]):
    """(mode, sorted package list, human-readable reason)."""
    code = [f for f in files if not is_docs_only(f)]
    if not files or not code:
        return "docs", [], "only documentation changed" if files else "no changes"
    full = [f for f in code if _any(f, FULL_TRIGGERS)]
    if full:
        return "full", [], f"workspace-wide input changed: {', '.join(full[:5])}"
    dirs, dependents = workspace_graph()
    seeds, unknown = set(), []
    for f in code:
        if _any(f, TOOLING):
            continue
        pkg = owning_package(f, dirs)
        if pkg:
            seeds.add(pkg)
        else:
            unknown.append(f)
    if unknown:
        return "full", [], f"no package owns: {', '.join(unknown[:5])} (conservatively: everything)"
    if not seeds:
        return "none", [], "only repository tooling changed (the checks job covers it)"
    scope, todo = set(seeds), list(seeds)
    while todo:
        for user in dependents.get(todo.pop(), ()):
            if user not in scope:
                scope.add(user)
                todo.append(user)
    return "packages", sorted(scope), f"changed: {', '.join(sorted(seeds))}; plus {len(scope) - len(seeds)} dependents"


def main(argv: list[str]) -> int:
    import argparse
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--base", default="origin/main")
    ap.add_argument("--worktree", action="store_true", help="include uncommitted and untracked files")
    ap.add_argument("--format", choices=("human", "shell", "github"), default="human")
    ap.add_argument("--files", nargs="*", help="classify these paths instead of a git diff")
    ap.add_argument("--full", action="store_true", help="the whole workspace, no diff (CI's heavy lane)")
    args = ap.parse_args(argv)
    if args.full:
        files, (mode, packages, reason) = [], ("full", [], "heavy lane: the whole workspace")
    else:
        files = args.files if args.files is not None else changed_files(args.base, args.worktree)
        mode, packages, reason = classify(files)
    # What a consumer passes to cargo: shared here so CI and just cannot differ.
    tested = [p for p in packages if p != "flui-platform"]
    pkg_args = "--workspace" if mode == "full" else " ".join(f"-p {p}" for p in packages)
    test_args = "--workspace --exclude flui-platform" if mode == "full" else " ".join(f"-p {p}" for p in tested)
    features = "--features flui/cupertino,flui/localizations" if mode == "full" or "flui" in packages else ""
    platform = "true" if mode == "full" or "flui-platform" in packages else "false"
    values = {
        "mode": mode, "packages": " ".join(packages), "reason": reason, "files": str(len(files)),
        "pkg_args": pkg_args, "test_args": test_args, "features": features, "platform": platform,
    }
    if args.format == "github":
        for k, v in values.items():
            print(f"{k}={v}")
    elif args.format == "shell":
        for k, v in values.items():
            print(f"{k.upper()}='{v}'")
    else:
        print(f"mode: {mode}  ({reason})")
        if packages:
            print(f"packages ({len(packages)}): {' '.join(packages)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
