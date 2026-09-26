//! Planted marker text lives in the fixtures, never here: this file is
//! scanned like any other.

use super::*;

const RS: &str = include_str!("../../fixtures/markers/planted.rs.txt");
const MD: &str = include_str!("../../fixtures/markers/planted.md.txt");

/// The line numbers of `text` whose content contains `needle`.
fn lines_with(text: &str, needle: &str) -> Vec<usize> {
    let lines: Vec<usize> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(needle))
        .map(|(index, _)| index + 1)
        .collect();
    assert!(!lines.is_empty(), "no fixture line contains {needle:?}");
    lines
}

/// The classes found on `line` of `text`, read as the file `path`.
fn classes_on(path: &str, text: &str, line: usize) -> Vec<&'static str> {
    find(path, text, false)
        .into_iter()
        .filter(|marker| marker.line == line)
        .map(|marker| marker.class)
        .collect()
}

#[test]
fn self_test_reports_exactly_the_planted_findings() {
    let (missed, extra) = self_test_diff().expect("self-test runs");
    assert!(missed.is_empty(), "missed: {missed:?}");
    assert!(extra.is_empty(), "false positives: {extra:?}");
    // every class is planted at least once, so none can drop out unnoticed
    let planted: BTreeSet<&str> = planted_markers().iter().map(|m| m.class).collect();
    for class in classes::CLASSES.iter() {
        assert!(
            planted.contains(class.name),
            "no planted {} marker",
            class.name
        );
    }
}

#[test]
fn quotation_is_not_prose() {
    for needle in [
        "This doc comment is Markdown",
        "Phase::B",
        "pub enum Phase",
        "const W400",
        "lowercase is domain vocabulary",
        "lay out the children",
        "a history link",
        "the keys",
        "fn nine_slice_advanced",
    ] {
        for line in lines_with(RS, needle) {
            assert_eq!(classes_on("x.rs", RS, line), Vec::<&str>::new(), "{needle}");
        }
    }
    for needle in [
        "in a fenced block",
        "[the plan]",
        "for the history",
        "A code span",
    ] {
        for line in lines_with(MD, needle) {
            assert_eq!(classes_on("x.md", MD, line), Vec::<&str>::new(), "{needle}");
        }
    }
}

#[test]
fn a_cli_flag_is_not_a_slice_label() {
    let flag = lines_with(RS, "run with --");
    assert_eq!(classes_on("x.rs", RS, flag[0]), Vec::<&str>::new());
    let label = lines_with(RS, "a plain slice");
    assert_eq!(classes_on("x.rs", RS, label[0]), ["slice"]);
}

#[test]
fn horizon_names_pass_in_markdown_only() {
    let md = lines_with(MD, "Horizon names");
    assert_eq!(classes_on("x.md", MD, md[0]), Vec::<&str>::new());
    // the same shape in a Rust comment is a tracker id
    let rs = lines_with(RS, "a heading named");
    assert_eq!(classes_on("x.rs", RS, rs[0]), ["tracker-h"]);
    // and the same Markdown read as plain text is prose whole
    assert!(classes_on("x.toml", MD, md[0]).contains(&"tracker-h"));
}

#[test]
fn strings_raw_strings_and_identifiers_are_read_by_the_lexer() {
    for (needle, class) in [
        ("string\" //", "cycle"),
        ("raw string", "slice"),
        ("fn test_", "spec-task"),
        ("block comment", "wave"),
    ] {
        let line = lines_with(RS, needle)[0];
        assert_eq!(classes_on("x.rs", RS, line), [class], "{needle}");
    }
}

#[test]
fn allowlist_counts_are_exact() {
    let found = planted_markers();
    let allow = Allowlist::parse(PLANTED_ALLOWLIST).expect("parses");
    let exits = Exits::from_parts(Vec::new(), PLANTED_PLAN);
    let scanned: BTreeSet<String> = PLANTED.iter().map(|&(path, ..)| path.to_owned()).collect();
    let labels: BTreeSet<String> = judge(&found, &scanned, &allow, &exits)
        .iter()
        .map(Finding::identity)
        .filter(|(_, line, _)| *line == 0)
        .map(|(path, _, label)| format!("{path} {label}"))
        .collect();
    let expected: BTreeSet<String> = RATCHET_EXPECTED
        .iter()
        .map(|(path, label)| format!("{path} {label}"))
        .collect();
    assert_eq!(labels, expected);
}

#[test]
fn a_bad_exit_or_missing_reason_is_a_finding() {
    let found = planted_markers();
    let scanned: BTreeSet<String> = PLANTED.iter().map(|&(path, ..)| path.to_owned()).collect();
    let mut allow = Allowlist::parse(PLANTED_ALLOWLIST).expect("parses");
    allow.reason = String::new();
    allow.exit = "ADR-9999".to_owned();
    let exits = Exits::from_parts(Vec::new(), PLANTED_PLAN);
    let labels: Vec<String> = judge(&found, &scanned, &allow, &exits)
        .iter()
        .map(|finding| finding.identity().2)
        .filter(|label| label == "no-reason" || label == "bad-exit")
        .collect();
    assert_eq!(labels, ["no-reason", "bad-exit"]);
}

