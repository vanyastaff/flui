//! Changelog fragments: `changelog.d/<branch-slug>.md`, merged into
//! `CHANGELOG.md`'s `## [Unreleased]` region at release time.
//!
//! Every pull request used to add its entry at the top of the same sections of
//! `CHANGELOG.md`, so any two conflicted. A fragment is a file of its own
//! instead, named after the branch (`/` becomes `-`, the name must match
//! `^[a-z0-9]+(-[a-z0-9]+)*\.md$`); `changelog.d/README.md` is the only other
//! file the directory may hold. Its body is one or more `### <Section>`
//! headers, each one of Keep a Changelog's six ([`SECTIONS`]), each followed by
//! one unordered list (continuation lines and nested lists are part of it).
//! Links are root-relative (`/docs/x.md`), absolute, or email autolinks: a
//! fragment is link-checked where it sits and then pasted into the root file,
//! and a root-relative link resolves the same in both places. An `#anchor`-only
//! link and a reference definition change target once merged, so both are
//! refused.
//!
//! `--check` validates every fragment and `CHANGELOG.md` itself: exactly one
//! `## [Unreleased]`, and only the six known `###` headings in its region, each
//! once. Each finding is a `path:line: rule: message` line; the rule ids are
//! the ones [`self_test`] pins. With no flag the command validates, then puts
//! each section's bullets at the top of that section of the region (fragments
//! in file-name order, a missing section created in canonical position), writes
//! `CHANGELOG.md` and removes the fragments; `--dry-run` prints the new region
//! and writes nothing.
//!
//! Limits: headers are recognized by line (`### ` at column 0), so an
//! unindented fence holding one would split a section, but such a fence at
//! section level is already `not-a-list`. Inline HTML inside a bullet is not
//! read. Writing `CHANGELOG.md` and removing the fragments are two steps; if a
//! removal fails the command names the files, and a rerun would add them again.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;
use std::path::Path;
use std::process::ExitCode;
use std::sync::LazyLock;

use anyhow::Context;
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
use regex::Regex;

use crate::util::repo_root;

/// The fragment directory, relative to the repository root.
const DIR: &str = "changelog.d";

/// The file fragments merge into, relative to the repository root.
const CHANGELOG: &str = "CHANGELOG.md";

/// The heading of the region fragments merge into.
const UNRELEASED: &str = "## [Unreleased]";

/// Keep a Changelog's sections, in the order a release lists them.
const SECTIONS: [&str; 6] = [
    "Added",
    "Changed",
    "Deprecated",
    "Removed",
    "Fixed",
    "Security",
];

/// Arguments for `cargo xtask changelog`.
#[derive(Debug, clap::Args)]
pub(crate) struct ChangelogArgs {
    /// Validate the fragments and CHANGELOG.md; write nothing.
    #[arg(long, conflicts_with = "dry_run")]
    check: bool,
    /// Print the Unreleased region the merge would write; write nothing.
    #[arg(long)]
    dry_run: bool,
    /// Run the rules over the planted fixtures instead of the repository.
    #[arg(long, conflicts_with_all = ["check", "dry_run"])]
    self_test: bool,
}

/// One rule broken at one place.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Finding {
    path: String,
    /// 1-based; 0 for the file as a whole.
    line: usize,
    rule: &'static str,
    message: String,
}

impl Finding {
    fn new(path: &str, line: usize, rule: &'static str, message: impl Into<String>) -> Self {
        Self {
            path: path.to_owned(),
            line,
            rule,
            message: message.into(),
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}: {}: {}", self.path, self.rule, self.message)
        } else {
            write!(
                f,
                "{}:{}: {}: {}",
                self.path, self.line, self.rule, self.message
            )
        }
    }
}

/// A valid fragment.
#[derive(Debug)]
struct Fragment {
    path: String,
    /// Canonical section index → (1-based header line, the list, blank lines
    /// trimmed at both ends).
    sections: BTreeMap<usize, (usize, String)>,
}

