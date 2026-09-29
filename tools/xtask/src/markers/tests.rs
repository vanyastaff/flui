//! Planted marker text lives in the fixtures, never here: this file is
//! scanned like any other.

use super::*;

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

fn archival_roots_are_the_ones_agents_md_lists() {
    let agents = crate::util::read("AGENTS.md").expect("AGENTS.md");
    assert_eq!(archival_drift(&agents), Ok(()));
    // a root added to the prose alone is drift
    let widened = agents.replace("`openspec`)", "`openspec`, `docs/drafts`)");
    assert_ne!(widened, agents, "the list's last item moved");
    let why = archival_drift(&widened).expect_err("drift is reported");
    assert!(why.contains("docs/drafts/"), "{why}");
    assert!(archival_drift("no list here").is_err());
}

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

#[test]
fn markers_contract() {
    crate::table_test::run_table(
        "markers_contract",
        &[
            (
                "self_test_reports_exactly_the_planted_findings",
                self_test_reports_exactly_the_planted_findings as fn(),
            ),
            (
                "a_bad_exit_or_missing_reason_is_a_finding",
                a_bad_exit_or_missing_reason_is_a_finding as fn(),
            ),
            ("seed_round_trips", seed_round_trips as fn()),
            (
                "archival_roots_are_the_ones_agents_md_lists",
                archival_roots_are_the_ones_agents_md_lists as fn(),
            ),
            (
                "the_real_allowlist_parses_and_names_known_classes",
                the_real_allowlist_parses_and_names_known_classes as fn(),
            ),
            (
                "the_token_walk_tells_code_from_comments_and_strings",
                the_token_walk_tells_code_from_comments_and_strings as fn(),
            ),
        ],
    );
}
