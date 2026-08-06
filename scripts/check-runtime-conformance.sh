#!/usr/bin/env bash
# Validate the Runtime contract registry (docs/runtime-contract.toml)
# against the source tree.
#
# This is the executable half of the Runtime.1 public contract registry: it
# records shipped invariants, planned ownership, and classified public runtime
# boundary families without depending on internal design records. It follows
# the same shape as scripts/check-workspace-inventory.sh —
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
#   * Source gates: root export declarations are compared with an explicit
#     manifest; retired identifiers stay deleted; `run_direct` keeps its
#     experimental marking; and ambient singletons / public lock exceptions
#     are matched declaration-by-declaration. This is a targeted boundary
#     gate, not a claim that source text is a Rust semantic API analyzer.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "${repo_root}"

if ! command -v python3 >/dev/null 2>&1; then
  echo "runtime-conformance: python3 not found on PATH" >&2
  exit 2
fi

python3 - "${repo_root}" <<'PY'
import re
import sys
import tomllib
from pathlib import Path

root = Path(sys.argv[1]).resolve()
registry_path = root / "docs" / "runtime-contract.toml"
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
# Contract vocabulary.
# ---------------------------------------------------------------------------
VALID_STATES = {"implemented", "partial", "planned", "documented-divergence"}
VALID_CLASSIFICATIONS = {"stable-candidate", "experimental", "transitional", "removal-target"}
# "workspace" is deliberately not a Runtime.1 topology domain (the other six
# are). It exists for the rare cross-cutting engineering-discipline contract
# that has no other mechanically-checked registry to live in (e.g. the
# expect("BUG: …") panic-policy gate) -- narrow on purpose, not a general
# dumping ground for anything that lacks a home.
VALID_DOMAINS = {"application", "realm", "presentation", "raster", "platform", "shared-engine", "workspace"}
VALID_EVIDENCE_KINDS = {"symbol", "test", "compile-time", "source-gate"}
OWNER_ISSUE_MIN, OWNER_ISSUE_MAX = 551, 565

# Runtime crates covered by the singleton and lock-surface nets.
RUNTIME_CRATES = ["flui-app", "flui-scheduler", "flui-platform", "flui-engine"]

# Empty: the macro-defining file (crates/flui-foundation/src/binding.rs) that
# used to need this exemption is deleted along with impl_binding_singleton!
# itself under #553 -- there is no `HasInstance`/`BindingBase` left anywhere
# in the workspace, so nothing needs exempting from the singleton net below.
SINGLETON_NET_EXEMPT: set[Path] = set()

header = registry.get("registry", {})
if not isinstance(header, dict):
    fail(f"{registry_rel} [registry] must be a table")
    header = {}
if header.get("schema") != 1:
    fail(f"{registry_rel} [registry].schema must be 1")


def table_array(name: str) -> list[dict]:
    value = registry.get(name, [])
    if not isinstance(value, list):
        fail(f"{registry_rel} `{name}` must be an array of tables (`[[{name}]]`)")
        return []
    result: list[dict] = []
    for index, entry in enumerate(value):
        if not isinstance(entry, dict):
            fail(f"{registry_rel} {name}[{index}] must be a table")
            continue
        result.append(entry)
    return result


def check_owner_issue(entry: dict, label: str, required: bool) -> None:
    issue = entry.get("owner_issue")
    if issue is None:
        if required:
            fail(f"{label} has no owning issue; partial/planned/transitional work needs exactly one owner in #{OWNER_ISSUE_MIN}-#{OWNER_ISSUE_MAX}")
        return
    if isinstance(issue, bool) or not isinstance(issue, int) or not (OWNER_ISSUE_MIN <= issue <= OWNER_ISSUE_MAX):
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
    path = (root / file_value).resolve()
    if not path.is_relative_to(root):
        fail(f"{label} cites path outside the repository: {file_value}")
        return
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
# Contracts
# ---------------------------------------------------------------------------
contract_keys: set[str] = set()
contracts_by_state: dict[str, int] = {state: 0 for state in VALID_STATES}
contracts = table_array("contract")