#[test]
fn exit_values_are_not_scanned() {
    // the planted allowlist's exit is a step id: silent as an allowlist
    let exit_line = lines_with(PLANTED_ALLOWLIST, "exit = ")[0];
    let as_allowlist: Vec<usize> = find("a.toml", PLANTED_ALLOWLIST, true)
        .iter()
        .map(|marker| marker.line)
        .collect();
    assert!(!as_allowlist.contains(&exit_line), "{as_allowlist:?}");
    // and reported when the same text is not an allowlist
    let as_text: Vec<usize> = find("a.toml", PLANTED_ALLOWLIST, false)
        .iter()
        .map(|marker| marker.line)
        .collect();
    assert!(as_text.contains(&exit_line), "{as_text:?}");
}

#[test]
fn seed_round_trips() {
    let found = planted_markers();
    let seeded = seed(&found)
        .replace("reason = \"\"", "reason = \"planted\"")
        .replace("exit = \"\"", "exit = \"ADR-0081\"");
    let allow = Allowlist::parse(&seeded).expect("the seed parses");
    let exits = Exits::from_parts(["0081".to_owned()], PLANTED_PLAN);
    let scanned: BTreeSet<String> = PLANTED.iter().map(|&(path, ..)| path.to_owned()).collect();
    let findings = judge(&found, &scanned, &allow, &exits);
    assert!(findings.is_empty(), "{findings:?}");
    // unfilled, the seed is refused
    let unfilled = Allowlist::parse(&seed(&found)).expect("parses");
    assert!(!judge(&found, &scanned, &unfilled, &exits).is_empty());
}

#[test]
fn archival_roots_are_the_ones_agents_md_lists() {
    let agents = crate::util::read("AGENTS.md").expect("AGENTS.md");
    let flat = agents.split_whitespace().collect::<Vec<_>>().join(" ");
    let (_, rest) = flat
        .split_once("Archival roots are exempt (")
        .expect("AGENTS.md lists the archival roots");
    let (list, _) = rest.split_once(").").expect("the list closes");
    let mut listed = BTreeSet::new();
    for item in list.split('`').skip(1).step_by(2) {
        match item.split_once('{') {
            Some((prefix, braced)) => {
                for name in braced.trim_end_matches('}').split(',') {
                    listed.insert(format!("{prefix}{}/", name.trim()));
                }
            }
            None => {
                listed.insert(format!("{item}/"));
            }
        }
    }
    let roots: BTreeSet<String> = ARCHIVAL_ROOTS
        .iter()
        .map(|root| (*root).to_owned())
        .collect();
    assert_eq!(listed, roots);
}

#[test]
fn the_real_allowlist_parses_and_names_known_classes() {
    let allow = Allowlist::parse(&crate::util::read(ALLOWLIST).expect("reads")).expect("parses");
    for entry in &allow.allow {
        assert!(
            classes::known(&entry.class),
            "{}: {}",
            entry.path,
            entry.class
        );
        assert!(entry.count > 0, "{}", entry.path);
    }
}

#[test]
fn the_token_walk_tells_code_from_comments_and_strings() {
    use tokens::{DocStyle, Token, tokens};
    let src = concat!(
        "fn f<'a>(x: &'a str) -> char {\n",
        "    let q = '\\''; let b = b'\"'; let c = 'z';\n",
        "    let s = \"a // not a comment \\\" still\";\n",
        "    let r = r##\"raw \"# inside\"##;\n",
        "    /* outer /* nested */ still outer */\n",
        "    let r#type = br\"bytes\";\n",
        "    'outer: loop { break 'outer; }\n",
        "}\n",
        "/// doc\n",
        "//! inner\n",
        "//// plain\n",
        "/** block doc */ /*** plain */ /**/\n",
    );
    let seen: Vec<(Token, &str)> = tokens(src)
        .into_iter()
        .filter(|(token, _)| *token != Token::Ident)
        .map(|(token, range)| (token, &src[range]))
        .collect();
    assert_eq!(
        seen,
        [
            (Token::Str, "\"a // not a comment \\\" still\""),
            (Token::Str, "r##\"raw \"# inside\"##"),
            (
                Token::BlockComment(None),
                "/* outer /* nested */ still outer */"
            ),
            (Token::Str, "br\"bytes\""),
            (Token::LineComment(Some(DocStyle::Outer)), "/// doc"),
            (Token::LineComment(Some(DocStyle::Inner)), "//! inner"),
            (Token::LineComment(None), "//// plain"),
            (
                Token::BlockComment(Some(DocStyle::Outer)),
                "/** block doc */"
            ),
            (Token::BlockComment(None), "/*** plain */"),
            (Token::BlockComment(None), "/**/"),
        ]
    );
    let idents: Vec<&str> = tokens(src)
        .into_iter()
        .filter(|(token, _)| *token == Token::Ident)
        .map(|(_, range)| &src[range])
        .collect();
    // lifetimes, labels and character literals are none of these; `r#type` is `type`
    assert!(idents.contains(&"type"), "{idents:?}");
    for absent in ["a", "outer", "z"] {
        assert!(!idents.contains(&absent), "{absent} lexed as an identifier");
    }
}
