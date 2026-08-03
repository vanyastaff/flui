#!/usr/bin/env bash
# Validate the Runtime.1 conformance registry (docs/runtime-conformance.toml)
# against the source tree.
#
# This is the executable half of the Runtime.1 conformance matrix: the
# registry records what each runtime ADR clause's state actually is and how
# every public runtime surface is classified; this script keeps those claims
# honest. It follows the same shape as scripts/check-workspace-inventory.sh —
# a thin bash wrapper around an embedded python3 program using the stdlib
# structural TOML parser (tomllib), never regexes-over-TOML.
#
# What it verifies — and, deliberately documented, what it cannot:
#
#   * Registry integrity: known states/classifications/domains, unique
#     non-empty descriptive keys, evidence present for `implemented`, exactly
#     one owning issue (#551-#565) for `partial`/`planned`, no documentation
#     files cited as implementation evidence.
#   * Citation existence: every cited file exists and contains the cited
#     string. A `contains` match proves the symbol/test still exists at that
#     path — it does NOT prove behavior. Behavior lives in the cited tests,
#     which `just test` executes.
#   * Source gates: retired identifiers stay deleted, `run_direct` keeps its
#     experimental marking, ADR-0039 stays Proposed, no `flui-presentation`
#     crate appears, and new ambient singletons / public lock-shaped surfaces
#     in the runtime crates must be registered with an owner. The singleton
#     and lock nets are textual (`impl_binding_singleton!`, `fn instance() ->
#     &'static`, `PORT-CHECK-OK-SP6`): a singleton built without those idioms,
#     or a lock surface port-check trigger #12's grammar misses, is invisible
#     to them. This is a targeted gate, not full Rust API analysis.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "${repo_root}"

if ! command -v python3 >/dev/null 2>&1; then
  echo "runtime-conformance: python3 not found on PATH" >&2
  exit 2
fi

python3 - "${repo_root}" <<'PY'
import sys
import tomllib
from pathlib import Path

root = Path(sys.argv[1]).resolve()
registry_path = root / "docs" / "runtime-conformance.toml"
registry_rel = registry_path.relative_to(root)

errors: list[str] = []


def fail(message: str) -> None:
    errors.append(message)


try:
    registry = tomllib.loads(registry_path.read_text())
except (OSError, tomllib.TOMLDecodeError) as error:
    print("runtime-conformance: violations detected", file=sys.stderr)
    print(f"  - cannot load {registry_rel}: {error}", file=sys.stderr)
    sys.exit(1)

# ---------------------------------------------------------------------------
# Contract vocabulary. The registry's [registry] header must agree, so the
# file and this script cannot drift apart silently.
# ---------------------------------------------------------------------------
ADR_SCOPE = ["ADR-0027", "ADR-0037", "ADR-0029", "ADR-0039"]
VALID_STATES = {"implemented", "partial", "planned", "documented-divergence"}
VALID_CLASSIFICATIONS = {"stable-candidate", "experimental", "transitional", "removal-target"}
VALID_DOMAINS = {"application", "realm", "presentation", "raster", "platform", "shared-engine"}
VALID_EVIDENCE_KINDS = {"symbol", "test", "compile-time", "source-gate"}
OWNER_ISSUE_MIN, OWNER_ISSUE_MAX = 551, 565

# Floors, not proof of completeness: deleting requirement entries below these
# counts fails; whether every normative clause is represented remains an
# editorial judgment recorded in the entries themselves.
MIN_REQUIREMENTS_PER_ADR = {"ADR-0027": 19, "ADR-0037": 12, "ADR-0029": 7, "ADR-0039": 6}

ADR_FILES = {
    "ADR-0027": "docs/adr/ADR-0027-owner-affine-ui-realms.md",
    "ADR-0037": "docs/adr/ADR-0037-presentation-ownership-domains.md",
    "ADR-0029": "docs/adr/ADR-0029-frame-pacing-swapchain-block-with-fallback-throttle.md",
    "ADR-0039": "docs/adr/ADR-0039-event-loop-affinity-capability.md",
}

# Runtime crates covered by the singleton and lock-surface nets.
RUNTIME_CRATES = ["flui-app", "flui-scheduler", "flui-platform", "flui-engine"]

