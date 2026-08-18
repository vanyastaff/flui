//! Executable Flutter-parity inventory for the `tests/parity/` suite.
//!
//! The inventory itself is `tests/parity/manifest.toml`: one row per Rust
//! parity module (a *target*), one row per Flutter oracle test file at the
//! pinned tag, the `#[ignore]`d divergence pins with their owning issues, and
//! derived summary counts. This test target validates the manifest so its
//! claims cannot drift from the tree — the same mechanical-guard philosophy as
//! `flui-objects`' render-object harness catalog and the runtime-contract
//! registry (`docs/runtime-contract.toml`).
//!
//! ## What is always checked (CI, no reference checkout needed)
//!
//! - the file set under `tests/parity/` is exactly `infrastructure` plus one
//!   `<rust>.rs` per target, and every target is compiled (`mod <rust>;` in
//!   `tests/parity/main.rs`) — a manifest row without a test target, or a
//!   parity file without a manifest row, fails;
//! - each target's `#[test]` count matches its `tests` field;
//! - each `#[ignore]` attribute corresponds 1:1 to a manifest pin carrying an
//!   owning issue and a named missing capability — an unowned or stale pin
//!   fails;
//! - every claimed oracle case is cited (backtick-quoted) in the claiming
//!   file's doc comments — the manifest cannot claim a case the code does not
//!   cite;
//! - diverged cases (those evidenced only by an `#[ignore]`d pin) live in the
//!   pin's `cases` and are counted in a separate `diverged` bucket — never in
//!   `claimed`, and a file with any diverged case can never read as `ported`;
//!   an ignored test citing a case the target claims fails outright;
//! - no two targets claim the same (oracle, case) unless the upstream file
//!   genuinely contains that name more than once and the pair is declared
//!   under `[[shared_cases]]`;
//! - per-oracle `claimed`/`status` and the whole `[summary]` block recompute
//!   exactly — `ROADMAP.md` cites these numbers, so they cannot go stale.
//!
//! ## What is additionally checked when `.flutter/` is present at the tag
//!
//! When the gitignored reference clone exists at the repo root **and**
//! `git describe --tags` matches `source.tag`, the inventory is validated
//! against the reference itself: every oracle file exists, its case count
//! matches, every claimed case name resolves to a real `testWidgets(`/`test(`
//! name, `[[shared_cases]]` occurrence counts are exact, and every
//! `*_test.dart` in the universe directory has a manifest row (a new upstream
//! file cannot be silently missed). With no reference — or a reference at a
//! different tag — these checks skip with a notice: validating against the
//! wrong Flutter version would be worse than skipping. Restore the pinned
//! reference with:
//!
//! ```text
//! git clone --depth 1 --branch 3.44.0 --filter=blob:none --sparse \
//!     https://github.com/flutter/flutter.git .flutter
//! cd .flutter && git sparse-checkout set packages/flutter/lib packages/flutter/test
//! ```
//!
//! ## Refreshing the pinned tag
//!
//! Bump `source.tag`, re-clone `.flutter` at the new tag, run this test, and
//! re-classify whatever it rejects (changed case counts, new universe files,
//! renamed cases). The tag lives in one place so an upstream refresh is a
//! deliberate, reviewable act.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