/// The `## [Unreleased]` region of a changelog, as line indices into
/// `text.split_inclusive('\n')`.
#[derive(Debug)]
struct Unreleased {
    /// The `## [Unreleased]` line.
    header: usize,
    /// The first line after the region.
    end: usize,
    /// Each `###` heading in the region: its line and canonical index.
    sections: Vec<(usize, usize)>,
}

/// `cargo xtask changelog`.
pub(crate) fn changelog(args: &ChangelogArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return Ok(self_test());
    }
    run(&repo_root(), args)
}

/// The command over the repository at `root`.
fn run(root: &Path, args: &ChangelogArgs) -> anyhow::Result<ExitCode> {
    let entries = entries(&root.join(DIR))?;
    let target = root.join(CHANGELOG);
    let text = normalized(
        &std::fs::read_to_string(&target)
            .with_context(|| format!("reading {}", target.display()))?,
    );
    let (fragments, mut findings) = judge(&entries);
    if let Err(found) = unreleased(&text) {
        findings.extend(found);
    }
    if !findings.is_empty() {
        for finding in &findings {
            println!("{finding}");
        }
        eprintln!(
            "changelog: {} finding(s); the format is in {DIR}/README.md",
            findings.len()
        );
        return Ok(ExitCode::FAILURE);
    }
    if args.check {
        println!(
            "changelog: {} fragment(s) and {CHANGELOG} ok",
            fragments.len()
        );
        return Ok(ExitCode::SUCCESS);
    }
    if fragments.is_empty() {
        println!("changelog: no fragments");
        return Ok(ExitCode::SUCCESS);
    }
    let merged = assemble(&text, &fragments)
        .map_err(|found| anyhow::anyhow!("{CHANGELOG} was valid a moment ago: {}", found[0]))?;
    if args.dry_run {
        print!("{}", region(&merged));
        return Ok(ExitCode::SUCCESS);
    }
    std::fs::write(&target, &merged).with_context(|| format!("writing {}", target.display()))?;
    println!(
        "changelog: {} fragment(s) merged into {CHANGELOG}",
        fragments.len()
    );
    let mut left = Vec::new();
    for fragment in &fragments {
        match std::fs::remove_file(root.join(&fragment.path)) {
            Ok(()) => println!("removed {}", fragment.path),
            Err(error) => left.push(format!("{} ({error})", fragment.path)),
        }
    }
    if left.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "changelog: {CHANGELOG} is written, but these fragments are still there; remove them \
         by hand, since a rerun would merge them again:"
    );
    for path in &left {
        eprintln!("  {path}");
    }
    Ok(ExitCode::FAILURE)
}

/// An entry of the fragment directory: its name, whether it is a directory,
/// and the text of a `.md` file.
type Entry = (String, bool, String);

/// The entries of `dir` sorted by name; none when it is missing.
fn entries(dir: &Path) -> anyhow::Result<Vec<Entry>> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    for entry in read {
        let entry = entry.with_context(|| format!("listing {}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type()?.is_dir();
        let text = if !is_dir && is_md(&name) {
            let path = entry.path();
            std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
        } else {
            String::new()
        };
        entries.push((name, is_dir, text));
    }
    entries.sort();
    Ok(entries)
}

/// Whether `name` ends in `.md`, exactly: `.MD` is not a fragment, since a
/// fragment name is lowercase.
fn is_md(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|ext| ext == "md")
}

/// The fragments among `entries` and every finding about them.
fn judge(entries: &[Entry]) -> (Vec<Fragment>, Vec<Finding>) {
    static SLUG: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[a-z0-9]+(-[a-z0-9]+)*\.md$").expect("BUG: static regex is valid")
    });
    let mut fragments = Vec::new();
    let mut findings = Vec::new();
    for (name, is_dir, text) in entries {
        let path = format!("{DIR}/{name}");
        if *is_dir || !is_md(name) {
            findings.push(Finding::new(
                &path,
                0,
                "not-md",
                format!("{DIR}/ holds only .md fragments and README.md"),
            ));
            continue;
        }
        if name == "README.md" {
            continue;
        }
        if !SLUG.is_match(name) {
            findings.push(Finding::new(
                &path,
                0,
                "name",
                "a fragment is named after its branch: lowercase letters, digits and single \
                 dashes, then .md",
            ));
        }
        match parse_fragment(&path, text) {
            Ok(fragment) => fragments.push(fragment),
            Err(found) => findings.extend(found),
        }
    }
    (fragments, findings)
}

