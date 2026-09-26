use std::fmt::Write as _;

use super::*;

/// The rules `text`, as fragment `x.md`, breaks: `(line, rule)`.
fn rules(text: &str) -> Vec<(usize, &'static str)> {
    match parse_fragment("x.md", text) {
        Ok(_) => Vec::new(),
        Err(found) => found.iter().map(|f| (f.line, f.rule)).collect(),
    }
}

fn fragment(path: &str, text: &str) -> Fragment {
    parse_fragment(path, text).unwrap_or_else(|found| panic!("{path} is valid: {found:?}"))
}

const CHANGELOG_FIXTURE: &str = "\
# Changelog

Top preamble.

## [Unreleased]

Unreleased preamble.

### Added

- old added

### Changed

- old changed

### Removed

- old removed

### Fixed

- old fixed

## [0.1.0] - 2026-01-01

### Added

- released
";

#[test]
fn self_test_passes() {
    let (missed, extra, wrong) = self_test_diff();
    assert!(missed.is_empty(), "missed: {missed:?}");
    assert!(extra.is_empty(), "false positives: {extra:?}");
    assert!(wrong.is_empty(), "assembly: {wrong:?}");
    assert_eq!(self_test(), ExitCode::SUCCESS);
}

#[test]
fn a_fragment_with_each_section_parses() {
    let mut text = String::new();
    for name in SECTIONS {
        writeln!(
            text,
            "### {name}\n\n- a {name} bullet\n  continued\n  - nested\n"
        )
        .expect("writes");
    }
    let parsed = fragment("x.md", &text);
    assert_eq!(parsed.sections.len(), SECTIONS.len());
    for (canon, name) in SECTIONS.iter().enumerate() {
        assert_eq!(
            parsed.sections[&canon].1,
            format!("- a {name} bullet\n  continued\n  - nested")
        );
    }
    // CRLF reads the same
    let crlf = fragment("x.md", &text.replace('\n', "\r\n"));
    assert_eq!(crlf.sections, parsed.sections);
}

#[test]
fn each_rule_fires_on_its_planted_line() {
    let (entries, _, expected) = planted();
    let (_, findings) = judge(&entries);
    let rules: BTreeSet<&str> = findings.iter().map(|f| f.rule).collect();
    // every fragment rule has a planted case
    for rule in [
        "name",
        "not-md",
        "empty",
        "preamble",
        "unknown-section",
        "duplicate-section",
        "empty-section",
        "not-a-list",
        "list-marker",
        "link",
    ] {
        assert!(rules.contains(rule), "{rule} never fires");
    }
    for finding in &findings {
        let id = (finding.path.clone(), finding.line, finding.rule.to_owned());
        assert!(expected.contains(&id), "unplanted: {finding}");
    }
}

#[test]
fn findings_print_as_path_line_rule_message() {
    let found = rules_of("### Bogus\n\n- x\n");
    assert_eq!(
        found[0].to_string(),
        "x.md:1: unknown-section: `Bogus` is not one of Added, Changed, Deprecated, Removed, \
         Fixed, Security"
    );
    assert_eq!(
        Finding::new("changelog.d/y", 0, "not-md", "m").to_string(),
        "changelog.d/y: not-md: m"
    );
}

fn rules_of(text: &str) -> Vec<Finding> {
    parse_fragment("x.md", text).expect_err("invalid")
}

#[test]
fn readme_is_the_only_non_slug_file() {
    let valid = "### Fixed\n\n- x\n".to_owned();
    let entry = |name: &str, is_dir| (name.to_owned(), is_dir, valid.clone());
    let (fragments, findings) = judge(&[
        entry("README.md", false),
        entry("tools-a-b2.md", false),
        entry("Readme.md", false),
        entry("readme.md", false),
        entry("x--y.md", false),
        entry("-x.md", false),
        entry("x_y.md", false),
        entry("x.markdown", false),
        entry("sub", true),
        entry("sub.md", true),
    ]);
    let names: Vec<&str> = fragments.iter().map(|f| f.path.as_str()).collect();
    // README.md is skipped; a bad name is still parsed, so its content is judged too
    assert_eq!(
        names,
        [
            "changelog.d/tools-a-b2.md",
            "changelog.d/Readme.md",
            "changelog.d/readme.md",
            "changelog.d/x--y.md",
            "changelog.d/-x.md",
            "changelog.d/x_y.md",
        ]
    );
    let found: Vec<(&str, &str)> = findings.iter().map(|f| (f.path.as_str(), f.rule)).collect();
    assert_eq!(
        found,
        [
            ("changelog.d/Readme.md", "name"),
            ("changelog.d/x--y.md", "name"),
            ("changelog.d/-x.md", "name"),
            ("changelog.d/x_y.md", "name"),
            ("changelog.d/x.markdown", "not-md"),
            ("changelog.d/sub", "not-md"),
            ("changelog.d/sub.md", "not-md"),
        ]
    );
}