// ─────────────────────────────────────────────────────────────────────────────
// Manifest schema
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    source: Source,
    infrastructure: Vec<String>,
    #[serde(default)]
    shared_cases: Vec<SharedCase>,
    targets: Vec<Target>,
    oracle_files: BTreeMap<String, OracleFile>,
    summary: Summary,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    #[allow(dead_code, reason = "documents the upstream repo; not checked")]
    repo: String,
    tag: String,
    test_root: String,
    universe: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedCase {
    oracle: String,
    case: String,
    occurrences: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    rust: String,
    family: String,
    oracles: Vec<String>,
    tests: usize,
    #[serde(default)]
    claims: Vec<Claim>,
    #[serde(default)]
    pins: Vec<Pin>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    oracle: String,
    cases: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    test: String,
    capability: String,
    issue: u64,
    /// The oracle file whose case(s) this pin diverges from. Required when
    /// `cases` is non-empty.
    oracle: Option<String>,
    /// Oracle case names whose upstream behavior FLUI currently diverges
    /// from, evidenced by this `#[ignore]`d pin. A diverged case is counted
    /// in the `diverged` bucket, never in `claimed` — a divergence must not
    /// inflate the parity number.
    #[serde(default)]
    cases: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleFile {
    cases: usize,
    claimed: usize,
    /// Cases with an `#[ignore]`d divergence pin — tracked separately from
    /// `claimed` so a divergence can never read as parity.
    #[serde(default)]
    diverged: usize,
    status: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    targets: usize,
    rust_tests: usize,
    pins: usize,
    oracle_files: usize,
    oracle_cases: usize,
    cases_claimed: usize,
    cases_diverged: usize,
    universe_files: usize,
    universe_cases: usize,
    universe_cases_claimed: usize,
    universe_cases_diverged: usize,
    universe_pending_files: usize,
    families: BTreeMap<String, FamilySummary>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FamilySummary {
    targets: usize,
    rust_tests: usize,
    cases_claimed: usize,
    cases_diverged: usize,
    pins: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Paths + loading
// ─────────────────────────────────────────────────────────────────────────────

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn parity_dir() -> PathBuf {
    crate_root().join("tests/parity")
}

fn flutter_root() -> PathBuf {
    crate_root().join("../../.flutter")
}

fn load_manifest() -> Manifest {
    let path = parity_dir().join("manifest.toml");
    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let manifest: Manifest =
        toml::from_str(&text).unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()));
    assert_eq!(manifest.schema, 1, "unknown manifest schema version");
    manifest
}

// ─────────────────────────────────────────────────────────────────────────────
// Rust-side scanning (tests/parity/*.rs)
// ─────────────────────────────────────────────────────────────────────────────

struct RustTest {
    name: String,
    ignored: bool,
    /// This test's own `///` doc comment, line-trimmed and joined with single
    /// spaces.
    doc: String,
}

struct RustFile {
    tests: Vec<RustTest>,
    /// All `///` doc-comment content in the file, line-trimmed and joined with
    /// single spaces — the text pool oracle-case citations are checked in.
    ///
    /// Deliberately `///` only, never `//!`: module docs enumerate
    /// out-of-scope and blocked cases with the same backtick-quoted names, so
    /// admitting them would let a case the file explicitly declines to port
    /// satisfy the citation check. A claim must be evidenced by an item doc
    /// comment sitting on actual test code.
    doc_joined: String,
}

/// Line-based scan for `#[test]` functions, `#[ignore]` attributes, and doc
/// comments. Deliberately mirrors the extraction that bootstrapped the
/// manifest; parity test files are plain enough (no macro-generated tests)
/// that a token-level parser is not needed.
fn scan_rust_file(path: &Path) -> RustFile {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut tests = Vec::new();
    let mut docs: Vec<String> = Vec::new();
    let mut pending_test = false;
    let mut pending_ignore = false;
    // >0 while inside a multi-line attribute: net unclosed `[` since its `#[`.
    let mut attr_depth: i32 = 0;
    let mut in_ignore_attr = false;
    let mut in_attr_string = false;
    // doc lines of the item currently being accumulated (reset on any code line)
    let mut current_doc: Vec<String> = Vec::new();
    for line in text.lines() {
        let s = line.trim();
        if let Some(doc) = s.strip_prefix("///") {
            let doc = doc.trim().to_owned();
            docs.push(doc.clone());
            current_doc.push(doc);
            continue;
        }
        if attr_depth > 0 {
            attr_depth += bracket_delta(s, &mut in_attr_string);
            if attr_depth == 0 && in_ignore_attr {
                pending_ignore = true;
                in_ignore_attr = false;
            }
            continue;
        }
        if s.starts_with("#[") {
            in_attr_string = false;
            let depth = bracket_delta(s, &mut in_attr_string);
            let is_test = s.starts_with("#[test]");
            let is_ignore = s.starts_with("#[ignore");
            if depth > 0 {
                attr_depth = depth;
                in_ignore_attr = is_ignore;
            } else if is_test {
                pending_test = true;
            } else if is_ignore {
                pending_ignore = true;
            }
            // other closed attributes between #[test] and fn keep state
            continue;
        }
        if let Some(rest) = s.strip_prefix("fn ").or_else(|| s.strip_prefix("pub fn ")) {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if pending_test {
                tests.push(RustTest {
                    name,
                    ignored: pending_ignore,
                    doc: current_doc.join(" "),
                });
            }
            current_doc.clear();
            pending_test = false;
            pending_ignore = false;
        } else if !s.is_empty() && !s.starts_with("//") {
            // a non-attribute, non-comment, non-fn line resets attribute state
            current_doc.clear();
            pending_test = false;
            pending_ignore = false;
        }
    }
    RustFile {
        tests,
        doc_joined: docs.join(" "),
    }
}

/// Net `[` minus `]` on one line, ignoring brackets inside double-quoted
/// string literals; `in_string` carries string state across the lines of a
/// multi-line attribute.
fn bracket_delta(s: &str, in_string: &mut bool) -> i32 {
    let mut delta = 0;
    let mut escaped = false;
    for c in s.chars() {
        if *in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                *in_string = false;
            }
            continue;
        }
        match c {
            '"' => *in_string = true,
            '[' => delta += 1,
            ']' => delta -= 1,
            _ => {}
        }
    }
    delta
}

