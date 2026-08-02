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
    if "flui-log" in text and "flui-log" not in active_crates:
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

    # `autotests = false` disables Cargo's default `tests/*.rs` auto-discovery
    # (commit 820a083c consolidated per-crate integration tests into one
    # linked binary for build-time/disk reasons). The cost: a test file added
    # to `tests/` afterward silently never runs unless something declares it
    # — this bit the workspace for 11+ days across 5 crates before anyone
    # noticed. Verify every top-level `tests/*.rs` file (excluding `main.rs`
    # itself, and subdirectory module files like `tests/common/mod.rs` or
    # `tests/parity/*`, which auto-discovery never touches either way) is
    # reachable: either declared as its own `[[test]]` target, or `mod`/
    # `#[path]`-mounted from a declared `[[test]]` target file. Usually that
    # mounting file is `tests/main.rs`, but it does not have to be —
    # `flui-objects` has no `tests/main.rs` and mounts a sibling file
    # (`harness_snapshot.rs`) straight from its sole `[[test]]` target.
    if raw_package.get("autotests") is False:
        tests_dir = manifest.parent / "tests"
        if tests_dir.is_dir():
            test_targets = raw_manifest.get("test", [])
            declared_test_paths = {
                (manifest.parent / target["path"]).resolve()
                for target in test_targets
                if isinstance(target, dict) and target.get("path")
            }

            candidate_test_files = {
                test_file.resolve()
                for test_file in tests_dir.glob("*.rs")
                if test_file.name != "main.rs"
            }

            # Only DECLARED `[[test]]` targets may act as mount sources. An
            # undeclared `tests/main.rs` is not compiled at all under
            # `autotests = false`, so trusting its `mod` declarations would let
            # this guard report every sibling as reachable while none of them
            # run — the exact failure this check exists to catch, reproduced
            # inside the check. Flag such a `main.rs` instead of reading it.
            mount_source_files = set(declared_test_paths)
            main_rs = tests_dir / "main.rs"
            if main_rs.exists() and main_rs.resolve() not in declared_test_paths:
                errors.append(
                    f"{main_rs.relative_to(root)} exists but is not a declared `[[test]]` target: "
                    f"`{name}` sets `autotests = false`, so Cargo never compiles it and the "
                    "modules it declares do not run. Declare it in Cargo.toml, or delete it"
                )

            # `#[path = "foo.rs"]\nmod foo;` (the mount idiom every consolidated
            # `tests/main.rs` in this workspace uses) — the explicit path is the
            # ground truth for what file a `mod` statement resolves to.
            path_mod_re = re.compile(
                r'#\[path\s*=\s*"(?P<path>[^"]+)"\]\s*(?:pub\s+)?mod\s+(?P<name>\w+)\s*;'
            )
            # A bare `mod foo;` (no `#[path]`) resolves via Rust's default module
            # resolution: relative to the *mounting file's own* directory. That
            # default only lands back in `tests_dir` when the mounting file is
            # `tests/main.rs` itself (a target file living in a subdirectory,
            # like `flui-objects`' `render_object_harness.rs`, would resolve a
            # bare `mod foo;` into a nonexistent subdirectory next to itself).
            any_mod_re = re.compile(r"(?:^|\n)\s*(?:pub\s+)?mod\s+(?P<name>\w+)\s*;")

            reachable_test_files: set[Path] = set()
            for mount_source in mount_source_files:
                if not mount_source.is_file():
                    continue
                mount_source_text = mount_source.read_text()
                mount_source_dir = mount_source.parent
                explicitly_pathed_names: set[str] = set()
                for match in path_mod_re.finditer(mount_source_text):
                    explicitly_pathed_names.add(match.group("name"))
                    reachable_test_files.add((mount_source_dir / match.group("path")).resolve())
                if mount_source_dir == tests_dir:
                    for match in any_mod_re.finditer(mount_source_text):
                        mod_name = match.group("name")
                        if mod_name in explicitly_pathed_names:
                            continue
                        reachable_test_files.add((tests_dir / f"{mod_name}.rs").resolve())

            orphaned_test_files = candidate_test_files - declared_test_paths - reachable_test_files
            for orphan in sorted(orphaned_test_files):
                errors.append(
                    f"{orphan.relative_to(root)} is not reachable: `{name}` sets `autotests = false`, "
                    "so this file needs its own `[[test]]` target in Cargo.toml, or a `mod`/`#[path]` "
                    "mount in `tests/main.rs` (or whichever `[[test]]` target file mounts sibling test modules)"
                )