# Descriptive kebab-case keys only. Internal process-ID shapes (SC-NNN, U##,
# T-N/R-N/E-N, P#) are banned by the repo's Agent Rules.
KEY_RE = re.compile(r"^[a-z][a-z0-9]*(-[a-z0-9]+)+$")
BANNED_KEY_RE = re.compile(r"(^|-)((sc|u|t|r|e|p)-?\d+)($|-)", re.IGNORECASE)

for index, entry in enumerate(contracts):
    key = entry.get("key")
    label = f"{registry_rel} contract[{index}]"
    if not isinstance(key, str) or not key:
        fail(f"{label} has an empty or missing key")
        continue
    label = f"{registry_rel} contract `{key}`"
    if key in contract_keys:
        fail(f"{label} is declared more than once")
        continue
    contract_keys.add(key)
    if not KEY_RE.match(key):
        fail(f"{label}: keys are descriptive kebab-case (`focus-per-presentation`), got {key!r}")
    if BANNED_KEY_RE.search(key):
        fail(f"{label}: numbered process-ID keys are banned by the repo's Agent Rules; use a descriptive key")

    if not str(entry.get("statement", "")).strip():
        fail(f"{label} has no normative statement")

    state = entry.get("state")
    if state not in VALID_STATES:
        fail(f"{label} declares state {state!r}; expected one of {sorted(VALID_STATES)}")
        continue
    contracts_by_state[state] += 1

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
        if not isinstance(citation, dict):
            fail(f"{citation_label} must be a table")
            continue
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
        if not str(entry.get("divergence", "")).strip():
            fail(f"{label} is a documented divergence with no divergence description")

# ---------------------------------------------------------------------------
# Public surfaces
# ---------------------------------------------------------------------------
surface_paths: set[str] = set()
run_direct_surface: dict | None = None
for index, entry in enumerate(table_array("surface")):
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

# ---------------------------------------------------------------------------
# Root export manifest
# ---------------------------------------------------------------------------
def root_public_declarations(path: Path) -> list[str]:
    """Return normalized column-zero `pub` declarations from one crate root.

    The manifest intentionally controls export roots, not every item reachable
    through a public module. Full Rust semantic API extraction requires
    unstable rustdoc JSON on the pinned toolchain; module families are
    classified above and their root growth is kept review-visible here.
    """
    lines = path.read_text().splitlines()
    declarations: list[str] = []
    index = 0
    while index < len(lines):
        line = lines[index].split("//", 1)[0].rstrip()
        if not line.startswith("pub "):
            index += 1
            continue
        parts = [line.strip()]
        is_use = line.startswith("pub use ")
        while True:
            joined = " ".join(parts)
            if ";" in joined or (not is_use and "{" in joined):
                break
            index += 1
            if index >= len(lines):
                break
            continued = lines[index].split("//", 1)[0].strip()
            if continued:
                parts.append(continued)
        declarations.append(" ".join(" ".join(parts).split()))
        index += 1
    return declarations


export_root_files: set[Path] = set()
for index, entry in enumerate(table_array("export_root")):
    label = f"{registry_rel} export_root[{index}]"
    file_value = entry.get("file")
    expected = entry.get("declarations")
    if not isinstance(file_value, str) or not file_value:
        fail(f"{label} has no file path")
        continue
    rel = Path(file_value)
    if rel in export_root_files:
        fail(f"{label} duplicates export root {file_value}")
        continue
    export_root_files.add(rel)
    path = root / rel
    if not path.is_file():
        fail(f"{label} names missing file {file_value}")
        continue
    if not isinstance(expected, list) or not all(isinstance(item, str) and item for item in expected):
        fail(f"{label} declarations must be a list of non-empty strings")
        continue
    if len(expected) != len(set(expected)):
        fail(f"{label} declarations contain duplicates")
    actual = root_public_declarations(path)
    missing = sorted(set(expected) - set(actual))
    unregistered = sorted(set(actual) - set(expected))
    for declaration in missing:
        fail(f"{file_value} no longer exports registered root declaration {declaration!r}")
    for declaration in unregistered:
        fail(
            f"{file_value} has unregistered root export {declaration!r} — classify its boundary family "
            "and update the explicit export manifest"
        )

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
    fail("crates/flui-app/src/app/direct.rs no longer marks run_direct as experimental — issue #551 owns its stabilization")