/// `text` with `\r\n` line endings made `\n`.
fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// The section a `### ` line names, if it is one.
fn heading(line: &str) -> Option<&str> {
    line.strip_prefix("### ").map(str::trim)
}

/// `text` without its leading and trailing blank lines.
fn trim_blank_lines(text: &str) -> &str {
    let start = text
        .split_inclusive('\n')
        .take_while(|line| line.trim().is_empty())
        .map(str::len)
        .sum::<usize>();
    text[start..].trim_end()
}

/// Parses one fragment, or every rule it breaks.
fn parse_fragment(path: &str, text: &str) -> Result<Fragment, Vec<Finding>> {
    let text = normalized(text);
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(first) = lines.iter().position(|line| !line.trim().is_empty()) else {
        return Err(vec![Finding::new(
            path,
            1,
            "empty",
            "the fragment is empty",
        )]);
    };
    let headers: Vec<usize> = (0..lines.len())
        .filter(|&index| heading(lines[index]).is_some())
        .collect();
    let mut findings = Vec::new();
    if headers.first() != Some(&first) {
        findings.push(Finding::new(
            path,
            first + 1,
            "preamble",
            "a fragment starts with a `### <Section>` header; nothing goes above it",
        ));
    }
    let mut sections = BTreeMap::new();
    for (nth, &at) in headers.iter().enumerate() {
        let name = heading(lines[at]).expect("BUG: headers are heading lines");
        let end = headers.get(nth + 1).copied().unwrap_or(lines.len());
        let body = lines[at + 1..end].join("\n");
        findings.extend(check_body(path, at + 1, &body));
        let list = trim_blank_lines(&body);
        if list.is_empty() {
            findings.push(Finding::new(
                path,
                at + 1,
                "empty-section",
                format!("`### {name}` has no list under it"),
            ));
        }
        let Some(canon) = SECTIONS.iter().position(|known| *known == name) else {
            findings.push(Finding::new(
                path,
                at + 1,
                "unknown-section",
                format!("`{name}` is not one of {}", SECTIONS.join(", ")),
            ));
            continue;
        };
        if let Some((first_at, _)) = sections.get(&canon) {
            findings.push(Finding::new(
                path,
                at + 1,
                "duplicate-section",
                format!("`### {name}` already appears on line {first_at}"),
            ));
            continue;
        }
        sections.insert(canon, (at + 1, list.to_owned()));
    }
    if findings.is_empty() {
        Ok(Fragment {
            path: path.to_owned(),
            sections,
        })
    } else {
        Err(findings)
    }
}

/// The rules a section body breaks: `header` is the 1-based line of its
/// `###` header, so the body's first line is `header + 1`.
fn check_body(path: &str, header: usize, body: &str) -> Vec<Finding> {
    let line_of = |offset: usize| header + 1 + body[..offset].matches('\n').count();
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_STRIKETHROUGH;
    let mut findings = Vec::new();
    let mut depth = 0_usize;
    let mut listed = false;
    let mut events = Parser::new_ext(body, options).into_offset_iter();
    for (event, span) in events.by_ref() {
        if let Event::Start(
            Tag::Link {
                link_type,
                dest_url,
                ..
            }
            | Tag::Image {
                link_type,
                dest_url,
                ..
            },
        ) = &event
            && let Some(why) = link_problem(*link_type, dest_url)
        {
            findings.push(Finding::new(path, line_of(span.start), "link", why));
        }
        match event {
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Start(tag) => {
                if depth == 0 {
                    if matches!(tag, Tag::List(None)) && !listed {
                        listed = true;
                    } else {
                        findings.push(not_a_list(path, line_of(span.start)));
                    }
                }
                depth += 1;
            }
            _ if depth == 0 => findings.push(not_a_list(path, line_of(span.start))),
            _ => {}
        }
    }
    let mut definitions: Vec<(Range<usize>, &str)> = events
        .reference_definitions()
        .iter()
        .map(|(label, definition)| (definition.span.clone(), label))
        .collect();
    definitions.sort_by_key(|(span, _)| span.start);
    for (span, label) in definitions {
        findings.push(Finding::new(
            path,
            line_of(span.start),
            "link",
            format!(
                "reference definition `[{label}]`: write the link inline, since a definition \
                 changes meaning once fragments are merged"
            ),
        ));
    }
    findings
}

