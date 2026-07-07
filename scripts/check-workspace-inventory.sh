#!/usr/bin/env bash
# Verify that human-facing crate inventories match Cargo's active workspace.
#
# The source of truth is `cargo metadata`: a crate is active when it is a
# workspace member under `crates/`. This check keeps the docs and justfile from
# drifting after crate splits, merges, or re-enables.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "${repo_root}"

if ! command -v cargo >/dev/null 2>&1; then
  echo "workspace-inventory: cargo not found on PATH" >&2
  exit 2
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "workspace-inventory: python3 not found on PATH" >&2
  exit 2
fi

metadata_file="$(mktemp)"
trap 'rm -f "${metadata_file}"' EXIT

cargo metadata --no-deps --format-version 1 >"${metadata_file}"

python3 - "${repo_root}" "${metadata_file}" <<'PY'
import json
import re
import sys
import tomllib
from pathlib import Path

root = Path(sys.argv[1]).resolve()
metadata_path = Path(sys.argv[2])
data = json.loads(metadata_path.read_text())
root_manifest = tomllib.loads((root / "Cargo.toml").read_text())
workspace_package = root_manifest["workspace"]["package"]

workspace_members = set(data["workspace_members"])
active_crates: list[str] = []
active_crate_manifests: dict[str, Path] = {}
workspace_packages: list[dict] = []
workspace_package_by_manifest_dir: dict[Path, dict] = {}


def publish_disabled(package: dict) -> bool:
    # `cargo metadata` reports `publish = false` manifests as an empty list.
    return package.get("publish") is False or package.get("publish") == []

for package in data["packages"]:
    if package["id"] not in workspace_members:
        continue
    workspace_packages.append(package)

    manifest = Path(package["manifest_path"]).resolve()
    workspace_package_by_manifest_dir[manifest.parent] = package
    try:
        rel = manifest.relative_to(root)
    except ValueError:
        continue

    if len(rel.parts) == 3 and rel.parts[0] == "crates" and rel.parts[2] == "Cargo.toml":
        active_crates.append(package["name"])
        active_crate_manifests[package["name"]] = manifest

active_crates = sorted(active_crates)
errors: list[str] = []

for manifest in root.rglob("Cargo.toml"):
    if "target" in manifest.relative_to(root).parts:
        continue
    if "https://github.com/anthropics/flui" in manifest.read_text():
        errors.append(f"{manifest.relative_to(root)} still references stale repository `anthropics/flui`")

docs_that_must_list_all = [
    root / "README.md",
    root / "docs" / "crates.md",
    root / "docs" / "PORT.md",
]

for path in docs_that_must_list_all:
    text = path.read_text()
    for crate in active_crates:
        if f"`{crate}`" not in text:
            errors.append(f"{path.relative_to(root)} does not list active crate `{crate}`")

current_inventory_files = [
    root / "AGENTS.md",
    root / "README.md",
    root / "docs" / "crates.md",
    root / "docs" / "PORT.md",
    root / "docs" / "architecture.md",
    root / "justfile",
]

for path in current_inventory_files:
    text = path.read_text()
    if "flui-log" in text:
        errors.append(f"{path.relative_to(root)} still references removed crate `flui-log`")

justfile = (root / "justfile").read_text()

active_match = re.search(r'^active_crates := "([^"]*)"$', justfile, re.MULTILINE)
if active_match is None:
    errors.append("justfile is missing the `active_crates := \"...\"` inventory line")
else:
    just_crates = active_match.group(1).split()
    missing = sorted(set(active_crates) - set(just_crates))
    extra = sorted(set(just_crates) - set(active_crates))
    if missing:
        errors.append("justfile active_crates missing: " + ", ".join(missing))
    if extra:
        errors.append("justfile active_crates has non-active crates: " + ", ".join(extra))

build_match = re.search(r"(?ms)^build-layered:\n(?P<body>(?:^[ \t].*\n)+)", justfile)
if build_match is None:
    errors.append("justfile is missing the `build-layered` recipe body")