# The macro-defining file: holds the definition, doc examples, and cfg(test)
# invocations of impl_binding_singleton!; exempt from the singleton net.
SINGLETON_NET_EXEMPT = {Path("crates/flui-foundation/src/binding.rs")}

header = registry.get("registry", {})
if header.get("adr_scope") != ADR_SCOPE:
    fail(
        f"{registry_rel} [registry].adr_scope = {header.get('adr_scope')!r} does not match "
        f"this script's contract {ADR_SCOPE!r} — update both together, deliberately"
    )

for adr, rel in ADR_FILES.items():
    if not (root / rel).is_file():
        fail(f"{rel} is missing, but {adr} is in the conformance scope")


def check_owner_issue(entry: dict, label: str, required: bool) -> None:
    issue = entry.get("owner_issue")
    if issue is None:
        if required:
            fail(f"{label} has no owning issue; partial/planned/transitional work needs exactly one owner in #{OWNER_ISSUE_MIN}-#{OWNER_ISSUE_MAX}")
        return
    if not isinstance(issue, int) or not (OWNER_ISSUE_MIN <= issue <= OWNER_ISSUE_MAX):
        fail(f"{label} declares owner_issue {issue!r}; expected an integer in {OWNER_ISSUE_MIN}-{OWNER_ISSUE_MAX}")


def check_citation(file_value: object, contains_value: object, label: str, *, forbid_docs: bool = False) -> None:
    """A citation names a real file containing the cited string.

    Existence + substring only: this proves the cited symbol/test is still
    there, not that it behaves as claimed.
    """
    if not isinstance(file_value, str) or not file_value:
        fail(f"{label} has no file path")
        return
    if not isinstance(contains_value, str) or not contains_value:
        fail(f"{label} has no `contains` string")
        return
    if forbid_docs and file_value.endswith((".md", ".markdown")):
        fail(f"{label} cites documentation ({file_value}) — documentation is never implementation evidence")
        return
    path = root / file_value
    if not path.is_file():
        fail(f"{label} cites {file_value}, which does not exist")
        return
    try:
        text = path.read_text()
    except (OSError, UnicodeDecodeError) as error:
        fail(f"{label} cites {file_value}, which cannot be read: {error}")
        return
    if contains_value not in text:
        fail(f"{label}: {file_value} does not contain {contains_value!r} — the citation is stale or fabricated")


# ---------------------------------------------------------------------------
# Requirements
# ---------------------------------------------------------------------------
requirement_keys: set[str] = set()
requirements_by_adr: dict[str, int] = {adr: 0 for adr in ADR_SCOPE}
requirements = registry.get("requirement", [])

# Descriptive kebab-case keys only. Internal process-ID shapes (SC-NNN, U##,
# T-N/R-N/E-N, P#) are banned by the repo's Agent Rules.
import re

KEY_RE = re.compile(r"^[a-z][a-z0-9]*(-[a-z0-9]+)+$")
BANNED_KEY_RE = re.compile(r"(^|-)((sc|u|t|r|e|p)-?\d+)($|-)", re.IGNORECASE)

