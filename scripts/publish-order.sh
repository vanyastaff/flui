#!/usr/bin/env bash
# Dry-run `cargo publish` for every publishable workspace crate, in
# dependency order (a crate's workspace-internal dependencies before the
# crate itself) so the report reads leaves-first, the way a real publish
# sequence would.
#
# Every crate that isn't actually released yet will fail its dry-run the
# moment it depends on another unreleased workspace crate — `cargo publish
# --dry-run` resolves path dependencies against the registry, and the exact
# pinned version (`flui-foo = { path = ..., version = "=X.Y.Z" }`, see
# AGENTS.md's version-pin policy) does not exist there yet. That is expected
# right now (nothing in this workspace is on crates.io except flui-cli) and
# is reported as informational, not a failure. What this script actually
# gates is everything ELSE `cargo publish --dry-run` checks per crate —
# missing `license`/`description`/`readme`, a malformed manifest, an
# uncommitted change (dry-run still packages the crate) — surfaced whether
# the crate is a leaf or not.
#
# Usage: bash scripts/publish-order.sh [--json]
set -euo pipefail

if [[ -z "${BASH_VERSINFO:-}" ]]; then
  echo "error: run this with bash, not sh" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

json_out=0
if [[ "${1:-}" == "--json" ]]; then
  json_out=1
fi

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

# `cargo metadata`'s output for this workspace is multiple MB — too large to
# pass as a shell argument (hits ARG_MAX), hence the temp file + stdin below
# rather than `python3 - "$(cargo metadata ...)"`.
cargo metadata --format-version 1 --locked > "$work_dir/metadata.json"

# Topological order (Kahn's algorithm) over workspace-internal edges only,
# restricted to crates that are actually meant to publish (`publish` is
# absent/null, meaning "any registry", or explicitly lists one — `publish =
# false` serializes as an empty list and is excluded). Writes the order (one
# crate per line) and a parallel "crate<TAB>dep1,dep2,..." edge map, so the
# rest of this script never has to re-invoke Python per crate.
python3 - "$work_dir/order.txt" "$work_dir/edges.tsv" "$work_dir/metadata.json" <<'PY'
import json, sys

order_path, edges_path, metadata_path = sys.argv[1], sys.argv[2], sys.argv[3]
with open(metadata_path, encoding="utf-8") as f:
    metadata = json.load(f)
packages = {p["name"]: p for p in metadata["packages"]}
workspace_ids = set(metadata["workspace_members"])
workspace_names = {p["name"] for p in metadata["packages"] if p["id"] in workspace_ids}


def is_publishable(pkg):
    publish = pkg.get("publish")
    return publish is None or len(publish) > 0


publishable = {name for name in workspace_names if is_publishable(packages[name])}

edges = {name: set() for name in publishable}
for name in publishable:
    pkg = packages[name]
    for dep in pkg.get("dependencies", []):
        dep_name = dep["name"]
        if dep_name in publishable and dep.get("kind") in (None, "normal", "build"):
            edges[name].add(dep_name)

remaining = {name: set(deps) for name, deps in edges.items()}
ordered = []
while remaining:
    ready = sorted(name for name, deps in remaining.items() if not deps)
    if not ready:
        cyclic = sorted(remaining)
        print(f"error: dependency cycle among workspace crates: {cyclic}", file=sys.stderr)
        sys.exit(1)
    for name in ready:
        ordered.append(name)
        del remaining[name]
    for deps in remaining.values():
        deps.difference_update(ready)

with open(order_path, "w", encoding="utf-8") as f:
    f.write("\n".join(ordered) + "\n")

with open(edges_path, "w", encoding="utf-8") as f:
    for name in ordered:
        f.write(f"{name}\t{','.join(sorted(edges[name]))}\n")
PY

order=()
while IFS= read -r name; do
  [[ -n "$name" ]] && order+=("$name")
done < "$work_dir/order.txt"

echo "publish-order: ${#order[@]} publishable workspace crate(s), dependency order:" >&2
printf '  %s\n' "${order[@]}" >&2
echo >&2

: > "$work_dir/results.tsv"
unexpected=0

for crate in "${order[@]}"; do
  deps_field="$(awk -F'\t' -v c="$crate" '$1 == c { print $2; exit }' "$work_dir/edges.tsv")"

  set +e
  out="$(cargo publish --dry-run --locked -p "$crate" 2>&1)"
  rc=$?
  set -e

  if [[ "$rc" -eq 0 ]]; then
    status="ok"
  elif [[ -n "$deps_field" ]] && echo "$out" | grep -qE "failed to select a version for the requirement|not found in registry|does not exist in registry"; then
    status="expected (dependency not published)"
  else
    status="UNEXPECTED FAILURE"
    unexpected=$((unexpected + 1))
  fi

  printf '%s\t%s\n' "$crate" "$status" >> "$work_dir/results.tsv"
  echo "publish-order: $crate -> $status" >&2
  if [[ "$status" == "UNEXPECTED FAILURE" ]]; then
    echo "--- cargo publish --dry-run -p $crate output ---" >&2
    echo "$out" >&2
    echo "---" >&2
  fi
done

echo >&2
echo "publish-order: ${#order[@]} crate(s) checked, $unexpected unexpected failure(s)" >&2

if [[ "$json_out" -eq 1 ]]; then
  python3 - "$work_dir/results.tsv" <<'PY'
import json, sys

rows = []
with open(sys.argv[1], encoding="utf-8") as f:
    for line in f:
        line = line.rstrip("\n")
        if not line:
            continue
        name, status = line.split("\t", 1)
        rows.append({"crate": name, "status": status})
print(json.dumps(rows, indent=2))
PY
fi

if [[ "$unexpected" -gt 0 ]]; then
  exit 1
fi
exit 0
