#!/usr/bin/env bash
# Mechanical gate for docs/PANIC-POLICY.md's `expect("BUG: …")` convention.
#
# Every production-path `expect()` call must carry a message prefixed with
# "BUG: " that names the internal invariant it asserts (see PANIC-POLICY.md
# for the caller-triggerable-vs-internal-invariant split this convention
# encodes). Nothing enforced this before this script: `expect_used` is
# deliberately not linted (the message is the review bar, not the call), so
# a bare `.expect("failed to get node")` compiled clean forever.
#
# This is a RATCHET, not a ban, following the same shape as
# scripts/check-runtime-conformance.sh's `ambient_reach` net: a per-file
# allowlist (docs/panic-policy-allowlist.txt) records files that still carry
# non-conforming production expect() sites. It shrinks only:
#
#   * a file with a non-conforming site that is NOT on the allowlist fails
#     (new debt must be declared, not silently introduced);
#   * a file ON the allowlist that no longer has any non-conforming site
#     fails too (a cleaned file must leave the list -- the allowlist is
#     "today's known debt", not a permanent exemption).
#
# Scope: `crates/*/src/**/*.rs`, minus:
#   * a whole module gated `#[cfg(test)]` / `#[cfg(any(test, feature =
#     "testing"))]` / etc. at its `mod NAME;` declaration (test-support
#     libraries like flui-rendering's `testing`/`test_support` modules,
#     compiled only under `cfg(test)` or an opt-in `testing` feature);
#   * an individual `mod { ... }` / `fn(...) { ... }` / `impl ... { ... }`
#     item gated the same way inline (`#[cfg(test)] mod tests { ... }`,
#     `#[cfg(test)] fn for_test() { ... }`, `#[cfg(all(test, ...))] impl
#     ... { ... }`);
#   * doc comments and line comments (never code).
#
# `crates/*/tests/**`, `crates/*/benches/**`, `crates/*/examples/**` are
# integration tests/benches/examples, not `src/`, and are outside this
# script's glob entirely -- they get free `expect()`/`unwrap()` under
# PANIC-POLICY.md's own "Test code, benches, examples" row.
#
# Lexer lineage: the literal-masking and line-comment-stripping passes below
# are ported verbatim from check-runtime-conformance.sh's
# `_mask_rust_literals` / `_strip_line_comments` (masking MUST run first --
# see that script's docstring for the two failure-open shapes this ordering
# closes: a same-line string containing "//", and a string containing an
# unbalanced brace). This script EXTENDS that lexer in one direction that
# script's own `_strip_top_level_test_modules` does not need to cover:
#
#   * `_strip_top_level_test_modules` only recognizes a single-line
#     `#[cfg(test)]` / `#[cfg(all(test, ...))]` immediately above a `mod
#     NAME { ... }` on the very next non-blank line. This workspace also
#     gates individual `fn`/`impl` items (89 and 8 real sites), and gates
#     some `mod`/`fn`/`impl` items with a `#[cfg(all(\n    test,\n    ...\n
#     ))]` attribute spanning multiple physical lines (`runner.rs`'s
#     `realm_dispatch_tests`/`desktop_pacing_tests`, missed by a single-line
#     regex). `find_cfg_test_gated_items` below walks attributes generically
#     (multi-line, stacked, any brace-delimited item kind) and evaluates the
#     cfg predicate with a small `all`/`any`/`not` boolean evaluator instead
#     of a regex, so `all(test, not(target_os = "ios"))` and `any(test,
#     feature = "testing")` are both recognized as test-only regardless of
#     line breaks.
#
# Usage:
#   scripts/check-panic-policy.sh              # scan; exit 1 on violation
#   scripts/check-panic-policy.sh --self-test  # verify the scanner itself
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
cd "${repo_root}"

if ! command -v python3 >/dev/null 2>&1; then
  echo "check-panic-policy: python3 not found on PATH" >&2
  exit 2
fi

mode="${1:-}"

python3 - "${repo_root}" "${mode}" <<'PY'
import re
import sys
from pathlib import Path

root = Path(sys.argv[1]).resolve()
mode = sys.argv[2] if len(sys.argv) > 2 else ""
ALLOWLIST_PATH = root / "docs" / "panic-policy-allowlist.txt"

# =============================================================================
# Lexer: literal masking + comment stripping (ported from
# check-runtime-conformance.sh's _mask_rust_literals / _strip_line_comments).
# =============================================================================