# ---------------------------------------------------------------------------
# Known advisory / unwired configuration must never look stable.
#
# `vsync`/`target_fps` used to live here as required advisory fields (they
# governed nothing real, so they were registered instead of left silently
# misleading). Issue #556 §5 removed both fields outright — the
# must-*exist* mechanism below has no more work to do for them, and the
# `forbidden_pattern` table further down (a must-NOT-exist mode this
# checker already had for retired identifiers, e.g. `HasInstance`,
# `REALM_CLAIMED`) enforces their absence instead: re-adding
# `pub vsync: bool` or `pub target_fps: u32` to `AppConfig` fails
# conformance without a second checker mode being built for it.
# ---------------------------------------------------------------------------
VALID_CONFIG_STATUSES = {"unwired", "advisory", "partially-wired"}

config_fields: dict[str, dict] = {}
for index, entry in enumerate(table_array("config_field")):
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

# ---------------------------------------------------------------------------
# Ambient singleton net: every impl_binding_singleton!/manual instance() in
# the runtime crates must be registered with an owning issue.
# ---------------------------------------------------------------------------
singleton_exemptions: dict[Path, list[str]] = {}
for index, entry in enumerate(table_array("singleton_exemption")):
    label = f"{registry_rel} singleton_exemption[{index}] `{entry.get('symbol', '?')}`"
    file_value = entry.get("file")
    contains = entry.get("contains")
    check_citation(file_value, contains, label)
    check_owner_issue(entry, label, required=True)
    if isinstance(file_value, str) and isinstance(contains, str):
        singleton_exemptions.setdefault(Path(file_value), []).append(contains)

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
        expected = singleton_exemptions.get(rel, [])
        for contains in expected:
            count = text.count(contains)
            if count != 1:
                fail(f"{rel} must contain registered singleton declaration {contains!r} exactly once; found {count}")
        for line_number, line in enumerate(text.splitlines(), start=1):
            marker = next((candidate for candidate in SINGLETON_MARKERS if candidate in line), None)
            if marker is None:
                continue
            matches = [contains for contains in expected if contains in line]
            if len(matches) != 1:
                fail(
                    f"{rel}:{line_number} contains singleton marker `{marker}` but matches "
                    f"{len(matches)} registered declarations — every ambient singleton needs its own exact entry"
                )

# ---------------------------------------------------------------------------
# Ambient-reach ratchet: every real (non-comment,
# non-test-oracle) `::instance()` call site, and every
# ambient global()/OnceLock/LazyLock-backed zero-arg accessor from a curated
# marker list, must be named in `[[ambient_reach]]` with an owning issue.
# Two grammars:
#
#   instance-call  -- the literal `::instance()` call syntax.
#   ambient-static -- a curated marker list of known ambient accessor
#                     signatures, distinct from the macro-generated `fn
#                     instance() -> &'static` the singleton net above already
#                     owns: `AssetRegistry::global()`, `global_timer_service()`,
#                     `shared_font_system()`.
#
# This is a RATCHET, not a ban: `instance-call` is meant to shrink to empty
# as later changes retire each remaining singleton (checked bidirectionally
# below -- a registered file with no matching reach left is a stale entry,
# same as an unregistered one is a missing entry); `ambient-static` carries
# named, owner-tracked residuals this migration does not close.
#
# Both grammars are judged on CODE, not prose or test oracles: line comments
# are stripped first, and top-level `#[cfg(test)]`/`#[cfg(all(test, ...))]`-
# gated `mod { ... }` blocks are blanked out by brace depth -- a test that
# calls `Foo::instance()` to prove NON-interference (this repo's divergence-
# check idiom, e.g. `ui_realm.rs`'s hot-reload test asserting
# `AppBinding::instance()` was untouched) is not a production ambient reach.
# ---------------------------------------------------------------------------
# Empty: the macro-defining file this used to exempt is deleted under #553
# along with impl_binding_singleton! itself (see SINGLETON_NET_EXEMPT above).
AMBIENT_REACH_EXEMPT: set[Path] = set()
AMBIENT_STATIC_MARKERS = (
    "fn global() -> &'static",
    "fn global_timer_service() -> &'static",
    "fn shared_font_system(",
)
INSTANCE_CALL_MARKER = "::instance()"
VALID_AMBIENT_GRAMMARS = {"instance-call", "ambient-static"}

