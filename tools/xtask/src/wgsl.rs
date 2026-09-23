//! WGSL derivatives stay in uniform control flow.
//!
//! WebGPU's uniformity analysis (Tint, in every browser) admits `dpdx`/`dpdy`/
//! `fwidth` and their coarse/fine variants — and the implicit-derivative
//! texture samplers `textureSample`/`textureSampleBias`/`textureSampleCompare`
//! — only in uniform control flow. Native wgpu goes through naga, whose
//! analysis is weaker: every clip-capable pipeline once compiled natively and
//! failed module creation in the browser because `sdfToAlpha` (which takes
//! derivatives) was called under an `if` on per-instance clip data, and the arc
//! shader took its angular gradient after a per-instance early `return`.
//! naga's validator with every flag on accepted both, so there is no host-side
//! oracle for this class; this check is the structural stand-in.
//!
//! The rule is syntactic and deliberately conservative: inside every function,
//! a call to a derivative builtin — or to any function that transitively calls
//! one — must not sit inside an `if`, `else`, `switch`, `loop`, `for` or
//! `while` block, and must not follow a `return` inside such a block earlier in
//! the same function. A branch on a genuinely uniform value is legal for Tint
//! but refused here unless a comment on the branch keyword's line, or on a line
//! just before it, carries the marker `wgsl-uniformity: uniform` and says why
//! the condition is uniform (a uniform-buffer field, a constant): that block,
//! and an `else` chained to it, are then walked as straight-line code. A marker
//! arms exactly the next branch keyword after it. Everything else: compute the
//! derivative unconditionally and `select` afterwards.
//!
//! The walk is over tokens, not lines, so layout cannot hide a branch: `} else
//! {` on the closing line of an `if`, a one-line `if c { return x; }`, a `for`
//! header split across lines and a call split from its argument list are all
//! seen for what they are.
//!
//! Exit 1 with one line per finding; 0 when clean. `--self-test` runs the
//! walker over the embedded fixture — every layout a line-anchored walk
//! misses — and exits 0 only when each expected finding, and no other, is
//! reported.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::LazyLock;

use anyhow::{Context, bail};
use regex::Regex;

use crate::util::repo_root;

const DEFAULT_ROOT: &str = "crates/flui-engine/src/shaders";

const DERIVATIVES: [&str; 12] = [
    "dpdx",
    "dpdy",
    "fwidth",
    "dpdxCoarse",
    "dpdyCoarse",
    "fwidthCoarse",
    "dpdxFine",
    "dpdyFine",
    "fwidthFine",
    "textureSample",
    "textureSampleBias",
    "textureSampleCompare",
];
const BRANCH_KEYWORDS: [&str; 6] = ["if", "else", "switch", "loop", "for", "while"];
const UNIFORM_MARKER: &str = "wgsl-uniformity: uniform";

/// Identifiers, and every single punctuation character that matters to the
/// walk; everything else (numbers, operators, commas) is skipped.
static TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z_]\w*|[{}();]").expect("BUG: static regex"));
static BLOCK_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").expect("BUG: static regex"));

/// Arguments for `cargo xtask wgsl`.
#[derive(Debug, clap::Args)]
pub(crate) struct WgslArgs {
    /// Run the walker over the embedded fixture instead of the shaders.
    #[arg(long, conflicts_with = "root")]
    self_test: bool,
    /// Directory searched recursively for `*.wgsl` (default: crates/flui-engine/src/shaders).
    root: Option<PathBuf>,
}

/// `cargo xtask wgsl`: check WGSL derivative uniformity.
pub(crate) fn wgsl(args: &WgslArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    let (root, shown) = match &args.root {
        Some(root) => (root.clone(), root.display().to_string()),
        None => (repo_root().join(DEFAULT_ROOT), DEFAULT_ROOT.to_owned()),
    };
    if !root.is_dir() {
        bail!("{shown} is not a directory");
    }
    let texts = read_shaders(&root, &shown)?;
    let (findings, derived) = analyse(&texts);
    for finding in &findings {
        println!("{finding}");
    }
    if !findings.is_empty() {
        eprintln!(
            "wgsl-uniformity: {} derivative(s) outside uniform control flow",
            findings.len()
        );
        return Ok(ExitCode::FAILURE);
    }
    println!(
        "wgsl-uniformity: {} shader files, {derived} derivative-taking functions, all in uniform control flow",
        texts.len()
    );
    Ok(ExitCode::SUCCESS)
}