fn not_a_list(path: &str, line: usize) -> Finding {
    Finding::new(
        path,
        line,
        "not-a-list",
        "a section holds one unordered list and nothing else",
    )
}

/// Why a link to `dest` cannot stay in a fragment, if it cannot.
fn link_problem(link_type: LinkType, dest: &str) -> Option<String> {
    let scheme = dest.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if link_type == LinkType::Email || dest.starts_with('/') || scheme {
        None
    } else if dest.starts_with('#') {
        Some(format!(
            "`{dest}` points into whichever file the text is in; link a heading as \
             `/path/file.md{dest}`"
        ))
    } else {
        Some(format!(
            "relative link `{dest}` resolves differently in {DIR}/ and in {CHANGELOG}; write it \
             root-relative (`/{dest}`) or absolute"
        ))
    }
}

/// The Unreleased region of `text` (already [`normalized`]), or what is wrong
/// with the changelog.
fn unreleased(text: &str) -> Result<Unreleased, Vec<Finding>> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let headers: Vec<usize> = (0..lines.len())
        .filter(|&index| lines[index].trim_end() == UNRELEASED)
        .collect();
    let mut findings = Vec::new();
    let Some(&header) = headers.first() else {
        return Err(vec![Finding::new(
            CHANGELOG,
            0,
            "changelog",
            format!("no `{UNRELEASED}` heading"),
        )]);
    };
    for &extra in &headers[1..] {
        findings.push(Finding::new(
            CHANGELOG,
            extra + 1,
            "changelog",
            format!("a second `{UNRELEASED}` heading"),
        ));
    }
    let end = (header + 1..lines.len())
        .find(|&index| lines[index].starts_with("## "))
        .unwrap_or(lines.len());
    let mut sections: Vec<(usize, usize)> = Vec::new();
    for (index, line) in lines.iter().enumerate().take(end).skip(header + 1) {
        let Some(name) = heading(line) else {
            continue;
        };
        match SECTIONS.iter().position(|known| *known == name) {
            None => findings.push(Finding::new(
                CHANGELOG,
                index + 1,
                "changelog",
                format!(
                    "`### {name}` in {UNRELEASED} is not one of {}",
                    SECTIONS.join(", ")
                ),
            )),
            Some(canon) if sections.iter().any(|&(_, seen)| seen == canon) => {
                findings.push(Finding::new(
                    CHANGELOG,
                    index + 1,
                    "changelog",
                    format!("`### {name}` appears twice in {UNRELEASED}"),
                ));
            }
            Some(canon) => sections.push((index, canon)),
        }
    }
    if findings.is_empty() {
        Ok(Unreleased {
            header,
            end,
            sections,
        })
    } else {
        Err(findings)
    }
}