def mask_rust_literals(text: str) -> str:
    """Replace the contents of every string/char literal with spaces,
    preserving every newline and the total character layout, so line numbers
    and column positions -- and byte offsets into the ORIGINAL text -- stay
    valid downstream. See check-runtime-conformance.sh's copy of this
    function for the full rationale; this is a verbatim port."""
    out: list[str] = []
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
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
        if ch == "'":
            if i + 2 < n and text[i + 1] == "\\":
                k = i + 2
                limit = min(n, i + 2 + 12)
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


def strip_line_comments(text: str) -> str:
    # split("\n"), NOT splitlines(): splitlines() drops a trailing empty
    # element for text ending in "\n", which would desync this function's
    # line count against the raw text this checker re-indexes into by line
    # number below.
    return "\n".join(line.split("//", 1)[0] for line in text.split("\n"))


def split_top_level(text: str, sep: str = ",") -> list[str]:
    """Split on `sep` at paren/bracket/brace depth 0."""
    parts: list[str] = []
    depth = 0
    buf: list[str] = []
    for ch in text:
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        if ch == sep and depth == 0:
            parts.append("".join(buf))
            buf = []
        else:
            buf.append(ch)
    parts.append("".join(buf))
    return [p.strip() for p in parts if p.strip()]


def cfg_requires_test(predicate: str) -> bool:
    """Does this cfg(...) predicate only ever hold under `test` or the
    `testing` feature? Handles the grammar actually used in this workspace:
    bare `test`, `all(...)`, `any(...)`, `not(...)`, `feature = "testing"`."""
    expr = predicate.strip()
    m = re.match(r"^(all|any|not)\((.*)\)$", expr, re.DOTALL)
    if m:
        kind, inner = m.group(1), m.group(2)
        children = split_top_level(inner)
        if kind == "all":
            return any(cfg_requires_test(c) for c in children)
        if kind == "any":
            return bool(children) and all(cfg_requires_test(c) for c in children)
        return False  # not(...) never *requires* test
    if expr == "test":
        return True
    if re.match(r'^feature\s*=\s*"testing"$', expr):
        return True
    return False


_ATTR_OPEN_RE = re.compile(r"^\s*#!?\[")
_ITEM_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?"
    r"(?:default\s+)?(?:unsafe\s+)?(?:async\s+)?(?:const\s+)?"
    r"(mod|fn|impl|trait)\b"
)


def _attr_span(lines: list[str], idx: int) -> int:
    """`lines[idx]` opens an attribute (`#[...]`/`#![...]`), possibly
    spanning multiple physical lines (bracket-depth tracked). Returns the
    index one past its last line."""
    depth = lines[idx].count("[") - lines[idx].count("]")
    i = idx + 1
    n = len(lines)
    while depth > 0 and i < n:
        depth += lines[i].count("[") - lines[i].count("]")
        i += 1
    return i


def find_cfg_test_gated_items(lines: list[str]) -> list[tuple[int, int]]:
    """Scan (masked, comment-stripped) `lines` for every brace-delimited item
    (`mod`/`fn`/`impl`/`trait`) gated -- directly, or through a stack of
    attributes -- by a cfg(...) predicate that only holds under test. Returns
    (start_line_idx, end_line_idx_inclusive) spans to blank; start is the
    first `#[cfg...]` line, so a `#[cfg(test)]` with no recognized item after
    it is simply left alone (nothing to blank, not an error: it may gate a
    struct/const/static this lexer does not open, none of which can contain
    an `.expect(` call worth scanning)."""
    spans: list[tuple[int, int]] = []
    n = len(lines)
    i = 0
    while i < n:
        if not _ATTR_OPEN_RE.match(lines[i]):
            i += 1
            continue
        attr_start = i
        requires_test = False
        j = i
        while j < n and _ATTR_OPEN_RE.match(lines[j]):
            end = _attr_span(lines, j)
            text = "\n".join(lines[j:end])
            m = re.match(r"\s*#!?\[\s*cfg\(", text, re.DOTALL)
            if m:
                depth = 1
                k = m.end()
                start_pred = k
                while depth > 0 and k < len(text):
                    if text[k] == "(":
                        depth += 1
                    elif text[k] == ")":
                        depth -= 1
                    k += 1
                predicate = text[start_pred : k - 1]
                if cfg_requires_test(predicate):
                    requires_test = True
            j = end
            while j < n and not lines[j].strip():
                j += 1
        if not requires_test:
            i = attr_start + 1
            continue
        if j >= n or not _ITEM_RE.match(lines[j]):
            i = attr_start + 1
            continue
        # Find the item's opening brace: the first '{' seen once paren depth
        # returns to (or stays at) 0, scanning forward from the item line.
        paren_depth = 0
        brace_line = None
        k = j
        while k < n:
            line = lines[k]
            col = 0
            found = False
            while col < len(line):
                c = line[col]
                if c == "(":
                    paren_depth += 1
                elif c == ")":
                    paren_depth -= 1
                elif c == "{" and paren_depth <= 0:
                    found = True
                    break
                col += 1
            if found:
                brace_line = k
                break
            k += 1
        if brace_line is None:
            i = attr_start + 1
            continue
        depth = 0
        seen = False
        m2 = brace_line
        while m2 < n:
            opens = lines[m2].count("{")
            closes = lines[m2].count("}")
            if opens:
                seen = True
            depth += opens - closes
            m2 += 1
            if seen and depth <= 0:
                break
        spans.append((attr_start, m2 - 1))
        i = m2
    return spans