#[test]
fn assembly_is_independent_of_input_order() {
    let fragments = [
        (
            "changelog.d/a.md",
            "### Added\n\n- a added\n\n### Fixed\n\n- a fixed\n",
        ),
        (
            "changelog.d/b.md",
            "### Fixed\n\n- b fixed\n\n### Security\n\n- b security\n",
        ),
        ("changelog.d/c.md", "### Added\n\n- c added\n  - c nested\n"),
    ];
    let merged = |order: [usize; 3]| {
        let list: Vec<Fragment> = order
            .iter()
            .map(|&i| fragment(fragments[i].0, fragments[i].1))
            .collect();
        assemble(CHANGELOG_FIXTURE, &list).expect("valid")
    };
    let first = merged([0, 1, 2]);
    for order in [[0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        assert_eq!(merged(order), first, "{order:?}");
    }
    assert!(
        first.contains("### Added\n\n- a added\n\n- c added\n  - c nested\n\n- old added\n"),
        "{first}"
    );
    assert!(
        first.contains("### Fixed\n\n- a fixed\n\n- b fixed\n\n- old fixed\n"),
        "{first}"
    );
}

#[test]
fn assembly_puts_new_bullets_above_existing_ones() {
    let merged = assemble(
        CHANGELOG_FIXTURE,
        &[fragment(
            "changelog.d/x.md",
            "### Changed\n\n- new changed\n",
        )],
    )
    .expect("valid");
    assert_eq!(
        merged,
        CHANGELOG_FIXTURE.replace(
            "### Changed\n\n- old changed\n",
            "### Changed\n\n- new changed\n\n- old changed\n"
        )
    );
}

#[test]
fn a_missing_section_is_created_in_canonical_position() {
    let merged = assemble(
        CHANGELOG_FIXTURE,
        &[fragment(
            "changelog.d/x.md",
            "### Security\n\n- new security\n\n### Deprecated\n\n- new deprecated\n",
        )],
    )
    .expect("valid");
    let want = CHANGELOG_FIXTURE
        .replace(
            "### Removed\n\n- old removed\n",
            "### Deprecated\n\n- new deprecated\n\n### Removed\n\n- old removed\n",
        )
        .replace(
            "- old fixed\n\n## [0.1.0]",
            "- old fixed\n\n### Security\n\n- new security\n\n## [0.1.0]",
        );
    assert_eq!(merged, want);

    // a region with no sections at all, ending at the end of the file
    let bare = "# Changelog\n\n## [Unreleased]\n\nOnly a preamble.\n";
    let merged = assemble(
        bare,
        &[fragment(
            "changelog.d/x.md",
            "### Fixed\n\n- f\n\n### Added\n\n- a\n",
        )],
    )
    .expect("valid");
    assert_eq!(
        merged,
        "# Changelog\n\n## [Unreleased]\n\nOnly a preamble.\n\n### Added\n\n- a\n\n### Fixed\n\n- f\n"
    );
}

#[test]
fn assembly_touches_nothing_outside_unreleased() {
    let merged = assemble(
        CHANGELOG_FIXTURE,
        &[fragment(
            "changelog.d/x.md",
            "### Added\n\n- n\n\n### Security\n\n- s\n",
        )],
    )
    .expect("valid");
    let region_start = CHANGELOG_FIXTURE.find("## [Unreleased]").expect("has it");
    let released = CHANGELOG_FIXTURE.find("## [0.1.0]").expect("has it");
    let preamble_end = CHANGELOG_FIXTURE.find("### Added").expect("has it");
    assert!(merged.starts_with(&CHANGELOG_FIXTURE[..preamble_end]));
    assert!(merged.ends_with(&CHANGELOG_FIXTURE[released..]));
    assert!(merged[region_start..].starts_with("## [Unreleased]\n\nUnreleased preamble.\n\n"));
    assert_eq!(
        region(&merged),
        merged[region_start..merged.find("## [0.1.0]").expect("kept")]
    );
}

#[test]
fn assembly_with_no_fragments_is_identity() {
    assert_eq!(
        assemble(CHANGELOG_FIXTURE, &[]).expect("valid"),
        CHANGELOG_FIXTURE
    );
    // CRLF input comes out LF, nothing else changes
    assert_eq!(
        assemble(&CHANGELOG_FIXTURE.replace('\n', "\r\n"), &[]).expect("valid"),
        CHANGELOG_FIXTURE
    );
}

#[test]
fn unknown_heading_in_unreleased_is_refused() {
    let broken = CHANGELOG_FIXTURE.replace("### Changed\n", "### Notes\n");
    let found = unreleased(&broken).expect_err("refused");
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].line, found[0].rule), (13, "changelog"));
    let found = assemble(&broken, &[]).expect_err("assembly refuses it too");
    assert_eq!(found[0].rule, "changelog");
    // no Unreleased heading, or two
    let none = CHANGELOG_FIXTURE.replace("## [Unreleased]", "## [0.2.0]");
    assert_eq!(unreleased(&none).expect_err("refused")[0].line, 0);
    let two = format!("{CHANGELOG_FIXTURE}\n## [Unreleased]\n");
    assert_eq!(unreleased(&two).expect_err("refused")[0].rule, "changelog");
    // a heading below the region is not judged
    let below = CHANGELOG_FIXTURE.replace("- released", "### Notes\n\n- released");
    assert!(unreleased(&below).is_ok());
}