_TEST_CFG_RE = re.compile(r"^\s*#\[cfg\(\s*(test\b|all\(\s*test\b)")
_TEST_MOD_RE = re.compile(r"^\s*(pub(\(crate\))?\s+)?mod\s+\w+\s*\{")


def _mask_rust_literals(text: str) -> str:
    """Replace the contents of every string/char literal with spaces,
    preserving every newline and the total character layout (so line
    numbers and column positions stay meaningful downstream).

    This MUST run before both `_strip_line_comments` (a naive
    `split("//", 1)`) and `_strip_top_level_test_modules` (a naive
    `{`/`}` count): neither knows about literals, so without this pass a
    same-line string like `"https://example.com"` truncates everything
    after it on that line -- including a real `::instance()` call sitting
    right after the string -- and a string containing an unbalanced brace
    (`"foo{bar"`) desyncs the brace-depth counter clean through to EOF,
    blanking every later production reach in the file as a false negative.
    Both are real, not hypothetical: this function exists because a
    fixture exercising exactly these two shapes is red without it.

    Handles: normal string literals with backslash escapes; char literals
    (disambiguated from lifetimes -- `'a'` is masked, `'a` is left alone);
    and raw (optionally byte-prefixed) strings `r"..."`/`br"..."` with any
    number of `#` delimiters, including ones that span multiple lines.
    Deliberately does not understand `/* */` block comments -- pre-existing
    scope of this checker, unrelated to the literal-masking hazard fixed
    here.
    """
    out: list[str] = []
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]

        # Raw / byte-raw string: (b)?r(#*)"..."(#*matching), any # depth.
        if ch in ("r", "b"):
            j = i
            if text[j] == "b" and j + 1 < n and text[j + 1] == "r":
                j += 1
            if j < n and text[j] == "r":
                k = j + 1
                hashes = 0
                while k < n and text[k] == "#":
                    hashes += 1
                    k += 1
                if k < n and text[k] == '"':
                    prefix_end = k + 1
                    out.append(text[i:prefix_end])
                    closer = '"' + "#" * hashes
                    end = text.find(closer, prefix_end)
                    body_end = end if end != -1 else n
                    body = text[prefix_end:body_end]
                    out.append("".join("\n" if c == "\n" else " " for c in body))
                    if end != -1:
                        out.append(closer)
                        i = end + len(closer)
                    else:
                        i = n
                    continue

        # Char literal ('c' or an escape like '\n'/'\u{1234}') vs. a
        # lifetime ('a, 'static, ...), which has no closing quote at all.
        if ch == "'":
            if i + 2 < n and text[i + 1] == "\\":
                k = i + 2
                limit = min(n, i + 2 + 12)  # bounds \u{10FFFF}'
                while k < limit and text[k] != "'":
                    k += 1
                if k < limit and text[k] == "'":
                    out.append("'" + " " * (k - (i + 1)) + "'")
                    i = k + 1
                    continue
            elif i + 2 < n and text[i + 2] == "'":
                out.append("' '")
                i += 3
                continue
            out.append(ch)
            i += 1
            continue

        if ch == '"':
            out.append('"')
            i += 1
            while i < n:
                c = text[i]
                if c == "\\" and i + 1 < n:
                    out.append("\n" if text[i + 1] == "\n" else " ")
                    out.append(" ")
                    i += 2
                    continue
                if c == '"':
                    out.append('"')
                    i += 1
                    break
                out.append("\n" if c == "\n" else " ")
                i += 1
            continue

        out.append(ch)
        i += 1
    return "".join(out)