def strip_cfg_test_gated_items(text: str) -> str:
    lines = text.split("\n")
    out = list(lines)
    for start, end in find_cfg_test_gated_items(lines):
        for idx in range(start, end + 1):
            out[idx] = ""
    return "\n".join(out)


def production_code(raw: str) -> str:
    """The full pipeline: mask literals, strip line comments (safe now that
    literals are masked), blank every cfg(test)-gated item."""
    return strip_cfg_test_gated_items(strip_line_comments(mask_rust_literals(raw)))


# =============================================================================
# Whole-module test-support exclusion: a `mod NAME;` file declaration gated
# by a test-requiring cfg (e.g. flui-rendering's `#[cfg(any(test, feature =
# "testing"))] pub mod testing;`) excludes NAME's entire file/directory from
# the scan, the same way `tests/`/`benches/`/`examples/` are out of scope by
# not being under `src/` at all.
# =============================================================================

_MOD_DECL_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;\s*$")
_ATTR_LINE_RE = re.compile(r"^\s*#!?\[(.*)\]\s*$")
_DOC_RE = re.compile(r"^\s*///.*$|^\s*//!.*$")


def find_test_support_modules(rs_file: Path) -> list[str]:
    try:
        lines = rs_file.read_text().splitlines()
    except (OSError, UnicodeDecodeError):
        return []
    names: list[str] = []
    for idx, line in enumerate(lines):
        m = _MOD_DECL_RE.match(line.split("//", 1)[0])
        if not m:
            continue
        name = m.group(1)
        j = idx - 1
        attrs: list[str] = []
        while j >= 0:
            stripped = lines[j].strip()
            if not stripped:
                j -= 1
                continue
            if _DOC_RE.match(lines[j]):
                j -= 1
                continue
            am = _ATTR_LINE_RE.match(lines[j])
            if am:
                attrs.append(am.group(1))
                j -= 1
                continue
            break
        gated = any(
            cfg_requires_test(a.strip()[4:-1])
            for a in attrs
            if a.strip().startswith("cfg(") and a.strip().endswith(")")
        )
        if gated:
            names.append(name)
    return names


def test_support_paths_for_crate(crate_src: Path) -> set[Path]:
    excluded: set[Path] = set()
    for rs_file in crate_src.rglob("*.rs"):
        names = find_test_support_modules(rs_file)
        if not names:
            continue
        decl_dir = rs_file.parent if rs_file.name in ("lib.rs", "mod.rs", "main.rs") else rs_file.parent / rs_file.stem
        for name in names:
            cand_file = decl_dir / f"{name}.rs"
            cand_dir = decl_dir / name
            if cand_dir.is_dir():
                excluded.add(cand_dir.relative_to(root))
            elif cand_file.is_file():
                excluded.add(cand_file.relative_to(root))
    return excluded


def is_excluded(rel: Path, excluded: set[Path]) -> bool:
    return any(rel == e or e in rel.parents for e in excluded)


def production_files(crates_dir: Path) -> list[Path]:
    files: list[Path] = []
    for crate_dir in sorted(crates_dir.iterdir()):
        src = crate_dir / "src"
        if not src.is_dir():
            continue
        excluded = test_support_paths_for_crate(src)
        for rs in sorted(src.rglob("*.rs")):
            rel = rs.relative_to(root)
            if not is_excluded(rel, excluded):
                files.append(rs)
    return files


# =============================================================================
# expect() site extraction: locate every `.expect(` call surviving the
# production_code() pipeline (i.e. not in a comment, doc, or cfg(test)-gated
# item), then read its message from the ORIGINAL (unmasked) text -- the mask
# pass preserves line/column layout exactly, so a location found in the
# masked text indexes validly into the raw text.
# =============================================================================