/// Every `*.wgsl` under `root` as `(displayed path, source)`, in path order.
fn read_shaders(root: &Path, shown: &str) -> anyhow::Result<Vec<(String, String)>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type().is_file() || path.extension().is_none_or(|ext| ext != "wgsl") {
            continue;
        }
        let parts: Vec<String> = path
            .strip_prefix(root)
            .expect("BUG: walkdir yields paths under its root")
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        files.push((parts, text));
    }
    files.sort();
    Ok(files
        .into_iter()
        .map(|(parts, text)| (format!("{shown}/{}", parts.join("/")), text))
        .collect())
}

/// One lexical token, the 1-based line it sits on, and whether a uniform
/// marker armed it (only ever set on a branch keyword).
#[derive(Debug, Clone)]
struct Token<'a> {
    line: usize,
    text: &'a str,
    armed: bool,
}

/// Why a derivative-taking call is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    InsideBranch,
    AfterEarlyReturn,
}

impl Kind {
    fn describe(self) -> &'static str {
        match self {
            Kind::InsideBranch => "inside a branch",
            Kind::AfterEarlyReturn => "after an early return",
        }
    }
}

/// A derivative-taking call outside uniform control flow.
#[derive(Debug)]
struct Finding {
    path: String,
    line: usize,
    callee: String,
    function: String,
    kind: Kind,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: `{}` takes a derivative {} in `{}`",
            self.path,
            self.line,
            self.callee,
            self.kind.describe(),
            self.function
        )
    }
}

/// Blanks comments without moving anything: a block comment keeps its
/// newlines so every later token stays on its own line — the marker lines are
/// numbered against the raw text.
fn strip_comments(text: &str) -> String {
    let blanked = BLOCK_COMMENT.replace_all(text, |caps: &regex::Captures<'_>| {
        caps[0]
            .chars()
            .map(|c| if c == '\n' { '\n' } else { ' ' })
            .collect::<String>()
    });
    blanked
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Line numbers whose comment carries the uniform-branch marker.
fn uniform_marked_lines(text: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.contains(UNIFORM_MARKER))
        .map(|(index, _)| index + 1)
        .collect()
}

/// Every token of the comment-stripped text, unarmed.
fn tokenize(text: &str) -> Vec<Token<'_>> {
    text.lines()
        .enumerate()
        .flat_map(|(index, line)| {
            TOKEN.find_iter(line).map(move |m| Token {
                line: index + 1,
                text: m.as_str(),
                armed: false,
            })
        })
        .collect()
}