for index, entry in enumerate(requirements):
    key = entry.get("key")
    label = f"{registry_rel} requirement[{index}]"
    if not isinstance(key, str) or not key:
        fail(f"{label} has an empty or missing key")
        continue
    label = f"{registry_rel} requirement `{key}`"
    if key in requirement_keys:
        fail(f"{label} is declared more than once")
        continue
    requirement_keys.add(key)
    if not KEY_RE.match(key):
        fail(f"{label}: keys are descriptive kebab-case (`focus-per-presentation`), got {key!r}")
    if BANNED_KEY_RE.search(key):
        fail(f"{label}: numbered process-ID keys are banned by the repo's Agent Rules; use a descriptive key")

    adr = entry.get("adr")
    if adr not in ADR_SCOPE:
        fail(f"{label} names ADR {adr!r}, which is not in the conformance scope {ADR_SCOPE}")
    else:
        requirements_by_adr[adr] += 1

    if not str(entry.get("section", "")).strip():
        fail(f"{label} has no ADR section reference")
    if not str(entry.get("statement", "")).strip():
        fail(f"{label} has no normative statement")

    state = entry.get("state")
    if state not in VALID_STATES:
        fail(f"{label} declares state {state!r}; expected one of {sorted(VALID_STATES)}")
        continue

    domain = entry.get("domain")
    if domain not in VALID_DOMAINS:
        fail(f"{label} declares domain {domain!r}; expected one of {sorted(VALID_DOMAINS)}")

    mechanical = entry.get("mechanical")
    if not isinstance(mechanical, bool):
        fail(f"{label} must declare `mechanical = true/false` (is the clause mechanically checkable?)")
    elif mechanical and not str(entry.get("mechanism", "")).strip():
        fail(f"{label} claims to be mechanically checkable but names no mechanism")

    evidence = entry.get("evidence", [])
    if not isinstance(evidence, list):
        fail(f"{label} evidence must be a list of citations")
        evidence = []

    kinds_seen: set[str] = set()
    for citation_index, citation in enumerate(evidence):
        citation_label = f"{label} evidence[{citation_index}]"
        kind = citation.get("kind")
        if kind not in VALID_EVIDENCE_KINDS:
            fail(f"{citation_label} declares kind {kind!r}; expected one of {sorted(VALID_EVIDENCE_KINDS)}")
            continue
        kinds_seen.add(kind)
        check_citation(citation.get("file"), citation.get("contains"), citation_label, forbid_docs=(state == "implemented"))

    if state == "implemented":
        if "symbol" not in kinds_seen:
            fail(f"{label} is `implemented` but cites no production symbol")
        if not kinds_seen & {"test", "compile-time", "source-gate"}:
            fail(f"{label} is `implemented` but cites no test/compile-time/source-gate evidence — an unverified claim is a hypothesis")
        check_owner_issue(entry, label, required=False)
    elif state in {"partial", "planned"}:
        check_owner_issue(entry, label, required=True)
    elif state == "documented-divergence":
        if not str(entry.get("divergence", entry.get("statement", ""))).strip():
            fail(f"{label} is a documented divergence with no divergence description")

for adr, minimum in MIN_REQUIREMENTS_PER_ADR.items():
    if requirements_by_adr.get(adr, 0) < minimum:
        fail(
            f"{registry_rel} carries {requirements_by_adr.get(adr, 0)} requirements for {adr}, "
            f"below the recorded floor of {minimum} — requirements may be reworded, not silently dropped"
        )

# ---------------------------------------------------------------------------
# Public surfaces
# ---------------------------------------------------------------------------
surface_paths: set[str] = set()
run_direct_surface: dict | None = None
for index, entry in enumerate(registry.get("surface", [])):
    path_value = entry.get("path")
    label = f"{registry_rel} surface[{index}]"
    if not isinstance(path_value, str) or not path_value:
        fail(f"{label} has an empty or missing `path`")
        continue
    label = f"{registry_rel} surface `{path_value}`"
    if path_value in surface_paths:
        fail(f"{label} is declared more than once")
        continue
    surface_paths.add(path_value)

    for field in ("crate", "kind", "thread_affinity", "owner", "failure_semantics"):
        if not str(entry.get(field, "")).strip():
            fail(f"{label} is missing `{field}`")
    consumers = entry.get("consumers")
    if not isinstance(consumers, list):
        fail(f"{label} must record `consumers` as a list (empty is allowed, absence is not)")

    classification = entry.get("classification")
    if classification not in VALID_CLASSIFICATIONS:
        fail(f"{label} declares classification {classification!r}; expected one of {sorted(VALID_CLASSIFICATIONS)}")
        continue

    check_citation(entry.get("declared_in"), entry.get("contains"), f"{label} declaration pin")
    check_owner_issue(entry, label, required=classification in {"transitional", "removal-target"})

    if path_value == "flui_app::run_direct":
        run_direct_surface = entry

# run_direct must stay registered, experimental, and marked in its module docs.
if run_direct_surface is None:
    fail(f"{registry_rel} no longer registers surface `flui_app::run_direct`")
else:
    if run_direct_surface.get("classification") != "experimental":
        fail(
            f"{registry_rel} surface `flui_app::run_direct` lost its `experimental` classification "
            f"(now {run_direct_surface.get('classification')!r}); graduating it belongs to its owning issue"
        )
direct_rs = root / "crates" / "flui-app" / "src" / "app" / "direct.rs"
if not direct_rs.is_file():
    fail("crates/flui-app/src/app/direct.rs is gone; remove or update the run_direct gates deliberately")