EXPECT_RE = re.compile(r"\.expect\(")
_CONST_DEF_TMPL = r"const\s+{name}\s*:[^=]*=\s*"


def extract_string_literal(raw: str, start: int) -> tuple[str, int] | None:
    """`raw[start]` must open a string/raw-string literal. Returns
    (content, end_idx) or None if it doesn't look like one."""
    n = len(raw)
    i = start
    if raw[i] in ("r", "b"):
        j = i
        if raw[j] == "b" and j + 1 < n and raw[j + 1] == "r":
            j += 1
        if j < n and raw[j] == "r":
            k = j + 1
            hashes = 0
            while k < n and raw[k] == "#":
                hashes += 1
                k += 1
            if k < n and raw[k] == '"':
                prefix_end = k + 1
                closer = '"' + "#" * hashes
                end = raw.find(closer, prefix_end)
                if end == -1:
                    return None
                return raw[prefix_end:end], end + len(closer)
        return None
    if raw[i] != '"':
        return None
    i += 1
    buf: list[str] = []
    while i < n:
        c = raw[i]
        if c == "\\" and i + 1 < n:
            buf.append(c)
            buf.append(raw[i + 1])
            i += 2
            continue
        if c == '"':
            return "".join(buf), i + 1
        buf.append(c)
        i += 1
    return None


def nonconforming_sites(rs_file: Path) -> list[tuple[int, str]]:
    """Returns (line_number, message-or-placeholder) for every production
    `.expect(...)` site whose message does not start with `BUG: `."""
    raw = rs_file.read_text()
    prod = production_code(raw)
    prod_lines = prod.split("\n")
    raw_lines = raw.split("\n")
    if len(prod_lines) != len(raw_lines):
        # The pipeline above is line-count-preserving by construction; a
        # mismatch means a bug in this script, not the scanned file.
        raise AssertionError(f"{rs_file}: lexer line-count mismatch ({len(prod_lines)} vs {len(raw_lines)})")

    sites: list[tuple[int, str]] = []
    for li, pline in enumerate(prod_lines):
        for m in EXPECT_RE.finditer(pline):
            col = m.end()
            content: str | None = None
            search_line = li
            search_col = col
            hops = 0
            while hops < 20:
                cur_raw = raw_lines[search_line]
                k = search_col
                while k < len(cur_raw) and cur_raw[k] in " \t":
                    k += 1
                if k < len(cur_raw):
                    ch = cur_raw[k]
                    if ch == '"' or (ch in ("r", "b") and k + 1 < len(cur_raw)):
                        joined = "\n".join(raw_lines[search_line : search_line + 25])
                        res = extract_string_literal(joined, k)
                        if res:
                            content = res[0]
                    else:
                        rest = cur_raw[k:]
                        idm = re.match(r"[A-Za-z_][A-Za-z0-9_:]*", rest)
                        if idm:
                            simple_name = idm.group(0).split("::")[-1]
                            cm = re.search(_CONST_DEF_TMPL.format(name=re.escape(simple_name)), raw)
                            if cm:
                                res = extract_string_literal(raw, cm.end())
                                if res:
                                    content = res[0]
                    break
                search_line += 1
                search_col = 0
                hops += 1
            if content is None or not content.startswith("BUG: "):
                sites.append((li + 1, content if content is not None else "<dynamic, non-literal argument>"))
    return sites


# =============================================================================
# Allowlist I/O: one relative path per line, `#`-comments and blank lines
# ignored.
# =============================================================================


def load_allowlist(path: Path) -> set[str]:
    if not path.is_file():
        return set()
    entries: set[str] = set()
    for line in path.read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            entries.add(line)
    return entries


def run_check(crates_dir: Path, allowlist: set[str]) -> tuple[list[str], dict[str, int]]:
    """Returns (errors, nonconforming_counts_by_relpath)."""
    errors: list[str] = []
    counts: dict[str, int] = {}
    for rs_file in production_files(crates_dir):
        rel = str(rs_file.relative_to(root))
        sites = nonconforming_sites(rs_file)
        if sites:
            counts[rel] = len(sites)
    unlisted = sorted(set(counts) - allowlist)
    stale = sorted(allowlist - set(counts))
    for rel in unlisted:
        sites = nonconforming_sites(root / rel)
        preview = "; ".join(f"{n}: {msg[:60]!r}" for n, msg in sites[:3])
        more = f" (+{len(sites) - 3} more)" if len(sites) > 3 else ""
        errors.append(
            f"{rel} has {len(sites)} production expect() site(s) without the `BUG: ` prefix "
            f"and is not on docs/panic-policy-allowlist.txt: {preview}{more}"
        )
    for rel in stale:
        errors.append(
            f"{rel} is on docs/panic-policy-allowlist.txt but no longer has any non-conforming "
            "expect() site -- remove its entry (the allowlist is a shrink-only ratchet)"
        )
    return errors, counts