/// A marker arms the NEXT branch keyword at or after its line, and nothing
/// else. A marker with no branch after it in its own function is dropped at
/// the next `fn`, so it can never arm a branch in a later function.
fn arm_uniform_branches(tokens: &mut [Token<'_>], marker_lines: &[usize]) {
    let mut pending: Vec<usize> = marker_lines.to_vec();
    pending.sort_unstable();
    let mut next = 0;
    for index in 0..tokens.len() {
        let (line, text) = (tokens[index].line, tokens[index].text);
        if text == "fn" {
            while next < pending.len() && pending[next] < line {
                next += 1;
            }
        }
        // The `else` of an `else if` is not the branch the marker means: the
        // `if` that follows carries the condition, so it takes the arm.
        let chained_if = text == "else" && tokens.get(index + 1).is_some_and(|t| t.text == "if");
        if BRANCH_KEYWORDS.contains(&text)
            && !chained_if
            && next < pending.len()
            && pending[next] <= line
        {
            tokens[index].armed = true;
            next += 1;
        }
    }
}

/// `(name, body)` for every `fn NAME(...) ... { body }`; `body` is the tokens
/// strictly between the body's braces.
fn functions<'t, 'a>(tokens: &'t [Token<'a>]) -> Vec<(&'a str, &'t [Token<'a>])> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].text != "fn" || i + 1 >= tokens.len() {
            i += 1;
            continue;
        }
        let name = tokens[i + 1].text;
        // The body opens at the first `{` at paren depth 0 after the parameters.
        let mut j = i + 2;
        let mut paren = 0_i32;
        while j < tokens.len() {
            match tokens[j].text {
                "(" => paren += 1,
                ")" => paren -= 1,
                "{" if paren == 0 => break,
                _ => {}
            }
            j += 1;
        }
        let mut depth = 0_i32;
        let mut k = j;
        while k < tokens.len() {
            match tokens[k].text {
                "{" => depth += 1,
                "}" => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        found.push((name, tokens.get(j + 1..k).unwrap_or_default()));
        i = k + 1;
    }
    found
}

fn is_identifier(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
}

/// Names called in `body`: an identifier directly followed by `(`.
fn calls_in<'a>(body: &[Token<'a>]) -> BTreeSet<&'a str> {
    body.windows(2)
        .filter(|pair| {
            !BRANCH_KEYWORDS.contains(&pair[0].text)
                && pair[1].text == "("
                && is_identifier(pair[0].text)
        })
        .map(|pair| pair[0].text)
        .collect()
}

/// What a `{` opened, one entry per open block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    Plain,
    Branch,
    Uniform,
}