#[test]
fn relative_and_anchor_links_are_refused_root_relative_accepted() {
    for ok in [
        "[x](/docs/testing.md)",
        "[x](/docs/testing.md#commands)",
        "[x](https://example.com/a)",
        "[x](mailto:a@example.com)",
        "<https://example.com>",
        "<a@example.com>",
        "`docs/testing.md`",
        "![img](/assets/x.png)",
    ] {
        assert_eq!(rules(&format!("### Added\n\n- {ok}\n")), [], "{ok}");
    }
    for bad in [
        "[x](docs/testing.md)",
        "[x](../docs/testing.md)",
        "[x](./x.md)",
        "[x](#added)",
        "![img](assets/x.png)",
        "[x](//example.com/a)",
    ] {
        assert_eq!(
            rules(&format!("### Added\n\n- {bad}\n")),
            [(3, "link")],
            "{bad}"
        );
    }
    assert_eq!(
        rules("### Added\n\n- a [ref] link\n\n[ref]: /docs/testing.md\n"),
        [(5, "link")]
    );
}

#[test]
fn section_bodies_hold_one_unordered_list() {
    assert_eq!(rules("### Added\n\n- a\n\n- b\n"), []);
    assert_eq!(rules("### Added\n\n1. a\n"), [(3, "not-a-list")]);
    assert_eq!(rules("### Added\n\n- a\n\n* b\n"), [(5, "not-a-list")]);
    assert_eq!(rules("### Added\n\n* a\n"), [(3, "list-marker")]);
    assert_eq!(
        rules("### Added\n\n+ a\n  - nested\n"),
        [(3, "list-marker")]
    );
    assert_eq!(rules("### Added\n\n - a\n   * nested\n"), []);
    assert_eq!(rules("### Added\n\n- a\n\n> quote\n"), [(5, "not-a-list")]);
    assert_eq!(
        rules("### Added\n\n- a\n\n<div>x</div>\n"),
        [(5, "not-a-list")]
    );
    assert_eq!(rules("### Added\n\n- a\n\n---\n"), [(5, "not-a-list")]);
    assert_eq!(
        rules("### Added\n\n| a |\n|---|\n| b |\n"),
        [(3, "not-a-list")]
    );
    assert_eq!(rules("### Added\n"), [(1, "empty-section")]);
    assert_eq!(rules("# Title\n\n### Added\n\n- a\n"), [(1, "preamble")]);
    assert_eq!(
        rules("---\nfront: matter\n---\n### Added\n\n- a\n"),
        [(1, "preamble")]
    );
    assert_eq!(rules(" \n\n"), [(1, "empty")]);
    assert_eq!(
        rules("### Added\n\n- a\n\n### Added\n\n- b\n"),
        [(5, "duplicate-section")]
    );
}