// ─────────────────────────────────────────────────────────────────────────────
// Dart-side scanning (reference-gated)
// ─────────────────────────────────────────────────────────────────────────────

/// Extract the ordered first-string-literal names of every `testWidgets(` /
/// `test(` call site in a Dart source. This is the same tokenizer the
/// manifest's `cases` counts were derived with; the count is a drift tripwire
/// against the pinned tag, not a semantic parse of Dart.
fn dart_cases(src: &str) -> Vec<String> {
    let bytes = src.as_bytes();
    let mut names = Vec::new();
    let mut starts: Vec<usize> = Vec::new();
    for needle in ["testWidgets(", "test("] {
        for (i, _) in src.match_indices(needle) {
            let prev = src[..i].chars().next_back();
            let blocked = prev.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '$'));
            if !blocked {
                starts.push(i + needle.len());
            }
        }
    }
    starts.sort_unstable();
    for start in starts {
        if let Some(name) = parse_dart_string(src, bytes, start) {
            names.push(name);
        }
    }
    names
}

/// Parse the first Dart string literal at or after byte offset `i`, skipping
/// whitespace and `//` comments, honoring `r`-prefixes, simple escapes, and
/// adjacent-literal concatenation.
fn parse_dart_string(src: &str, bytes: &[u8], mut i: usize) -> Option<String> {
    let n = bytes.len();
    // skip whitespace and line comments
    loop {
        if i >= n {
            return None;
        }
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'/' if i + 1 < n && bytes[i + 1] == b'/' => {
                i = src[i..].find('\n').map_or(n, |j| i + j + 1);
            }
            _ => break,
        }
    }
    let mut raw = false;
    if bytes[i] == b'r' && i + 1 < n && matches!(bytes[i + 1], b'\'' | b'"') {
        raw = true;
        i += 1;
    }
    if !matches!(bytes[i], b'\'' | b'"') {
        return None;
    }
    let mut parts = String::new();
    while i < n && matches!(bytes[i], b'\'' | b'"') {
        let quote = bytes[i];
        i += 1;
        while i < n {
            let c = bytes[i];
            if c == b'\\' && !raw && i + 1 < n {
                let nxt = bytes[i + 1];
                match nxt {
                    b'n' => parts.push('\n'),
                    b't' => parts.push('\t'),
                    b'\'' => parts.push('\''),
                    b'"' => parts.push('"'),
                    b'\\' => parts.push('\\'),
                    b'$' => parts.push('$'),
                    _ => {
                        parts.push('\\');
                        // push the raw (possibly multi-byte) char
                        let ch = src[i + 1..].chars().next()?;
                        parts.push(ch);
                        i += 1 + ch.len_utf8();
                        continue;
                    }
                }
                i += 2;
                continue;
            }
            if c == quote {
                i += 1;
                break;
            }
            let ch = src[i..].chars().next()?;
            parts.push(ch);
            i += ch.len_utf8();
        }
        // adjacent string-literal concatenation
        let mut j = i;
        loop {
            if j >= n {
                break;
            }
            match bytes[j] {
                b' ' | b'\t' | b'\r' | b'\n' => j += 1,
                b'/' if j + 1 < n && bytes[j + 1] == b'/' => {
                    j = src[j..].find('\n').map_or(n, |k| j + k + 1);
                }
                _ => break,
            }
        }
        raw = false;
        if j < n && bytes[j] == b'r' && j + 1 < n && matches!(bytes[j + 1], b'\'' | b'"') {
            raw = true;
            j += 1;
        }
        if j < n && matches!(bytes[j], b'\'' | b'"') {
            i = j;
        } else {
            break;
        }
    }
    Some(parts)
}