/// `changelog` with every fragment's bullets merged into its Unreleased
/// region; nothing outside the region changes.
fn assemble(changelog: &str, fragments: &[Fragment]) -> Result<String, Vec<Finding>> {
    let text = normalized(changelog);
    let region = unreleased(&text)?;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut ordered: Vec<&Fragment> = fragments.iter().collect();
    ordered.sort_by(|a, b| a.path.cmp(&b.path));

    // (line index to insert before, canonical index, text)
    let mut inserts: Vec<(usize, usize, String)> = Vec::new();
    for (canon, name) in SECTIONS.iter().enumerate() {
        let lists: Vec<&str> = ordered
            .iter()
            .filter_map(|fragment| fragment.sections.get(&canon))
            .map(|(_, list)| list.as_str())
            .collect();
        if lists.is_empty() {
            continue;
        }
        let block = lists.join("\n\n");
        if let Some(&(at, _)) = region.sections.iter().find(|&&(_, c)| c == canon) {
            let insert = if !lines[at].ends_with('\n') {
                (at + 1, format!("\n\n{block}\n"))
            } else if lines.get(at + 1).is_some_and(|line| line.trim().is_empty()) {
                (at + 2, format!("{block}\n\n"))
            } else {
                (at + 1, format!("\n{block}\n\n"))
            };
            inserts.push((insert.0, canon, insert.1));
            continue;
        }
        let later = region
            .sections
            .iter()
            .filter(|&&(_, c)| c > canon)
            .map(|&(at, _)| at)
            .min();
        if let Some(at) = later {
            inserts.push((at, canon, format!("### {name}\n\n{block}\n\n")));
        } else {
            let last = lines[region.end - 1];
            let lead = if !last.ends_with('\n') {
                "\n\n"
            } else if last.trim().is_empty() {
                ""
            } else {
                "\n"
            };
            let tail = if region.end < lines.len() {
                "\n\n"
            } else {
                "\n"
            };
            inserts.push((
                region.end,
                canon,
                format!("{lead}### {name}\n\n{block}{tail}"),
            ));
        }
    }
    inserts.sort_by_key(|&(at, canon, _)| (at, canon));

    let mut out =
        String::with_capacity(text.len() + inserts.iter().map(|i| i.2.len()).sum::<usize>());
    let mut pending = inserts.into_iter().peekable();
    for index in 0..=lines.len() {
        while let Some((_, _, insert)) = pending.next_if(|&(at, _, _)| at == index) {
            out.push_str(&insert);
        }
        if let Some(line) = lines.get(index) {
            out.push_str(line);
        }
    }
    Ok(out)
}

/// The Unreleased region of a valid changelog, heading included.
fn region(text: &str) -> String {
    let region = unreleased(text).expect("BUG: the merge keeps the changelog valid");
    text.split_inclusive('\n')
        .skip(region.header)
        .take(region.end - region.header)
        .collect()
}

/// The planted fragment directory: `(name, is a directory, text)`.
const PLANTED_ENTRIES: [(&str, bool, &str); 8] = [
    (
        "Bad_Name.md",
        false,
        include_str!("../fixtures/changelog/Bad_Name.md.txt"),
    ),
    ("README.md", false, "Anything; it is not a fragment.\n"),
    (
        "a-valid.md",
        false,
        include_str!("../fixtures/changelog/a-valid.md.txt"),
    ),
    (
        "b-valid.md",
        false,
        include_str!("../fixtures/changelog/b-valid.md.txt"),
    ),
    (
        "empty.md",
        false,
        include_str!("../fixtures/changelog/empty.md.txt"),
    ),
    (
        "malformed.md",
        false,
        include_str!("../fixtures/changelog/malformed.md.txt"),
    ),
    ("notes.txt", false, ""),
    ("nested", true, ""),
];

/// The findings no line of a fixture can carry a note for.
const FILE_LEVEL_EXPECTED: [(&str, &str); 3] = [
    ("changelog.d/Bad_Name.md", "name"),
    ("changelog.d/notes.txt", "not-md"),
    ("changelog.d/nested", "not-md"),
];

/// A valid changelog the planted fragments merge into.
const PLANTED_CHANGELOG: &str = include_str!("../fixtures/changelog/changelog.md.txt");
/// A changelog breaking each `changelog` rule; its findings name [`CHANGELOG`].
const PLANTED_BAD_CHANGELOG: &str = include_str!("../fixtures/changelog/changelog-bad.md.txt");
/// [`PLANTED_CHANGELOG`] with `a-valid.md` and `b-valid.md` merged.
const PLANTED_EXPECTED: &str = include_str!("../fixtures/changelog/expected.md.txt");

type Identity = (String, usize, String);

