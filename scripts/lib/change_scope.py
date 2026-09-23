"""Which workspace packages a change touches -- one answer for CI and local.

Used by scripts/affected-crates.sh (CI's `plan` job, `just check-changed`)
and by scripts/check-paths-filter-allowlist.py (the docs-only allowlist
guard), so the classification below exists exactly once. It only classifies;
turning the result into cargo arguments is scripts/lib/cargo_args.py.

  docs      every changed file is documentation (DOCS_ONLY): nothing compiles
  none      only repo tooling the `checks` job runs itself: nothing compiles
  packages  the changed packages plus every workspace package declaring a
            dependency on them (normal, dev, build, optional, target-specific;
            transitively)
  full      something every package depends on changed, or a file nobody
            here can attribute: the whole workspace

`heavy_required` is set when a changed file is an input the heavy lane
exercises (HEAVY_TRIGGERS, or a script only a heavy job runs): the PR then
runs the heavy lane, not just the whole workspace in the fast one.

Python >= 3.9 on purpose (no tomllib): it must run on a stock macOS python3.
"""
from __future__ import annotations

import fnmatch
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
CI_YML = ROOT / ".github" / "workflows" / "ci.yml"

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

# Workspace-wide inputs whose breakage shows up in the heavy jobs (msrv,
# feature-matrix, wasm, cross, gpu, doc, miri): the heavy lane runs.
HEAVY_TRIGGERS = (
    "Cargo.toml",  # the root manifest: workspace deps, lints, profiles
    "Cargo.lock",
    ".cargo/**",
    "rust-toolchain.toml",
    ".github/workflows/**",
)

# Everything depends on these, and the fast lane checks what they change.
FULL_TRIGGERS = (
    "clippy.toml",
    ".config/nextest.toml",
    # The lane machinery itself: a PR can change its own classification, so it
    # gets the whole workspace rather than the scope it would compute.
    "scripts/lib/change_scope.py",
    "scripts/lib/cargo_args.py",
    "scripts/lib/interpreters.sh",
    "scripts/affected-crates.sh",
)

# Repository tooling no package compiles and the `checks` job runs itself.
# Scripts only a heavy job runs are NOT tooling: heavy_job_inputs() reads
# them out of ci.yml and they require the heavy lane.
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


def heavy_job_inputs() -> set:
    """Repo files the heavy jobs run but `checks` does not, read from ci.yml:
    every `scripts/...` path named in a job gated on the heavy lane, plus the
    justfile when such a job runs `just`."""
    try:
        text = CI_YML.read_text(encoding="utf-8")
    except OSError:
        return set()
    jobs = re.split(r"(?m)^  (?=[A-Za-z0-9_-]+:\s*$)", text.split("\njobs:\n", 1)[-1])
    found = set()
    for job in jobs:
        if "if: needs.plan.outputs.heavy == 'true'" not in job:
            continue
        found |= set(re.findall(r"scripts/[A-Za-z0-9_./-]+[A-Za-z0-9_]", job))
        if re.search(r"(?m)^\s+(run: )?just ", job):
            found.add("justfile")
    return found


def _git(*args: str, root: Path = ROOT) -> str:
    return subprocess.run(["git", *args], cwd=root, check=True, capture_output=True, text=True).stdout


def changed_files(base: str, worktree: bool, root: Path = ROOT) -> list:
    # --no-renames: a move reports BOTH the old and the new path, so the crate a
    # file left is in scope too, not only the crate it arrived in.
    merge_base = _git("merge-base", base, "HEAD", root=root).strip()
    files = set(_git("diff", "--name-only", "--no-renames", f"{merge_base}..HEAD", root=root).split())
    if worktree:  # also staged, unstaged and untracked work, for `just check-changed`
        files |= set(_git("diff", "--name-only", "--no-renames", "HEAD", root=root).split())
        files |= set(_git("ls-files", "--others", "--exclude-standard", root=root).split())
    return sorted(files)


def workspace_graph():
    """(package name -> owned path prefixes, name -> set of workspace dependents).

    Edges come from the DECLARED dependencies (`packages[].dependencies` of
    `cargo metadata --no-deps`): optional and target-specific edges included,
    which the resolved graph omits when their feature/target is not active.
    """
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps", "--offline"],
            cwd=ROOT, check=True, capture_output=True, text=True,
        ).stdout
    )
    names = {p["name"] for p in meta["packages"]}
    owned: dict = {}
    dependents: dict = {n: set() for n in names}
    for p in meta["packages"]:
        pkg_dir = os.path.relpath(os.path.dirname(p["manifest_path"]), ROOT)
        if pkg_dir != ".":
            owned[p["name"]] = [pkg_dir + "/"]
        else:  # the root package owns its targets' directories, not the repo
            prefixes = {"src/", "tests/", "benches/", "build.rs", "Cargo.toml"}
            for t in p["targets"]:
                rel = os.path.relpath(t["src_path"], ROOT)
                d = os.path.dirname(rel)
                prefixes.add(rel if d in ("", "examples", "src", "tests", "benches") else d + "/")
            owned[p["name"]] = sorted(prefixes)
        for d in p["dependencies"]:
            if d["name"] in names and d["name"] != p["name"] and d.get("path"):
                dependents[d["name"]].add(p["name"])
    return owned, dependents


def owning_package(path: str, owned: dict):
    best, best_len = None, -1
    for name, prefixes in owned.items():
        for pre in prefixes:
            hit = path == pre or (pre.endswith("/") and path.startswith(pre))
            if hit and len(pre) > best_len:
                best, best_len = name, len(pre)
    return best


def classify(files: list):
    """dict: mode, packages (sorted), manifests (packages whose Cargo.toml
    changed), heavy_required (bool), reason."""
    code = [f for f in files if not is_docs_only(f)]
    result = {"mode": "docs", "packages": [], "manifests": [], "heavy_required": False,
              "reason": "only documentation changed" if files else "no changes"}
    if not files or not code:
        return result
    heavy = [f for f in code if _any(f, HEAVY_TRIGGERS)]
    heavy_scripts = heavy_job_inputs()
    heavy += [f for f in code if f in heavy_scripts]
    if heavy:
        result.update(mode="full", heavy_required=True,
                      reason=f"input of the heavy jobs changed: {', '.join(sorted(set(heavy))[:5])}")
        return result
    full = [f for f in code if _any(f, FULL_TRIGGERS)]
    if full:
        result.update(mode="full", reason=f"workspace-wide input changed: {', '.join(full[:5])}")
        return result
    owned, dependents = workspace_graph()
    seeds, manifests, unknown = set(), set(), []
    for f in code:
        if _any(f, TOOLING):
            continue
        pkg = owning_package(f, owned)
        if pkg:
            seeds.add(pkg)
            if f.endswith("/Cargo.toml") or f == "Cargo.toml":
                manifests.add(pkg)
        else:
            unknown.append(f)
    if unknown:
        result.update(mode="full", reason=f"no package owns: {', '.join(unknown[:5])} (conservatively: everything)")
        return result
    if not seeds:
        result.update(mode="none", reason="only repository tooling the checks job runs changed")
        return result
    scope, todo = set(seeds), list(seeds)
    while todo:
        for user in dependents.get(todo.pop(), ()):
            if user not in scope:
                scope.add(user)
                todo.append(user)
    result.update(mode="packages", packages=sorted(scope), manifests=sorted(manifests),
                  reason=f"changed: {', '.join(sorted(seeds))}; plus {len(scope) - len(seeds)} dependents")
    return result