// ─────────────────────────────────────────────────────────────────────────────
// Always-on checks
// ─────────────────────────────────────────────────────────────────────────────

/// The parity directory contains exactly the manifest's infrastructure files
/// plus one `<rust>.rs` per target, and every target is compiled via `mod` in
/// `main.rs`. A missing Rust test target — or an unrepresented parity file —
/// fails here.
#[test]
fn parity_files_and_manifest_rows_correspond_exactly() {
    let manifest = load_manifest();
    let mut expected: BTreeSet<String> = manifest.infrastructure.iter().cloned().collect();
    for t in &manifest.targets {
        let file = format!("{}.rs", t.rust);
        assert!(
            expected.insert(file.clone()),
            "duplicate manifest row or infrastructure entry for {file}"
        );
    }
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(parity_dir()).expect("read tests/parity") {
        let entry = entry.expect("read tests/parity entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        if Path::new(&name).extension().is_some_and(|ext| ext == "rs") {
            actual.insert(name);
        }
    }
    let missing_rows: Vec<_> = actual.difference(&expected).collect();
    let missing_files: Vec<_> = expected.difference(&actual).collect();
    assert!(
        missing_rows.is_empty() && missing_files.is_empty(),
        "tests/parity/ and manifest.toml disagree.\n\
         parity files without a manifest row (classify them): {missing_rows:?}\n\
         manifest rows without a Rust test target: {missing_files:?}"
    );

    let main_rs = fs::read_to_string(parity_dir().join("main.rs")).expect("read parity main.rs");
    // Active `mod x;` declarations only: line-anchored after trimming, so a
    // commented-out `// mod x;` cannot satisfy the check (its whole point is
    // detecting parity files that exist but are not compiled).
    let declared: BTreeSet<&str> = main_rs
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("mod ").and_then(|r| r.strip_suffix(';')))
        .map(str::trim)
        .collect();
    for t in &manifest.targets {
        assert!(
            declared.contains(t.rust.as_str()),
            "target `{}` has no active `mod {};` in tests/parity/main.rs — the file \
             exists but is not compiled, so its tests never run",
            t.rust,
            t.rust
        );
    }
}