# Design-system decoupling contract (ADR-0028): core crates never depend on a
# design system. `flui-material`/`flui-cupertino` depend downward on the
# widgets substrate (see docs/FOUNDATIONS.md's layer DAG, L7 --> L6); the
# reverse edge must never exist, or Flutter's own pre-decoupling coupling
# (material/cupertino baked into the framework core) reappears in FLUI.
#
# This lives here, not in `port-check.sh`'s regex triggers, because it is a
# `Cargo.toml` dependency-graph fact, not a source-pattern fact — the same
# reason `port-check.sh`'s own dependency-adjacent checks (N-geom.U16, the
# sanctioned-dyn-boundary allowlist, ...) grep `.rs` sources: they audit
# *usage*, whereas this needs the declared dependency edge itself. This
# script already parses `cargo metadata`'s per-package `dependencies` (see
# the path/version check above) — the only place in the repo doing that today.
#
# Keep this set explicit because the guard spans every dependency kind, unlike
# the normal-edge layer rule below.
design_system_crates = {"flui-material", "flui-cupertino"}
# The only crates *allowed* to point at a design system: the design systems
# themselves (self-deps are nonsensical but harmless to exempt), the global
# localization implementation package, the application/composition-root crate,
# and anything outside `crates/` (examples, tools). The localization exception
# is directional: ADR-0041 separately forbids material/cupertino ->
# flui-localizations while permitting flui-localizations -> design systems.
design_system_dependents_allowed = design_system_crates | {
    "flui-localizations",
    "flui-app",
    "flui",
}

for package in workspace_packages:
    manifest = Path(package["manifest_path"]).resolve()
    rel = manifest.relative_to(root)
    name = package["name"]

    if name not in active_crate_manifests:
        continue
    if name in design_system_dependents_allowed:
        continue

    for dependency in package["dependencies"]:
        if dependency["name"] in design_system_crates:
            errors.append(
                f"{rel} (core crate `{name}`) declares a dependency on design-system crate "
                f"`{dependency['name']}` — forbidden by ADR-0028 (design-system decoupling "
                "contract): core must never depend on flui-material/flui-cupertino"
            )

# Workspace topology contract (ADR-0041): the declared layer policy in
# `docs/workspace-layers.toml` is validated against Cargo's *normal* edges.
#
# The design-system rule above proves one forbidden pair. It does not prove
# that the rest of the graph matches the documented architecture, which is how
# `flui-view -> flui-objects` stayed mis-drawn in `docs/FOUNDATIONS.md` while
# `cargo metadata` and every inventory check stayed green: member names and
# metadata inheritance were covered, dependency *direction* was not.
#
# Normal edges only, deliberately. A dev-dependency is a testing convenience,
# not an architectural claim, and Cargo tolerates dev-dependency cycles that a
# normal edge could never form. (The ADR-0028 check above stays broader — it
# spans every dependency kind — because an accidental Material dev-dependency
# in a core crate is itself the coupling smell that rule exists to catch.)
policy_path = root / "docs" / "workspace-layers.toml"
policy_rel = policy_path.relative_to(root)

try:
    policy = tomllib.loads(policy_path.read_text())
except (OSError, tomllib.TOMLDecodeError) as error:
    print("workspace-inventory: drift detected", file=sys.stderr)
    print(f"  - cannot load {policy_rel}: {error}", file=sys.stderr)
    sys.exit(1)

VALID_DISPOSITIONS = {"keep", "rename", "narrow", "optionalize", "deferred-extraction"}
VALID_PLANNED_STATUSES = {"gated", "sanctioned"}

layer_names = {layer["rank"]: layer["name"] for layer in policy.get("layer", [])}

layer_of: dict[str, int] = {}
allowed_dependents_of: dict[str, set[str]] = {}
restriction_reason: dict[str, str] = {}
for entry in policy.get("member", []):
    member_name = entry["name"]
    if member_name in layer_of:
        errors.append(f"{policy_rel} lists member `{member_name}` more than once")
        continue
    if entry["layer"] not in layer_names:
        errors.append(
            f"{policy_rel} member `{member_name}` declares layer {entry['layer']}, "
            "which has no [[layer]] entry"
        )
    if entry["disposition"] not in VALID_DISPOSITIONS:
        errors.append(
            f"{policy_rel} member `{member_name}` declares disposition "
            f"{entry['disposition']!r}, expected one of {sorted(VALID_DISPOSITIONS)}"
        )
    layer_of[member_name] = entry["layer"]

    # A crate whose *depth* in the layer DAG says nothing about who may use it.
    # `flui-log` sits low so it can have no in-workspace dependencies, which
    # under the layer rule alone would let every crate above it depend on it —
    # the exact shape (a universally depended-on logging wrapper) that got the
    # original crate deleted. `allowed_dependents` names the complete set of
    # in-workspace crates permitted a *normal* edge into this member.
    if "allowed_dependents" in entry:
        permitted = entry["allowed_dependents"]
        if not isinstance(permitted, list) or not all(
            isinstance(name, str) and name for name in permitted
        ):
            errors.append(
                f"{policy_rel} member `{member_name}` declares `allowed_dependents` that is "
                "not a list of non-empty crate names"
            )
            continue
        if "why" not in entry or not str(entry.get("why", "")).strip():
            errors.append(
                f"{policy_rel} member `{member_name}` restricts its dependents but gives no "
                "`why` — the restriction has to say what it is protecting"
            )
        allowed_dependents_of[member_name] = set(permitted)
        restriction_reason[member_name] = " ".join(str(entry.get("why", "")).split())

