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

# Two edge sets, deliberately different:
#  - `order_edges` (normal + build only) decides PUBLISH ORDER: a dev-only
#    dependency does not need to exist on the registry before a downstream
#    consumer can *use* this crate, so it must not gate the topological sort
#    (a dev-dep cycle between two crates would otherwise be reported as an
#    unpublishable cycle it isn't).
#  - `verify_edges` (normal + build + dev) decides what counts as an
#    EXPECTED dry-run failure: `cargo publish --dry-run` (no `--no-verify`
#    here — the whole point is to also catch packaging-completeness bugs)
#    extracts the package and re-resolves its FULL manifest, dev-deps
#    included, to compile its test targets during verification. A crate
#    whose only internal edge is a dev-dependency (flui-macros -> flui-
#    foundation, real example: kept as a dev-dep specifically to avoid a
#    production cycle, per that crate's own Cargo.toml comment) still fails
#    dry-run on that dependency not being published, and that failure is
#    exactly as "expected" as a normal-dependency one.
order_edges = {name: set() for name in publishable}
verify_edges = {name: set() for name in publishable}
for name in publishable:
    pkg = packages[name]
    for dep in pkg.get("dependencies", []):
        dep_name = dep["name"]
        if dep_name not in publishable:
            continue
        kind = dep.get("kind")
        if kind in (None, "normal", "build"):
            order_edges[name].add(dep_name)
            verify_edges[name].add(dep_name)
        elif kind == "dev":
            verify_edges[name].add(dep_name)

remaining = {name: set(deps) for name, deps in order_edges.items()}
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
        f.write(f"{name}\t{','.join(sorted(verify_edges[name]))}\n")
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

  status="UNEXPECTED FAILURE"
  if [[ "$rc" -eq 0 ]]; then
    status="ok"
  elif [[ -n "$deps_field" ]]; then
    # Match against cargo's ACTUAL messages (verified against a real cargo
    # 1.98 run, not guessed): "no matching package named `X`" and "failed to
    # select a version for the requirement `X ..."`. Matched by one of THIS
    # crate's own internal dependency names specifically, via `<<<`, not a
    # pipe into `grep -q` (which SIGPIPEs the writer once `-q` finds its
    # match and exits — harmless for `echo`, but this project's convention
    # under `set -o pipefail` is to avoid the pattern rather than rely on
    # `echo`'s SIGPIPE being silently tolerated) — so a genuinely unrelated
    # failure (a bad license field, a missing readme, cargo's own bug) on a
    # non-leaf crate is never miscounted as "expected".
    IFS=',' read -r -a deps_array <<<"$deps_field"
    for dep in "${deps_array[@]}"; do
      [[ -z "$dep" ]] && continue
      if grep -qE "no matching package named \`${dep}\`|failed to select a version for the requirement \`${dep}[ \`]" <<<"$out"; then
        status="expected (dependency not published: $dep)"
        break
      fi
    done
  fi
  if [[ "$status" == "UNEXPECTED FAILURE" ]]; then
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