/// Per-target facts match the tree: `#[test]` counts, and `#[ignore]` pins
/// exactly matching manifest pins that each carry an owning issue and a named
/// missing capability.
#[test]
fn targets_match_the_tree_and_pins_are_owned() {
    let manifest = load_manifest();
    for t in &manifest.targets {
        let scanned = scan_rust_file(&parity_dir().join(format!("{}.rs", t.rust)));
        assert_eq!(
            scanned.tests.len(),
            t.tests,
            "target `{}`: manifest says {} tests, the file has {} — update the row",
            t.rust,
            t.tests,
            scanned.tests.len()
        );
        assert!(t.tests > 0, "target `{}` has no tests", t.rust);

        let ignored: BTreeSet<&str> = scanned
            .tests
            .iter()
            .filter(|x| x.ignored)
            .map(|x| x.name.as_str())
            .collect();
        let pinned: BTreeSet<&str> = t.pins.iter().map(|p| p.test.as_str()).collect();
        assert_eq!(
            ignored, pinned,
            "target `{}`: #[ignore]d tests and manifest pins disagree. Every ignored \
             parity test needs a pin row with an owning issue; every pin row needs a \
             still-#[ignore]d test (a landed capability means removing BOTH the \
             attribute and the row)",
            t.rust
        );
        let oracle_set: BTreeSet<&str> = t.oracles.iter().map(String::as_str).collect();
        for p in &t.pins {
            assert!(
                p.issue > 0 && !p.capability.trim().is_empty(),
                "target `{}`: pin `{}` must name a missing capability and an owning issue",
                t.rust,
                p.test
            );
            match (&p.oracle, p.cases.is_empty()) {
                (None, true) => {}
                (None, false) => panic!(
                    "target `{}`: pin `{}` lists diverged cases but no `oracle`",
                    t.rust, p.test
                ),
                (Some(oracle), _) => {
                    assert!(
                        oracle_set.contains(oracle.as_str()),
                        "target `{}`: pin `{}` names oracle `{oracle}`, which is not in \
                         the target's `oracles` list",
                        t.rust,
                        p.test
                    );
                    assert!(
                        !p.cases.is_empty(),
                        "target `{}`: pin `{}` names an oracle but no diverged cases",
                        t.rust,
                        p.test
                    );
                    let pin_doc = scanned
                        .tests
                        .iter()
                        .find(|x| x.name == p.test)
                        .map(|x| x.doc.as_str())
                        .unwrap_or_default();
                    for case in &p.cases {
                        let citation = format!("`'{case}'`");
                        assert!(
                            pin_doc.contains(&citation),
                            "target `{}`: pin `{}` records diverged case `{case}` but \
                             the pinned test's own doc comment does not cite it as \
                             {citation} — the divergence claim needs the same citation \
                             evidence as a parity claim",
                            t.rust,
                            p.test
                        );
                    }
                }
            }
        }
    }
}

/// Claims are structurally sound: claimed oracles are declared on the target,
/// no within-target duplicate case names, every claimed case is cited
/// (backtick-quoted) in the claiming file's doc comments, no cross-target
/// duplicate (oracle, case) claim exists without a `[[shared_cases]]`
/// declaration — and no diverged case is counted as claimed. A case whose
/// oracle behavior FLUI diverges from (evidenced by an `#[ignore]`d pin) must
/// live in the pin's `cases`, never in `claims`: a divergence counted as a
/// claim is exactly the "partial reported as parity" failure this inventory
/// exists to kill.
#[test]
fn claims_are_cited_and_unique() {
    let manifest = load_manifest();
    let shared: BTreeMap<(&str, &str), usize> = manifest
        .shared_cases
        .iter()
        .map(|s| ((s.oracle.as_str(), s.case.as_str()), s.occurrences))
        .collect();
    // (oracle, case) → diverged-pin owners, across all targets.
    let mut diverged: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
    for t in &manifest.targets {
        for p in &t.pins {
            if let Some(oracle) = &p.oracle {
                for case in &p.cases {
                    diverged
                        .entry((oracle.as_str(), case.as_str()))
                        .or_default()
                        .push(t.rust.as_str());
                }
            }
        }
    }
    let mut claim_counts: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
    for t in &manifest.targets {
        let oracle_set: BTreeSet<&str> = t.oracles.iter().map(String::as_str).collect();
        let scanned = scan_rust_file(&parity_dir().join(format!("{}.rs", t.rust)));
        // An #[ignore]d test's doc citing a case this target claims means the
        // claim rests (at least partly) on non-passing evidence — the case
        // must be reclassified as diverged via the pin row.
        let claimed_here: BTreeSet<&str> = t
            .claims
            .iter()
            .flat_map(|c| c.cases.iter().map(String::as_str))
            .collect();
        for test in scanned.tests.iter().filter(|x| x.ignored) {
            for case in &claimed_here {
                let citation = format!("`'{case}'`");
                assert!(
                    !test.doc.contains(&citation),
                    "target `{}`: ignored test `{}` cites `{case}`, which the target \
                     also lists under `claims` — a case with a pinned divergence must \
                     be recorded in the pin's `cases` (diverged), not in `claims`",
                    t.rust,
                    test.name
                );
            }
        }
        for claim in &t.claims {
            assert!(
                oracle_set.contains(claim.oracle.as_str()),
                "target `{}` claims cases from `{}`, which is not in its `oracles` list",
                t.rust,
                claim.oracle
            );
            let mut seen = BTreeSet::new();
            for case in &claim.cases {
                assert!(
                    seen.insert(case.as_str()),
                    "target `{}` claims `{case}` twice from `{}`",
                    t.rust,
                    claim.oracle
                );
                let citation = format!("`'{case}'`");
                assert!(
                    scanned.doc_joined.contains(&citation),
                    "target `{}` claims oracle case `{case}` (`{}`) but no doc comment \
                     in the file cites it as {citation} — a claim the code does not \
                     cite is not evidence",
                    t.rust,
                    claim.oracle
                );
                claim_counts
                    .entry((claim.oracle.as_str(), case.as_str()))
                    .or_default()
                    .push(t.rust.as_str());
            }
        }
    }
    // One classification per case name: diverged beats claimed. (Even for a
    // [[shared_cases]] name with two upstream occurrences, a split
    // claimed/diverged classification is not representable — the conservative
    // reading, diverged, wins for the whole name.)
    for ((oracle, case), pinners) in &diverged {
        assert!(
            !claim_counts.contains_key(&(*oracle, *case)),
            "oracle case (`{oracle}`, `{case}`) is recorded as diverged by {pinners:?} \
             but also appears in `claims` — a diverged case must never inflate the \
             claimed/ported number"
        );
        let allowed = shared.get(&(*oracle, *case)).copied().unwrap_or(1);
        assert!(
            pinners.len() <= allowed,
            "oracle case (`{oracle}`, `{case}`) is recorded as diverged by {pinners:?} \
             but the upstream file has only {allowed} occurrence(s) of that name"
        );
    }
    for ((oracle, case), claimants) in &claim_counts {
        let allowed = shared.get(&(*oracle, *case)).copied().unwrap_or(1);
        assert!(
            claimants.len() <= allowed,
            "oracle case (`{oracle}`, `{case}`) is claimed by {claimants:?} but the \
             upstream file has only {allowed} occurrence(s) of that name — a duplicate \
             claim double-counts parity"
        );
    }
    for s in &manifest.shared_cases {
        assert!(
            s.occurrences >= 2,
            "shared_cases entry (`{}`, `{}`) with fewer than 2 occurrences is \
             meaningless — remove it",
            s.oracle,
            s.case
        );
    }
}