elif "experimental" not in direct_rs.read_text().lower():
    fail("crates/flui-app/src/app/direct.rs no longer marks run_direct as experimental — ADR-0039 slice 2 owns its stabilization")

# ADR-0039 acceptance belongs to its owning issue; the file must stay Proposed.
adr_0039 = root / ADR_FILES["ADR-0039"]
if adr_0039.is_file() and "**Status:** Proposed" not in adr_0039.read_text():
    fail(
        f"{ADR_FILES['ADR-0039']} no longer carries `**Status:** Proposed` — accepting/implementing "
        "ADR-0039 belongs to its owning issue, and this gate plus the registry must be updated with it"
    )

# ---------------------------------------------------------------------------
# Known advisory / unwired configuration must never look stable.
# ---------------------------------------------------------------------------
VALID_CONFIG_STATUSES = {"unwired", "advisory", "partially-wired"}
# vsync does not control the present mode; target_fps is advisory. These two
# must be registered and must not be classified as working stable config.
REQUIRED_ADVISORY_FIELDS = {"vsync", "target_fps"}

config_fields: dict[str, dict] = {}
for index, entry in enumerate(registry.get("config_field", [])):
    name = entry.get("name")
    label = f"{registry_rel} config_field[{index}]"
    if not isinstance(name, str) or not name:
        fail(f"{label} has an empty or missing `name`")
        continue
    label = f"{registry_rel} config_field `{name}`"
    if name in config_fields:
        fail(f"{label} is declared more than once")
        continue
    config_fields[name] = entry

    status = entry.get("status")
    if status not in VALID_CONFIG_STATUSES:
        fail(f"{label} declares status {status!r}; expected one of {sorted(VALID_CONFIG_STATUSES)}")
    classification = entry.get("classification")
    if classification not in VALID_CLASSIFICATIONS:
        fail(f"{label} declares classification {classification!r}; expected one of {sorted(VALID_CLASSIFICATIONS)}")
    elif classification == "stable-candidate":
        fail(f"{label} is {status!r} yet classified `stable-candidate` — configuration that does not govern behavior is never stable")
    check_citation(entry.get("file"), entry.get("contains"), f"{label} field pin")
    check_owner_issue(entry, label, required=True)

for required in sorted(REQUIRED_ADVISORY_FIELDS):
    if required not in config_fields:
        fail(
            f"{registry_rel} must register config_field `{required}` — it is a known advisory/unwired "
            "field and removing its entry requires actually wiring or deleting the field first"
        )

# ---------------------------------------------------------------------------
# Ambient singleton net: every impl_binding_singleton!/manual instance() in
# the runtime crates must be registered with an owning issue.
# ---------------------------------------------------------------------------
singleton_exemptions: dict[Path, dict] = {}
for index, entry in enumerate(registry.get("singleton_exemption", [])):
    label = f"{registry_rel} singleton_exemption[{index}] `{entry.get('symbol', '?')}`"
    check_citation(entry.get("file"), entry.get("contains"), label)
    check_owner_issue(entry, label, required=True)
    if isinstance(entry.get("file"), str):
        singleton_exemptions[Path(entry["file"])] = entry

SINGLETON_MARKERS = ("impl_binding_singleton!(", "fn instance() -> &'static")
for crate in RUNTIME_CRATES + [
    "flui-foundation", "flui-view", "flui-rendering", "flui-interaction",
    "flui-semantics", "flui-painting",
]:
    crate_src = root / "crates" / crate / "src"
    if not crate_src.is_dir():
        continue
    for rs_file in sorted(crate_src.rglob("*.rs")):
        rel = rs_file.relative_to(root)
        if rel in SINGLETON_NET_EXEMPT:
            continue
        try:
            text = rs_file.read_text()
        except (OSError, UnicodeDecodeError):
            continue
        for marker in SINGLETON_MARKERS:
            if marker in text and rel not in singleton_exemptions:
                fail(
                    f"{rel} contains `{marker}` but is not in the registry's singleton allowlist — "
                    "a new ambient singleton cannot land without an owning issue "
                    "(ADR-0027: singleton retirement)"
                )