/// `text` with each line's `<!-- expect: rule[, rule] -->` note removed, and
/// the findings those notes ask for. The note is removed before any rule
/// reads the text, so it cannot cause a finding itself.
fn annotated(path: &str, text: &str) -> (String, BTreeSet<Identity>) {
    let mut clean = String::with_capacity(text.len());
    let mut expected = BTreeSet::new();
    for (index, line) in normalized(text).split_inclusive('\n').enumerate() {
        let Some((kept, note)) = line.split_once("<!-- expect:") else {
            clean.push_str(line);
            continue;
        };
        let rules = note.split_once("-->").map_or(note, |(rules, _)| rules);
        for rule in rules.split([',', ' ']).filter(|rule| !rule.is_empty()) {
            expected.insert((path.to_owned(), index + 1, rule.trim().to_owned()));
        }
        clean.push_str(kept.trim_end());
        if line.ends_with('\n') {
            clean.push('\n');
        }
    }
    (clean, expected)
}

/// The planted entries with their notes removed, and every finding expected
/// of them and of the planted bad changelog.
fn planted() -> (Vec<Entry>, String, BTreeSet<Identity>) {
    let mut expected: BTreeSet<Identity> = FILE_LEVEL_EXPECTED
        .iter()
        .map(|&(path, rule)| (path.to_owned(), 0, rule.to_owned()))
        .collect();
    let entries = PLANTED_ENTRIES
        .iter()
        .map(|&(name, is_dir, text)| {
            let (clean, notes) = annotated(&format!("{DIR}/{name}"), text);
            expected.extend(notes);
            (name.to_owned(), is_dir, clean)
        })
        .collect();
    let (bad, notes) = annotated(CHANGELOG, PLANTED_BAD_CHANGELOG);
    expected.extend(notes);
    (entries, bad, expected)
}

/// `(missed, false positives)` of the rules over the planted fixtures, and
/// whether each order of the valid fragments assembles to the expected file.
fn self_test_diff() -> (Vec<Identity>, Vec<Identity>, Vec<String>) {
    let (entries, bad, expected) = planted();
    let (fragments, mut findings) = judge(&entries);
    findings.extend(unreleased(&bad).err().unwrap_or_default());
    if let Err(found) = unreleased(&normalized(PLANTED_CHANGELOG)) {
        findings.extend(found);
    }
    let seen: BTreeSet<Identity> = findings
        .iter()
        .map(|f| (f.path.clone(), f.line, f.rule.to_owned()))
        .collect();
    let missed = expected.difference(&seen).cloned().collect();
    let extra = seen.difference(&expected).cloned().collect();

    let mut valid: Vec<Fragment> = fragments
        .into_iter()
        .filter(|fragment| fragment.path.ends_with("-valid.md"))
        .collect();
    let mut wrong = Vec::new();
    let want = normalized(PLANTED_EXPECTED);
    for order in ["a, b", "b, a"] {
        match assemble(PLANTED_CHANGELOG, &valid) {
            Ok(got) if got == want => {}
            Ok(got) => wrong.push(format!("order {order} assembled to:\n{got}")),
            Err(found) => wrong.push(format!("order {order}: {}", found[0])),
        }
        valid.reverse();
    }
    if valid.len() != 2 {
        wrong.push(format!("{} valid fragments parsed, want 2", valid.len()));
    }
    (missed, extra, wrong)
}

/// `cargo xtask changelog --self-test`.
fn self_test() -> ExitCode {
    let (missed, extra, wrong) = self_test_diff();
    for (path, line, rule) in &missed {
        println!("self-test: MISSED {path}:{line}: {rule}");
    }
    for (path, line, rule) in &extra {
        println!("self-test: FALSE POSITIVE {path}:{line}: {rule}");
    }
    for why in &wrong {
        println!("self-test: ASSEMBLY {why}");
    }
    if !missed.is_empty() || !extra.is_empty() || !wrong.is_empty() {
        return ExitCode::FAILURE;
    }
    println!(
        "changelog: self-test ok ({} planted findings, assembly independent of order)",
        planted().2.len()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests;
