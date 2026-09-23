"""Turn change_scope's classification into the cargo arguments the fast lane
runs -- the one place the test-scope policy lives, for CI's `plan` job and
`just check-changed` alike (both reach it through scripts/affected-crates.sh):

- tests exclude flui-platform (its suite needs a display server: a separate
  headless leg runs it when it is in scope);
- the facade's non-default catalogs join the run when `flui` is in scope
  (`--features flui/cupertino,flui/localizations`);
- cfg-gated code the Linux lane would never compile gets a check on its own
  target: flui-platform's four backends, the flui-app/flui mobile runner, the
  flui-cli Windows paths (mirroring the cross-typecheck job), and wasm32 for
  the wasm-capable packages in scope (mirroring wasm-check; the excluded set
  is read from ci.yml's NO_WASM_PKGS, not restated here);
- a crate whose Cargo.toml changed gets the per-feature clippy pass
  (feature-matrix's `cargo hack --each-feature`), for that crate only;
- rustdoc -D warnings runs over the scope with its packages' `testing`
  features (doc-strict.sh's flags, narrowed): a moved item's broken intra-doc
  link otherwise merges green and fails main's `doc` job.

Python >= 3.9 on purpose (no tomllib).
"""
from __future__ import annotations

import re
import shlex
import sys

import change_scope as cs


def testing_packages() -> set:
    """Workspace packages with a `testing` feature: doc-strict.sh turns every
    one on, since code behind it is documented too."""
    import json
    import subprocess
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--offline"],
        cwd=cs.ROOT, check=True, capture_output=True, text=True).stdout)
    return {p["name"] for p in meta["packages"] if "testing" in p["features"]}


def no_wasm_packages() -> set:
    text = cs.CI_YML.read_text(encoding="utf-8")
    m = re.search(r"NO_WASM_PKGS: >-\n((?:[ \t]+--exclude [A-Za-z0-9_-]+\n)+)", text)
    return set(re.findall(r"--exclude ([A-Za-z0-9_-]+)", m.group(1))) if m else set()


def args_for(result: dict, members=None) -> dict:
    mode, packages = result["mode"], result["packages"]
    full = mode == "full"
    scope = set(packages)
    p = lambda names: " ".join(f"-p {n}" for n in sorted(names))  # noqa: E731
    no_wasm = no_wasm_packages()
    if full:
        wasm_args = "--workspace " + " ".join(f"--exclude {n}" for n in sorted(no_wasm))
    else:
        wasm_args = p(scope - no_wasm)
    # rustdoc -D warnings over the scope: doc-strict.sh's flags, narrowed.
    # `--features x/testing` is only valid for a selected package.
    doc_args = ""
    if mode in ("packages", "full"):
        testing = sorted(testing_packages() if full else testing_packages() & scope)
        feats = f" --features {','.join(n + '/testing' for n in testing)}" if testing else ""
        doc_args = ("--workspace" if full else p(scope)) + feats
    return {
        "mode": mode,
        "heavy_required": "true" if result["heavy_required"] else "false",
        "reason": result["reason"],
        "packages": " ".join(packages),
        "pkg_args": "--workspace" if full else p(scope),
        "test_args": "--workspace --exclude flui-platform" if full else p(scope - {"flui-platform"}),
        "features": "--features flui/cupertino,flui/localizations" if full or "flui" in scope else "",
        "platform": "true" if full or "flui-platform" in scope else "false",
        "cross_platform": "true" if full or "flui-platform" in scope else "false",
        "cross_app": "true" if full or scope & {"flui-app", "flui"} else "false",
        "cross_cli": "true" if full or "flui-cli" in scope else "false",
        "wasm_args": wasm_args if mode in ("packages", "full") else "",
        "wasm_facade": "true" if full or "flui" in scope else "false",
        "hack_args": p(result["manifests"]) if mode == "packages" else "",
        "doc_args": doc_args,
    }


def main(argv: list) -> int:
    import argparse
    ap = argparse.ArgumentParser(description="affected packages and the fast lane's cargo arguments")
    ap.add_argument("--base", default="origin/main")
    ap.add_argument("--worktree", action="store_true", help="include uncommitted and untracked files")
    ap.add_argument("--format", choices=("human", "shell", "github"), default="human")
    ap.add_argument("--files", nargs="*", help="classify these paths instead of a git diff")
    ap.add_argument("--full", action="store_true", help="the whole workspace, no diff (CI's heavy lane)")
    args = ap.parse_args(argv)
    if args.full:
        result = {"mode": "full", "packages": [], "manifests": [], "heavy_required": False,
                  "reason": "heavy lane: the whole workspace"}
    else:
        files = args.files if args.files is not None else cs.changed_files(args.base, args.worktree)
        result = cs.classify(files)
    values = args_for(result)
    if args.format == "github":
        for k, v in values.items():  # GITHUB_OUTPUT: one line per key; values carry no newlines
            print(f"{k}={v.replace(chr(10), ' ')}")
    elif args.format == "shell":
        for k, v in values.items():  # quoted: `reason` quotes file names, which are untrusted
            print(f"{k.upper()}={shlex.quote(v)}")
    else:
        print(f"mode: {values['mode']}{'  [heavy lane required]' if values['heavy_required'] == 'true' else ''}  ({values['reason']})")
        if values["packages"]:
            print(f"packages ({len(result['packages'])}): {values['packages']}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