# =============================================================================
# Self-test: fixtures exercise both ratchet directions plus the lexer edge
# cases (masking-before-comment-stripping, cfg(test) fn/impl/mod exclusion,
# multi-line cfg attributes, const-resolved identifier arguments).
# =============================================================================


def self_test() -> int:
    global root  # noqa: PLW0603 -- self-test only, restored before returning
    import shutil
    import tempfile

    fixtures = root / "scripts" / "fixtures" / "panic-policy"
    status = 0

    def check(name: str, expect_sites: int) -> None:
        nonlocal status
        path = fixtures / name
        sites = nonconforming_sites(path)
        if len(sites) == expect_sites:
            print(f"  ok: {name} -> {len(sites)} non-conforming site(s) as expected")
        else:
            print(
                f"  FAIL: {name} -> expected {expect_sites} non-conforming site(s), got {len(sites)}: {sites}"
            )
            status = 1

    print("self-test: lexer correctness (fixture files)")
    check("conforming.rs.fixture", 0)
    check("unlisted_violation.rs.fixture", 1)
    check("stale_allowlist_candidate.rs.fixture", 0)

    # Exercise the REAL run_check() pipeline end to end (not a hand-rolled
    # re-check of the same set arithmetic): copy the fixtures into a
    # throwaway `<tmp>/crates/fixture_crate/src/` so `production_files()`
    # walks them exactly as it would a real crate, then run the gate against
    # two synthetic allowlists that plant one violation in each direction.
    with tempfile.TemporaryDirectory() as tmp:
        tmp_root = Path(tmp)
        crate_src = tmp_root / "crates" / "fixture_crate" / "src"
        crate_src.mkdir(parents=True)
        for f in fixtures.glob("*.rs.fixture"):
            shutil.copy(f, crate_src / f.name.replace(".fixture", ""))

        real_root = root
        root = tmp_root
        try:
            rel_prefix = "crates/fixture_crate/src"

            print("self-test: ratchet direction 1 -- unlisted violation must fail")
            errors, _ = run_check(crate_src.parent.parent, {f"{rel_prefix}/stale_allowlist_candidate.rs"})
            if any("unlisted_violation.rs" in e and "not on" in e for e in errors):
                print("  ok: an unlisted file with a bare expect() is flagged")
            else:
                print(f"  FAIL: expected the unlisted violation to be flagged; errors were: {errors}")
                status = 1

            print("self-test: ratchet direction 2 -- cleaned file left on the allowlist must fail")
            errors, _ = run_check(
                crate_src.parent.parent,
                {f"{rel_prefix}/unlisted_violation.rs", f"{rel_prefix}/stale_allowlist_candidate.rs"},
            )
            if any("stale_allowlist_candidate.rs" in e and "stale" in e.lower() for e in errors):
                print("  ok: a listed-but-now-clean file is flagged as stale")
            else:
                print(f"  FAIL: expected the stale allowlist entry to be flagged; errors were: {errors}")
                status = 1

            print("self-test: fully reconciled allowlist passes clean")
            errors, _ = run_check(
                crate_src.parent.parent,
                {f"{rel_prefix}/unlisted_violation.rs"},
            )
            if not errors:
                print("  ok: no violations when the allowlist matches reality exactly")
            else:
                print(f"  FAIL: expected zero errors, got: {errors}")
                status = 1
        finally:
            root = real_root

    return status


if mode == "--self-test":
    sys.exit(self_test())

allowlist = load_allowlist(ALLOWLIST_PATH)
errors, counts = run_check(root / "crates", allowlist)

if errors:
    print("check-panic-policy: violations detected", file=sys.stderr)
    for error in errors:
        print(f"  - {error}", file=sys.stderr)
    sys.exit(1)

total_sites = sum(counts.values())
print(
    f"check-panic-policy: {len(counts)} file(s) on the allowlist, "
    f"{total_sites} tracked non-conforming expect() site(s); no unlisted or stale entries"
)
PY