# Every active `crates/*` member plus the root `flui` facade is governed by the
# contract. Adding a crate to the workspace without classifying it here fails
# the gate — which is what makes the `[[planned]]` extraction gates below
# enforceable rather than advisory.
root_package_names = {
    package["name"]
    for package in workspace_packages
    if Path(package["manifest_path"]).resolve().parent == root
}
governed_names = set(active_crates) | root_package_names

for missing in sorted(governed_names - set(layer_of)):
    errors.append(
        f"{policy_rel} does not classify workspace member `{missing}` — add a [[member]] "
        "entry with its layer and disposition (see the [[planned]] gates before creating "
        "a new crate)"
    )
for extra in sorted(set(layer_of) - governed_names):
    errors.append(f"{policy_rel} classifies `{extra}`, which is not an active workspace member")

same_layer_allowed = {
    (edge["from"], edge["to"]) for edge in policy.get("same_layer_edge", [])
}
forbidden_pairs = {
    (edge["from"], edge["to"]): edge["why"] for edge in policy.get("forbidden_edge", [])
}

for restricted, permitted in sorted(allowed_dependents_of.items()):
    for name in sorted(permitted):
        if name not in layer_of:
            errors.append(
                f"{policy_rel} member `{restricted}` permits `{name}` as a dependent, but "
                f"`{name}` is not a classified member"
            )

for source, target in sorted(same_layer_allowed):
    if source not in layer_of or target not in layer_of:
        errors.append(f"{policy_rel} same_layer_edge `{source} -> {target}` names an unclassified crate")
    elif layer_of[source] != layer_of[target]:
        errors.append(
            f"{policy_rel} same_layer_edge `{source} -> {target}` is not a same-layer pair "
            f"({layer_of[source]} vs {layer_of[target]}) — it needs no exemption"
        )

# Normal in-workspace edges, deduplicated across target-specific and optional
# dependency tables (a `[target.'cfg(...)'.dependencies]` entry is still a real
# architectural edge, and `cargo metadata` lists it separately).
workspace_names = {package["name"] for package in workspace_packages}
normal_edges: dict[str, set[str]] = {}
for package in workspace_packages:
    normal_edges[package["name"]] = {
        dependency["name"]
        for dependency in package["dependencies"]
        if dependency.get("kind") is None and dependency["name"] in workspace_names
    }

for package in workspace_packages:
    name = package["name"]
    if name not in layer_of:
        continue
    rel = Path(package["manifest_path"]).resolve().relative_to(root)

    for target in sorted(normal_edges[name]):
        if target == name:
            continue

        if target not in layer_of:
            reason = (
                f"`{target}` is not classified in {policy_rel}"
                if target in governed_names
                else f"`{target}` is an example/tool member, and those are terminal "
                "consumers that nothing under `crates/` may depend on"
            )
            errors.append(f"{rel} (`{name}`) has a normal dependency on {reason}")
            continue

        permitted = allowed_dependents_of.get(target)
        if permitted is not None and name not in permitted:
            errors.append(
                f"{rel} declares the normal edge `{name} -> {target}`, but `{target}` is "
                f"restricted to {sorted(permitted)} in {policy_rel}: "
                f"{restriction_reason.get(target, '')}"
            )
            continue

        why = forbidden_pairs.get((name, target))
        if why is not None:
            errors.append(
                f"{rel} declares the forbidden normal edge `{name} -> {target}`: "
                f"{' '.join(why.split())}"
            )
            continue

        source_layer, target_layer = layer_of[name], layer_of[target]
        if target_layer < source_layer:
            continue
        if target_layer == source_layer and (name, target) in same_layer_allowed:
            continue

        if target_layer == source_layer:
            errors.append(
                f"{rel} declares the same-layer normal edge `{name} -> {target}` "
                f"(both L{source_layer} — {layer_names[source_layer]}) without a "
                f"[[same_layer_edge]] exemption in {policy_rel}"
            )
        else:
            errors.append(
                f"{rel} declares the upward normal edge `{name} -> {target}` "
                f"(L{source_layer} {layer_names[source_layer]} -> L{target_layer} "
                f"{layer_names[target_layer]}) — dependencies point down the layer DAG"
            )