/// A scratch repository: `CHANGELOG.md` holding [`CHANGELOG_FIXTURE`] and
/// `changelog.d/` holding `fragments`. Removed on drop.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str, fragments: &[(&str, &str)]) -> Self {
        let root =
            std::env::temp_dir().join(format!("xtask-changelog-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(DIR)).expect("creates the scratch tree");
        std::fs::write(root.join(CHANGELOG), CHANGELOG_FIXTURE).expect("writes");
        for (name, text) in fragments {
            std::fs::write(root.join(DIR).join(name), text).expect("writes");
        }
        Self(root)
    }

    fn changelog(&self) -> String {
        std::fs::read_to_string(self.0.join(CHANGELOG)).expect("reads")
    }

    fn fragments(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.0.join(DIR))
            .expect("lists")
            .map(|entry| {
                entry
                    .expect("lists")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `cargo xtask changelog <flags>`, parsed as the command line parses it.
fn args(flags: &[&str]) -> ChangelogArgs {
    use clap::Parser as _;
    let argv = ["xtask", "changelog"].iter().chain(flags);
    match crate::Cli::try_parse_from(argv).expect("parses").command {
        crate::Command::Changelog(args) => args,
        other => panic!("parsed as {other:?}"),
    }
}

#[test]
fn the_merge_writes_the_changelog_and_removes_the_fragments() {
    let scratch = Scratch::new(
        "merge",
        &[
            ("README.md", "Not a fragment.\n"),
            ("b.md", "### Fixed\n\n- b fixed\n"),
            ("a.md", "### Fixed\n\n- a fixed\n"),
        ],
    );
    assert_eq!(
        run(&scratch.0, &args(&["--write"])).expect("runs"),
        ExitCode::SUCCESS
    );
    assert_eq!(
        scratch.changelog(),
        CHANGELOG_FIXTURE.replace(
            "### Fixed\n\n- old fixed\n",
            "### Fixed\n\n- a fixed\n\n- b fixed\n\n- old fixed\n"
        )
    );
    assert_eq!(scratch.fragments(), ["README.md"]);
    // a second run finds nothing to merge and changes nothing
    let merged = scratch.changelog();
    assert_eq!(
        run(&scratch.0, &args(&["--write"])).expect("runs"),
        ExitCode::SUCCESS
    );
    assert_eq!(scratch.changelog(), merged);
}

#[test]
fn only_write_changes_the_tree() {
    let scratch = Scratch::new("read-only", &[("a.md", "### Added\n\n- a\n")]);
    for flags in [&[][..], &["--check"], &["--dry-run"]] {
        assert_eq!(
            run(&scratch.0, &args(flags)).expect("runs"),
            ExitCode::SUCCESS
        );
        assert_eq!(scratch.changelog(), CHANGELOG_FIXTURE);
        assert_eq!(scratch.fragments(), ["a.md"], "{flags:?}");
    }
    // the modes exclude each other
    for flags in [["--write", "--check"], ["--write", "--dry-run"]] {
        let argv = ["xtask", "changelog"].iter().chain(&flags);
        assert!(
            <crate::Cli as clap::Parser>::try_parse_from(argv).is_err(),
            "{flags:?}"
        );
    }
}

#[test]
fn an_invalid_fragment_stops_the_merge() {
    let scratch = Scratch::new(
        "invalid",
        &[
            ("a.md", "### Added\n\n- a\n"),
            ("b.md", "### Notes\n\n- b\n"),
        ],
    );
    assert_eq!(
        run(&scratch.0, &args(&["--write"])).expect("runs"),
        ExitCode::FAILURE
    );
    assert_eq!(scratch.changelog(), CHANGELOG_FIXTURE);
    assert_eq!(scratch.fragments(), ["a.md", "b.md"]);
}

#[test]
fn a_fragment_directory_that_cannot_be_listed_is_an_error() {
    let scratch = Scratch::new("unlistable", &[]);
    assert!(
        entries(&scratch.0.join("missing"))
            .expect("missing is empty")
            .is_empty()
    );
    // a file where the directory should be: listing it fails, and is not empty
    let file = scratch.0.join(CHANGELOG);
    let error = entries(&file).expect_err("not a directory");
    assert!(error.to_string().contains("listing"), "{error:#}");
}