/// The per-oracle `claimed`/`status` fields and the whole `[summary]` block
/// recompute exactly from the target rows. ROADMAP.md cites these numbers;
/// drift fails here with the corrected values in the message.
#[test]
fn oracle_rows_and_summary_recompute() {
    let manifest = load_manifest();
    let universe_prefix = format!("{}/", manifest.source.universe);

    // claimed / diverged instances per oracle file
    let mut claimed: BTreeMap<&str, usize> = BTreeMap::new();
    let mut diverged: BTreeMap<&str, usize> = BTreeMap::new();
    for t in &manifest.targets {
        for claim in &t.claims {
            *claimed.entry(claim.oracle.as_str()).or_default() += claim.cases.len();
        }
        for pin in &t.pins {
            if let Some(oracle) = &pin.oracle {
                *diverged.entry(oracle.as_str()).or_default() += pin.cases.len();
            }
        }
        for oracle in &t.oracles {
            assert!(
                manifest.oracle_files.contains_key(oracle),
                "target `{}` references oracle `{oracle}` which has no [oracle_files] row",
                t.rust
            );
        }
    }
    for (oracle, n) in &claimed {
        assert!(
            manifest.oracle_files.contains_key(*oracle),
            "claims reference oracle `{oracle}` which has no [oracle_files] row"
        );
        let row = &manifest.oracle_files[*oracle];
        assert_eq!(
            row.claimed, *n,
            "[oracle_files] `{oracle}`: claimed = {} but targets claim {n}",
            row.claimed
        );
    }
    // files, cases, claimed, diverged
    let mut totals = (0usize, 0usize, 0usize, 0usize);
    // files, cases, claimed, diverged, pending
    let mut uni = (0usize, 0usize, 0usize, 0usize, 0usize);
    for (file, row) in &manifest.oracle_files {
        let n = claimed.get(file.as_str()).copied().unwrap_or(0);
        assert_eq!(
            row.claimed, n,
            "[oracle_files] `{file}`: claimed = {} but targets claim {n}",
            row.claimed
        );
        let d = diverged.get(file.as_str()).copied().unwrap_or(0);
        assert_eq!(
            row.diverged, d,
            "[oracle_files] `{file}`: diverged = {} but pins record {d}",
            row.diverged
        );
        assert!(
            row.claimed + row.diverged <= row.cases,
            "[oracle_files] `{file}`: claimed {} + diverged {} exceeds its {} cases",
            row.claimed,
            row.diverged,
            row.cases
        );
        // `ported` requires every case claimed by passing, cited tests — a
        // file with any diverged case can never read as ported.
        let status = if row.claimed == 0 && row.diverged == 0 {
            "pending"
        } else if row.claimed >= row.cases {
            "ported"
        } else {
            "partial"
        };
        assert_eq!(
            row.status, status,
            "[oracle_files] `{file}`: status should be `{status}` \
             ({} of {} cases claimed, {} diverged)",
            row.claimed, row.cases, row.diverged
        );
        totals.0 += 1;
        totals.1 += row.cases;
        totals.2 += row.claimed;
        totals.3 += row.diverged;
        if file.starts_with(&universe_prefix) {
            uni.0 += 1;
            uni.1 += row.cases;
            uni.2 += row.claimed;
            uni.3 += row.diverged;
            if row.claimed == 0 && row.diverged == 0 {
                uni.4 += 1;
            }
        }
    }

    let s = &manifest.summary;
    let rust_tests: usize = manifest.targets.iter().map(|t| t.tests).sum();
    let pins: usize = manifest.targets.iter().map(|t| t.pins.len()).sum();
    let cases_claimed: usize = claimed.values().sum();
    let expected = [
        ("targets", manifest.targets.len(), s.targets),
        ("rust_tests", rust_tests, s.rust_tests),
        ("pins", pins, s.pins),
        ("oracle_files", totals.0, s.oracle_files),
        ("oracle_cases", totals.1, s.oracle_cases),
        ("cases_claimed", cases_claimed, s.cases_claimed),
        ("cases_diverged", totals.3, s.cases_diverged),
        ("universe_files", uni.0, s.universe_files),
        ("universe_cases", uni.1, s.universe_cases),
        ("universe_cases_claimed", uni.2, s.universe_cases_claimed),
        ("universe_cases_diverged", uni.3, s.universe_cases_diverged),
        ("universe_pending_files", uni.4, s.universe_pending_files),
    ];
    for (name, computed, stored) in expected {
        assert_eq!(
            stored, computed,
            "[summary] {name} = {stored} is stale; recomputed value is {computed}"
        );
    }

    // per-family recompute
    let mut fams: BTreeMap<&str, (usize, usize, usize, usize, usize)> = BTreeMap::new();
    for t in &manifest.targets {
        let e = fams.entry(t.family.as_str()).or_default();
        e.0 += 1;
        e.1 += t.tests;
        e.2 += t.claims.iter().map(|c| c.cases.len()).sum::<usize>();
        e.3 += t.pins.iter().map(|p| p.cases.len()).sum::<usize>();
        e.4 += t.pins.len();
    }
    let stored_fams: BTreeSet<&str> = s.families.keys().map(String::as_str).collect();
    let computed_fams: BTreeSet<&str> = fams.keys().copied().collect();
    assert_eq!(
        stored_fams, computed_fams,
        "[summary.families] keys disagree with target families"
    );
    for (family, (targets, tests, cases, diverged, pins)) in fams {
        let f = &s.families[family];
        assert_eq!(
            (
                f.targets,
                f.rust_tests,
                f.cases_claimed,
                f.cases_diverged,
                f.pins
            ),
            (targets, tests, cases, diverged, pins),
            "[summary.families.\"{family}\"] is stale; recomputed \
             (targets, rust_tests, cases_claimed, cases_diverged, pins) = \
             ({targets}, {tests}, {cases}, {diverged}, {pins})"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Reference-gated checks
// ─────────────────────────────────────────────────────────────────────────────

/// Returns the reference test root if `.flutter/` is present *at the pinned
/// tag*; otherwise prints a skip notice and returns `None`.
fn pinned_reference(manifest: &Manifest) -> Option<PathBuf> {
    let root = flutter_root();
    if !root.is_dir() {
        eprintln!(
            "parity-inventory: .flutter/ reference not present — skipping \
             reference-gated checks (clone it at tag {} to run them)",
            manifest.source.tag
        );
        return None;
    }
    let described = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["describe", "--tags"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    match described {
        Some(tag) if tag == manifest.source.tag => Some(root.join(&manifest.source.test_root)),
        other => {
            eprintln!(
                "parity-inventory: .flutter/ is present but not at pinned tag {} \
                 (git describe --tags says {:?}) — skipping reference-gated checks. \
                 Re-clone with: git clone --depth 1 --branch {} --filter=blob:none \
                 --sparse https://github.com/flutter/flutter.git .flutter",
                manifest.source.tag, other, manifest.source.tag
            );
            None
        }
    }
}

/// Validate the manifest against the pinned Flutter reference: oracle files
/// exist with matching case counts, every claimed case name resolves, shared
/// occurrence counts are exact, and the universe directory is fully
/// classified. Skips (with a notice) when the pinned reference is absent.
#[test]
fn manifest_matches_pinned_reference() {
    let manifest = load_manifest();
    let Some(test_root) = pinned_reference(&manifest) else {
        return;
    };

    // Every oracle row exists at the tag with a matching case count.
    let mut case_lists: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (file, row) in &manifest.oracle_files {
        let path = test_root.join(file);
        let src = fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "[oracle_files] `{file}` does not exist in the pinned reference \
                 ({}): {e}",
                path.display()
            )
        });
        let cases = dart_cases(&src);
        assert_eq!(
            row.cases,
            cases.len(),
            "[oracle_files] `{file}`: manifest says {} cases, the pinned reference \
             has {} — the row is stale",
            row.cases,
            cases.len()
        );
        case_lists.insert(file.as_str(), cases);
    }

    // Every claimed and diverged case name resolves against the reference.
    for t in &manifest.targets {
        for claim in &t.claims {
            let names = &case_lists[claim.oracle.as_str()];
            for case in &claim.cases {
                assert!(
                    names.iter().any(|n| n == case),
                    "target `{}` claims case `{case}` which does not exist in \
                     `{}` at tag {}",
                    t.rust,
                    claim.oracle,
                    manifest.source.tag
                );
            }
        }
        for pin in &t.pins {
            let Some(oracle) = &pin.oracle else { continue };
            let names = &case_lists[oracle.as_str()];
            for case in &pin.cases {
                assert!(
                    names.iter().any(|n| n == case),
                    "target `{}`: pin `{}` records diverged case `{case}` which does \
                     not exist in `{oracle}` at tag {}",
                    t.rust,
                    pin.test,
                    manifest.source.tag
                );
            }
        }
    }
    for s in &manifest.shared_cases {
        let actual = case_lists
            .get(s.oracle.as_str())
            .map_or(0, |names| names.iter().filter(|n| *n == &s.case).count());
        assert_eq!(
            s.occurrences, actual,
            "[[shared_cases]] (`{}`, `{}`): declared {} occurrences, the pinned \
             reference has {actual}",
            s.oracle, s.case, s.occurrences
        );
    }

    // Universe completeness: every *_test.dart in the universe directory has a
    // manifest row (a new upstream file must be classified, not missed).
    let universe_dir = test_root.join(&manifest.source.universe);
    let mut universe_actual = BTreeSet::new();
    for entry in fs::read_dir(&universe_dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", universe_dir.display()))
    {
        let entry = entry.expect("read universe entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with("_test.dart") && entry.path().is_file() {
            universe_actual.insert(format!("{}/{name}", manifest.source.universe));
        }
    }
    let universe_prefix = format!("{}/", manifest.source.universe);
    let universe_rows: BTreeSet<String> = manifest
        .oracle_files
        .keys()
        .filter(|f| f.starts_with(&universe_prefix))
        .cloned()
        .collect();
    let unclassified: Vec<_> = universe_actual.difference(&universe_rows).collect();
    let vanished: Vec<_> = universe_rows.difference(&universe_actual).collect();
    assert!(
        unclassified.is_empty() && vanished.is_empty(),
        "universe drift against the pinned reference.\n\
         upstream files with no manifest row (classify them): {unclassified:?}\n\
         manifest rows whose upstream file is gone: {vanished:?}"
    );
}