# Acyclicity over the normal graph *plus* the projected edges. Cargo already
# rejects a normal-edge cycle, so the value here is the projection: it proves
# the future localization graph stays acyclic before the edges that would close
# a cycle actually exist.
projected: dict[str, set[str]] = {}
for edge in policy.get("projected_edge", []):
    if edge["from"] not in layer_of or edge["to"] not in layer_of:
        errors.append(f"{policy_rel} projected_edge `{edge['from']} -> {edge['to']}` names an unclassified crate")
        continue
    projected.setdefault(edge["from"], set()).add(edge["to"])

adjacency = {
    name: (normal_edges.get(name, set()) | projected.get(name, set())) & set(layer_of)
    for name in layer_of
}

VISITING, DONE = 1, 2
state: dict[str, int] = {}
reported_cycles: set[tuple[str, ...]] = set()


def canonical_cycle(cycle: list[str]) -> tuple[str, ...]:
    """Return a rotation-independent key for a closed directed cycle."""
    nodes = cycle[:-1]
    return min(tuple(nodes[index:] + nodes[:index]) for index in range(len(nodes)))


def find_cycles(node: str, stack: list[str]) -> None:
    state[node] = VISITING
    stack.append(node)
    for neighbour in sorted(adjacency[node]):
        if state.get(neighbour) == VISITING:
            cycle = stack[stack.index(neighbour):] + [neighbour]
            key = canonical_cycle(cycle)
            if key not in reported_cycles:
                reported_cycles.add(key)
                errors.append(
                    f"{policy_rel}: normal + projected dependency cycle " + " -> ".join(cycle)
                )
        elif state.get(neighbour) is None:
            find_cycles(neighbour, stack)
    stack.pop()
    state[node] = DONE


for node in sorted(adjacency):
    if state.get(node) is None:
        find_cycles(node, [])

# A planned entry is a reviewed extraction decision, so validate its schema
# instead of letting a typo silently disable a gate. Planned entries are removed
# when the crate becomes an active, classified member.
planned_by_name: dict[str, dict] = {}
required_planned_fields = {"name", "status", "owner_issue", "gate"}
for entry in policy.get("planned", []):
    missing_fields = sorted(required_planned_fields - set(entry))
    if missing_fields:
        errors.append(
            f"{policy_rel} planned entry is missing required fields: "
            + ", ".join(missing_fields)
        )
        continue

    planned_name = entry["name"]
    if not isinstance(planned_name, str) or not planned_name:
        errors.append(f"{policy_rel} planned entry has a non-string or empty `name`")
        continue
    if planned_name in planned_by_name:
        errors.append(f"{policy_rel} lists planned crate `{planned_name}` more than once")
        continue
    planned_by_name[planned_name] = entry

    planned_status = entry["status"]
    if not isinstance(planned_status, str) or planned_status not in VALID_PLANNED_STATUSES:
        errors.append(
            f"{policy_rel} planned crate `{planned_name}` declares status "
            f"{planned_status!r}, expected one of {sorted(VALID_PLANNED_STATUSES)}"
        )
    if not isinstance(entry["owner_issue"], int) or entry["owner_issue"] <= 0:
        errors.append(
            f"{policy_rel} planned crate `{planned_name}` must declare a positive integer `owner_issue`"
        )
    if not isinstance(entry["gate"], str) or not entry["gate"].strip():
        errors.append(
            f"{policy_rel} planned crate `{planned_name}` must declare a non-empty string `gate`"
        )
    if planned_name in layer_of:
        errors.append(
            f"{policy_rel} lists `{planned_name}` as both [[planned]] and [[member]]; "
            "remove the planned entry when the crate is activated"
        )

# A `[[planned]]` crate marked `gated` may not have a manifest yet, whether or
# not it was added to workspace.members. Creating it is a contract change:
# satisfy the gate, then replace the planned entry with a [[member]] entry.
for planned_name, entry in planned_by_name.items():
    planned_manifest = root / "crates" / planned_name / "Cargo.toml"
    if entry["status"] == "gated" and (
        planned_manifest.is_file() or planned_name in workspace_names
    ):
        errors.append(
            f"planned crate `{planned_name}` exists, but {policy_rel} still marks it "
            f"`status = \"gated\"` (issue #{entry['owner_issue']}). Extraction gate: "
            + " ".join(entry["gate"].split())
        )

if errors:
    print("workspace-inventory: drift detected", file=sys.stderr)
    for error in errors:
        print(f"  - {error}", file=sys.stderr)
    sys.exit(1)

print(
    f"workspace-inventory: {len(active_crates)} active crates covered; "
    f"{len(layer_of)} members across {len(layer_names)} layers, "
    f"{sum(len(targets) for targets in normal_edges.values())} normal edges checked"
)
PY