/// Findings for one function body: derivative-taking calls in non-uniform flow.
fn check_function(
    path: &str,
    name: &str,
    body: &[Token<'_>],
    derives: &BTreeSet<&str>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut blocks: Vec<Block> = Vec::new();
    // What the next `{` opens, from the statement head so far.
    let mut pending = Block::Plain;
    // The block just closed was Uniform; an `else` continues it.
    let mut chained_uniform = false;
    // A `return` inside a Branch block earlier in this body.
    let mut returned = false;
    // A `;` inside a `for (init; cond; step)` header is not a statement end.
    let mut paren = 0_i32;
    for (index, token) in body.iter().enumerate() {
        let tok = token.text;
        if BRANCH_KEYWORDS.contains(&tok) {
            if token.armed || (tok == "else" && chained_uniform) {
                pending = Block::Uniform;
            } else if tok == "if" && pending == Block::Uniform {
                // `else if <cond>` chained to a uniform branch: the new
                // condition is its own question, and only its own marker can
                // answer it — a bare `else {` inherits, `else if` does not.
                pending = Block::Branch;
            } else if pending != Block::Uniform {
                pending = Block::Branch;
            }
        } else if tok == "(" {
            paren += 1;
        } else if tok == ")" {
            paren -= 1;
        } else if tok == "{" {
            blocks.push(pending);
            pending = Block::Plain;
        } else if tok == "}" {
            chained_uniform = blocks.pop() == Some(Block::Uniform);
            pending = Block::Plain;
            continue;
        } else if tok == ";" && paren == 0 {
            pending = Block::Plain;
        } else if tok == "return" && blocks.contains(&Block::Branch) {
            returned = true;
        } else if body.get(index + 1).is_some_and(|next| next.text == "(")
            && derives.contains(tok)
            && tok != name
        {
            let kind = if blocks.contains(&Block::Branch) {
                Some(Kind::InsideBranch)
            } else if returned {
                Some(Kind::AfterEarlyReturn)
            } else {
                None
            };
            if let Some(kind) = kind {
                findings.push(Finding {
                    path: path.to_owned(),
                    line: token.line,
                    callee: tok.to_owned(),
                    function: name.to_owned(),
                    kind,
                });
            }
        }
        chained_uniform = false;
    }
    findings
}

/// Findings across `texts` (`(path, wgsl source)`, in order) and the number of
/// user functions that take a derivative.
fn analyse(texts: &[(String, String)]) -> (Vec<Finding>, usize) {
    let normalised: Vec<(&str, String, String)> = texts
        .iter()
        .map(|(path, text)| {
            let text = text.replace("\r\n", "\n");
            (path.as_str(), strip_comments(&text), text)
        })
        .collect();
    let sources: Vec<(&str, Vec<Token<'_>>)> = normalised
        .iter()
        .map(|(path, stripped, raw)| {
            let mut tokens = tokenize(stripped);
            arm_uniform_branches(&mut tokens, &uniform_marked_lines(raw));
            (*path, tokens)
        })
        .collect();

    // Transitive closure of "takes a derivative", across every file: the
    // helpers live in common/*.wgsl and are concatenated into the modules.
    let mut fn_calls: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (_, tokens) in &sources {
        for (name, body) in functions(tokens) {
            fn_calls.entry(name).or_default().extend(calls_in(body));
        }
    }
    let mut derives: BTreeSet<&str> = DERIVATIVES.into_iter().collect();
    let mut changed = true;
    while changed {
        changed = false;
        for (name, called) in &fn_calls {
            if !derives.contains(name) && called.iter().any(|callee| derives.contains(callee)) {
                derives.insert(name);
                changed = true;
            }
        }
    }

    let mut findings = Vec::new();
    for (path, tokens) in &sources {
        for (name, body) in functions(tokens) {
            findings.extend(check_function(path, name, body, &derives));
        }
    }
    (findings, derives.len() - DERIVATIVES.len())
}

/// One function per layout the walk must see through.
const SELF_TEST_FIXTURE: &str = r"
fn else_on_closing_line(x: f32, c: bool) -> f32 {
    var r = 0.0;
    if c {
        r = 1.0;
    } else {
        r = dpdx(x);
    }
    return r;
}
fn one_line_if_return(x: f32, c: bool) -> f32 {
    if c { return 1.0; }
    return dpdx(x);
}
fn one_line_if_else_return(x: f32, c: bool) -> f32 {
    if c { return 1.0; } else { return 2.0; }
    return fwidth(x);
}
fn for_header_split_and_call_split(x: f32) -> f32 {
    for (var i = 0;
         i < 2; i++) {
        return 0.0;
    }
    return helper
        (x);
}
fn helper(x: f32) -> f32 { return dpdy(x); }
fn sampler_in_branch(uv: vec2<f32>, c: bool) -> vec4<f32> {
    if c {
        return textureSample(t, s, uv);
    }
    return vec4<f32>(0.0);
}
fn unconditional_then_select(x: f32, c: bool) -> f32 {
    let g = fwidth(x);
    if c { return 1.0; }
    return select(g, 0.0, c);
}
fn marked_uniform_if_else(x: f32) -> f32 {
    // wgsl-uniformity: uniform (u is the uniform buffer)
    if u.flag > 0.0 {
        return dpdx(x);
    } else {
        return fwidth(x);
    }
}
fn marked_uniform_loop(x: f32) -> f32 {
    var acc = 0.0;
    for (var i = 0; i < u.taps; i++) { // wgsl-uniformity: uniform
        acc += dpdx(x);
    }
    return acc;
}
fn marker_arms_only_the_next_branch(x: f32, c: bool) -> f32 {
    if u.flag > 0.0 { // wgsl-uniformity: uniform
        return 1.0;
    }
    if c {
        return dpdx(x);
    }
    return 0.0;
}
fn else_if_does_not_inherit_the_marker(x: f32, c: bool) -> f32 {
    // wgsl-uniformity: uniform
    if u.flag > 0.0 {
        return 1.0;
    } else if c {
        return dpdx(x);
    }
    return 0.0;
}
fn else_if_with_its_own_marker(x: f32) -> f32 {
    // wgsl-uniformity: uniform
    if u.flag > 0.0 {
        return 1.0;
    } else if u.other > 0.0 { // wgsl-uniformity: uniform
        return dpdx(x);
    }
    return 0.0;
}
fn block_comment_before_marker(x: f32) -> f32 {
    /* a block comment
       spanning three
       lines */
    // wgsl-uniformity: uniform
    if u.flag > 0.0 {
        return dpdx(x);
    }
    return 0.0;
}
fn marker_without_a_branch_is_dropped_here(x: f32) -> f32 {
    // wgsl-uniformity: uniform (nothing to arm in this function)
    return x;
}
fn branch_after_a_stale_marker(x: f32, c: bool) -> f32 {
    if c {
        return dpdx(x);
    }
    return 0.0;
}
";

/// The function and kind of every finding the fixture must produce; every
/// other function must come back clean.
const EXPECTED: [(&str, Kind); 8] = [
    ("else_on_closing_line", Kind::InsideBranch),
    ("one_line_if_return", Kind::AfterEarlyReturn),
    ("one_line_if_else_return", Kind::AfterEarlyReturn),
    ("for_header_split_and_call_split", Kind::AfterEarlyReturn),
    ("sampler_in_branch", Kind::InsideBranch),
    ("marker_arms_only_the_next_branch", Kind::InsideBranch),
    ("else_if_does_not_inherit_the_marker", Kind::InsideBranch),
    ("branch_after_a_stale_marker", Kind::InsideBranch),
];

/// A function of the fixture and the finding it produces.
type Expectation = (String, Kind);

/// `(missed, false positives)` of the walker over the fixture.
fn self_test_diff() -> (Vec<Expectation>, Vec<Expectation>) {
    let (findings, _) = analyse(&[("fixture.wgsl".to_owned(), SELF_TEST_FIXTURE.to_owned())]);
    let seen: BTreeSet<Expectation> = findings
        .into_iter()
        .map(|finding| (finding.function, finding.kind))
        .collect();
    let expected: BTreeSet<Expectation> = EXPECTED
        .iter()
        .map(|(name, kind)| ((*name).to_owned(), *kind))
        .collect();
    (
        expected.difference(&seen).cloned().collect(),
        seen.difference(&expected).cloned().collect(),
    )
}

fn self_test() -> ExitCode {
    let (missed, extra) = self_test_diff();
    for (name, kind) in &missed {
        println!("self-test: MISSED ({name}, {})", kind.describe());
    }
    for (name, kind) in &extra {
        println!("self-test: FALSE POSITIVE ({name}, {})", kind.describe());
    }
    if !missed.is_empty() || !extra.is_empty() {
        return ExitCode::FAILURE;
    }
    println!(
        "wgsl-uniformity: self-test ok ({} expected findings, no others)",
        EXPECTED.len()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn findings(source: &str) -> Vec<String> {
        analyse(&[("t.wgsl".to_owned(), source.to_owned())])
            .0
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn self_test_fixture_yields_exactly_the_expected_findings() {
        let (missed, extra) = self_test_diff();
        assert!(missed.is_empty(), "missed: {missed:?}");
        assert!(extra.is_empty(), "false positives: {extra:?}");
    }

    #[test]
    fn crlf_source_reports_the_same_lines() {
        let source = "fn f(x: f32, c: bool) -> f32 {\n    if c {\n        return dpdx(x);\n    }\n    return 0.0;\n}\n";
        let lf = findings(source);
        assert_eq!(
            lf,
            ["t.wgsl:3: `dpdx` takes a derivative inside a branch in `f`"]
        );
        assert_eq!(findings(&source.replace('\n', "\r\n")), lf);
    }

    #[test]
    fn transitive_helper_in_another_file_is_a_derivative() {
        let texts = [
            (
                "common.wgsl".to_owned(),
                "fn sdf(x: f32) -> f32 { return fwidth(x); }".to_owned(),
            ),
            (
                "main.wgsl".to_owned(),
                "fn fs(x: f32, c: bool) -> f32 {\n  if c { return 0.0; }\n  return sdf(x);\n}"
                    .to_owned(),
            ),
        ];
        let (found, derived) = analyse(&texts);
        assert_eq!(derived, 2, "sdf, and fs through it");
        assert_eq!(
            found.iter().map(ToString::to_string).collect::<Vec<_>>(),
            ["main.wgsl:3: `sdf` takes a derivative after an early return in `fs`"]
        );
    }

    #[test]
    fn marker_inside_a_block_comment_line_still_counts_for_its_line() {
        let source = "fn f(x: f32) -> f32 {\n  /* wgsl-uniformity: uniform */\n  if u.a > 0.0 { return dpdx(x); }\n  return 0.0;\n}";
        assert!(findings(source).is_empty());
    }
}