def _strip_line_comments(text: str) -> str:
    return "\n".join(line.split("//", 1)[0] for line in text.splitlines())


def _strip_top_level_test_modules(text: str) -> str:
    """Blank every `#[cfg(test)]`/`#[cfg(all(test, ...))]`-gated `mod { ... }`
    block (brace-depth tracked, same technique as
    check-frame-capability-scope.sh) so the ratchet judges production
    reachability, not a test module that legitimately still calls a
    singleton as its own oracle.
    """
    lines = text.split("\n")
    out = list(lines)
    i = 0
    n = len(lines)
    while i < n:
        if _TEST_CFG_RE.match(lines[i]):
            j = i + 1
            while j < n and not lines[j].strip():
                j += 1
            if j < n and _TEST_MOD_RE.match(lines[j]):
                depth = 0
                seen = False
                k = j
                while k < n:
                    opens = lines[k].count("{")
                    closes = lines[k].count("}")
                    if opens:
                        seen = True
                    depth += opens - closes
                    k += 1
                    if seen and depth <= 0:
                        break
                for idx in range(i, k):
                    out[idx] = ""
                i = k
                continue
        i += 1
    return "\n".join(out)


def _ambient_reach_code(path: Path) -> str | None:
    try:
        raw = path.read_text()
    except (OSError, UnicodeDecodeError):
        return None
    # Literal masking MUST run first -- see `_mask_rust_literals`'s own
    # docstring for the two failure-open shapes this ordering closes.
    return _strip_top_level_test_modules(_strip_line_comments(_mask_rust_literals(raw)))


ambient_reach_files: dict[str, set[Path]] = {"instance-call": set(), "ambient-static": set()}
for index, entry in enumerate(table_array("ambient_reach")):
    label = f"{registry_rel} ambient_reach[{index}]"
    file_value = entry.get("file")
    grammar = entry.get("grammar")
    if not isinstance(file_value, str) or not file_value:
        fail(f"{label} has no file path")
        continue
    label = f"{registry_rel} ambient_reach `{file_value}` (grammar {grammar!r})"
    if grammar not in VALID_AMBIENT_GRAMMARS:
        fail(f"{label}: expected grammar one of {sorted(VALID_AMBIENT_GRAMMARS)}")
        continue
    path = (root / file_value).resolve()
    if not path.is_relative_to(root) or not path.is_file():
        fail(f"{label} names a missing file")
        continue
    if not str(entry.get("note", "")).strip():
        fail(f"{label} has no `note` explaining the residual")
    check_owner_issue(entry, label, required=True)
    rel = Path(file_value)
    if rel in ambient_reach_files[grammar]:
        fail(f"{label} is declared more than once for this grammar")
    ambient_reach_files[grammar].add(rel)

for crate_dir in sorted((root / "crates").iterdir()):
    if not crate_dir.is_dir():
        continue
    crate_src = crate_dir / "src"
    if not crate_src.is_dir():
        continue
    for rs_file in sorted(crate_src.rglob("*.rs")):
        rel = rs_file.relative_to(root)
        if rel in AMBIENT_REACH_EXEMPT:
            continue
        code = _ambient_reach_code(rs_file)
        if code is None:
            continue
        if INSTANCE_CALL_MARKER in code and rel not in ambient_reach_files["instance-call"]:
            fail(
                f"{rel} contains a production `::instance()` call site with no "
                "registered `[[ambient_reach]]` entry (grammar \"instance-call\") -- "
                "every ambient singleton reach must be named with an owning issue"
            )
        if any(marker in code for marker in AMBIENT_STATIC_MARKERS) and rel not in ambient_reach_files["ambient-static"]:
            fail(
                f"{rel} contains an ambient global()/OnceLock/LazyLock-backed accessor "
                "with no registered `[[ambient_reach]]` entry (grammar \"ambient-static\") "
                "-- every named ambient residual must carry an owning issue"
            )