else:
    built = re.findall(r"cargo build -p ([A-Za-z0-9_-]+)", build_match.group("body"))
    missing = sorted(set(active_crates) - set(built))
    extra = sorted(set(built) - set(active_crates))
    if missing:
        errors.append("build-layered missing active crates: " + ", ".join(missing))
    if extra:
        errors.append("build-layered builds non-active crates: " + ", ".join(extra))

expected_workspace_values = {
    "version": workspace_package["version"],
    "edition": workspace_package["edition"],
    "rust_version": workspace_package["rust-version"],
    "license": workspace_package["license"],
    "authors": workspace_package["authors"],
    "repository": workspace_package["repository"],
}

for package in workspace_packages:
    manifest = Path(package["manifest_path"]).resolve()
    rel = manifest.relative_to(root)
    name = package["name"]

    if not publish_disabled(package):
        for dependency in package["dependencies"]:
            dep_path = dependency.get("path")
            if dep_path is None:
                continue

            req = dependency.get("req")
            if not req or req == "*":
                errors.append(
                    f"{rel} dependency `{dependency['name']}` uses `path` without a publishable version requirement"
                )
                continue

            dependency_package = workspace_package_by_manifest_dir.get(Path(dep_path).resolve())
            if dependency_package is None:
                continue

            dependency_version = dependency_package["version"]
            if req not in {dependency_version, f"^{dependency_version}"}:
                errors.append(
                    f"{rel} dependency `{dependency['name']}` uses version requirement {req!r}, "
                    f"expected {dependency_version!r} or '^{dependency_version}'"
                )

    if name == "flui":
        for field in ("authors", "repository", "description", "license"):
            if not package.get(field):
                errors.append(f"root package is missing resolved Cargo metadata field `{field}`")
        if package.get("readme") != "README.md":
            errors.append("root package must declare `readme = \"README.md\"`")

    if rel.parts[0] in {"examples", "tools"}:
        raw_package = tomllib.loads(manifest.read_text())["package"]
        if raw_package.get("publish") is not False:
            errors.append(f"{rel} must set `publish = false`")
        for metadata_key in ("version", "edition", "rust_version", "license"):
            expected = expected_workspace_values[metadata_key]
            if package.get(metadata_key) != expected:
                cargo_key = metadata_key.replace("_", "-")
                errors.append(
                    f"{rel} resolves `{cargo_key}` to {package.get(metadata_key)!r}, expected {expected!r}"
                )
        for key in ("version", "edition", "rust-version", "license"):
            if raw_package.get(key) != {"workspace": True}:
                errors.append(f"{rel} must inherit `{key}.workspace = true`")

    if name not in active_crate_manifests:
        continue

    for metadata_key, expected in expected_workspace_values.items():
        if package.get(metadata_key) != expected:
            cargo_key = metadata_key.replace("_", "-")
            errors.append(
                f"{rel} resolves `{cargo_key}` to {package.get(metadata_key)!r}, expected {expected!r}"
            )

    if not package.get("description"):
        errors.append(f"{rel} is missing package description")

    raw_manifest = tomllib.loads(manifest.read_text())
    raw_package = raw_manifest["package"]
    for key in ("version", "edition", "rust-version", "license", "authors", "repository"):
        if raw_package.get(key) != {"workspace": True}:
            errors.append(f"{rel} must inherit `{key}.workspace = true`")

    # Workspace lints must apply to every active crate. A missing `[lints]`
    # table silently opts the crate out of `[workspace.lints]`; a local table
    # shadows it with a stale copy (and Cargo forbids mixing `workspace = true`
    # with local keys, so equality is the only valid shape).
    if raw_manifest.get("lints") != {"workspace": True}:
        errors.append(f"{rel} must set `[lints] workspace = true` (local or missing lint tables bypass workspace lints)")

if errors:
    print("workspace-inventory: drift detected", file=sys.stderr)
    for error in errors:
        print(f"  - {error}", file=sys.stderr)
    sys.exit(1)

print(f"workspace-inventory: {len(active_crates)} active crates covered")
PY