# ---------------------------------------------------------------------------
# Public lock-shaped surface net: every PORT-CHECK-OK-SP6 marker in the
# runtime crates must come from a registered file. (Trigger #12 in
# scripts/port-check.sh forces the marker onto any public lock surface its
# grammar can see; this gate forces the marker's file into the registry.)
# ---------------------------------------------------------------------------
lock_exempt_files = set()
for index, entry in enumerate(registry.get("lock_exemption", [])):
    label = f"{registry_rel} lock_exemption[{index}] `{entry.get('file', '?')}`"
    file_value = entry.get("file")
    if not isinstance(file_value, str) or not (root / file_value).is_file():
        fail(f"{label} names a missing file")
        continue
    if not str(entry.get("reason", "")).strip():
        fail(f"{label} has no reason")
    check_owner_issue(entry, label, required=True)
    lock_exempt_files.add(Path(file_value))

for crate in RUNTIME_CRATES:
    crate_src = root / "crates" / crate / "src"
    if not crate_src.is_dir():
        continue
    for rs_file in sorted(crate_src.rglob("*.rs")):
        rel = rs_file.relative_to(root)
        try:
            text = rs_file.read_text()
        except (OSError, UnicodeDecodeError):
            continue
        if "PORT-CHECK-OK-SP6" in text and rel not in lock_exempt_files:
            fail(
                f"{rel} carries a PORT-CHECK-OK-SP6 lock exemption but is not in the registry's "
                "lock allowlist — a public lock-shaped runtime surface needs an owning issue (SP-6)"
            )

# ---------------------------------------------------------------------------
# Process-global guards: existence-pinned so retirement updates the registry.
# ---------------------------------------------------------------------------
for index, entry in enumerate(registry.get("process_global_guard", [])):
    label = f"{registry_rel} process_global_guard `{entry.get('symbol', '?')}`"
    check_citation(entry.get("file"), entry.get("contains"), label)
    check_owner_issue(entry, label, required=True)

# ---------------------------------------------------------------------------
# Forbidden patterns/paths. Substring scans over crates/**/*.rs (comments
# included: these are retired identifiers that must not reappear even as
# prose). `scope` narrows a pattern to one file or directory.
# ---------------------------------------------------------------------------
all_rs_files: list[Path] = [
    p for p in sorted((root / "crates").rglob("*.rs")) if "target" not in p.relative_to(root).parts
]

for index, entry in enumerate(registry.get("forbidden_pattern", [])):
    pattern = entry.get("pattern")
    label = f"{registry_rel} forbidden_pattern[{index}]"
    if not isinstance(pattern, str) or not pattern:
        fail(f"{label} has an empty pattern")
        continue
    if not str(entry.get("why", "")).strip():
        fail(f"{label} (`{pattern}`) has no `why`")
    allow = {Path(a) for a in entry.get("allow_files", [])}
    scope = entry.get("scope")
    if scope:
        scope_path = root / scope
        candidates = [scope_path] if scope_path.is_file() else sorted(scope_path.rglob("*.rs"))
    else:
        candidates = all_rs_files
    for rs_file in candidates:
        rel = rs_file.relative_to(root)
        if rel in allow:
            continue
        try:
            text = rs_file.read_text()
        except (OSError, UnicodeDecodeError):
            continue
        if pattern in text:
            fail(f"{rel} contains forbidden pattern `{pattern}`: {' '.join(str(entry.get('why', '')).split())}")

for index, entry in enumerate(registry.get("forbidden_path", [])):
    path_value = entry.get("path")
    if not isinstance(path_value, str) or not path_value:
        fail(f"{registry_rel} forbidden_path[{index}] has an empty path")
        continue
    if (root / path_value).exists():
        fail(f"{path_value} exists but is forbidden: {' '.join(str(entry.get('why', '')).split())}")

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------
if errors:
    print("runtime-conformance: violations detected", file=sys.stderr)
    for error in errors:
        print(f"  - {error}", file=sys.stderr)
    sys.exit(1)

surface_count = len(surface_paths)
requirement_count = len(requirement_keys)
by_adr = ", ".join(f"{adr} {count}" for adr, count in requirements_by_adr.items())
print(
    f"runtime-conformance: {requirement_count} requirements ({by_adr}); "
    f"{surface_count} classified surfaces; {len(config_fields)} config fields; "
    f"{len(singleton_exemptions)} singleton + {len(lock_exempt_files)} lock exemptions verified"
)
PY