# Bidirectional: a registered entry whose file no longer contains a matching
# reach is stale -- the ratchet's "shrinks to empty" claim only holds if
# retiring the last call site in a file also requires removing its entry.
for grammar, files in ambient_reach_files.items():
    marker_check = (
        (lambda code: INSTANCE_CALL_MARKER in code)
        if grammar == "instance-call"
        else (lambda code: any(marker in code for marker in AMBIENT_STATIC_MARKERS))
    )
    for rel in sorted(files):
        path = root / rel
        if not path.is_file():
            continue  # already reported missing above
        code = _ambient_reach_code(path)
        if code is not None and not marker_check(code):
            fail(
                f"{registry_rel} ambient_reach `{rel}` (grammar {grammar!r}) is registered "
                "but the file no longer contains a matching reach -- remove the stale entry"
            )

# ---------------------------------------------------------------------------
# Public lock-shaped surface net: every PORT-CHECK-OK-SP6 marker in the
# runtime crates must come from a registered file. (Trigger #12 in
# scripts/port-check.sh forces the marker onto any public lock surface its
# grammar can see; this gate forces the marker's file into the registry.)
# ---------------------------------------------------------------------------
lock_exemptions: dict[Path, list[str]] = {}
for index, entry in enumerate(table_array("lock_exemption")):
    label = f"{registry_rel} lock_exemption[{index}] `{entry.get('file', '?')}`"
    file_value = entry.get("file")
    if not isinstance(file_value, str) or not (root / file_value).is_file():
        fail(f"{label} names a missing file")
        continue
    if not str(entry.get("reason", "")).strip():
        fail(f"{label} has no reason")
    check_owner_issue(entry, label, required=True)
    markers = entry.get("markers")
    if not isinstance(markers, list) or not markers or not all(isinstance(marker, str) and marker for marker in markers):
        fail(f"{label} must declare a non-empty string list `markers`")
        continue
    lock_exemptions.setdefault(Path(file_value), []).extend(markers)

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
        expected = lock_exemptions.get(rel, [])
        for marker in expected:
            count = text.count(marker)
            if count != 1:
                fail(f"{rel} must contain registered lock marker {marker!r} exactly once; found {count}")
        for line_number, line in enumerate(text.splitlines(), start=1):
            if "PORT-CHECK-OK-SP6" not in line:
                continue
            matches = [marker for marker in expected if marker in line]
            if len(matches) != 1:
                fail(
                    f"{rel}:{line_number} carries a lock exemption but matches {len(matches)} registered markers — "
                    "every public lock-shaped declaration needs its own exact entry"
                )

# ---------------------------------------------------------------------------
# Process-global guards: existence-pinned so retirement updates the registry.
# ---------------------------------------------------------------------------
for index, entry in enumerate(table_array("process_global_guard")):
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

for index, entry in enumerate(table_array("forbidden_pattern")):
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

for index, entry in enumerate(table_array("forbidden_path")):
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
contract_count = len(contract_keys)
by_state = ", ".join(f"{state} {contracts_by_state[state]}" for state in sorted(contracts_by_state))
print(
    f"runtime-conformance: {contract_count} contracts ({by_state}); "
    f"{surface_count} classified surfaces; {len(config_fields)} config fields; "
    f"{sum(map(len, singleton_exemptions.values()))} singleton + "
    f"{sum(map(len, lock_exemptions.values()))} lock declarations verified"
)
PY
