use super::modules::{implies_test, physical_lines, walk};
use super::*;

#[test]
fn self_test_reports_exactly_the_planted_files() {
    let (missed, extra) = self_test_diff().expect("self-test runs");
    assert!(missed.is_empty(), "missed: {missed:?}");
    assert!(extra.is_empty(), "false positives: {extra:?}");
}

#[test]
fn cfg_predicates_that_imply_test() {
    let implies = |text: &str| implies_test(&syn::parse_str(text).expect("a cfg predicate"));
    for yes in [
        "test",
        "all(test, unix)",
        "all(any(test, x), test)",
        "any(test, all(test, y))",
    ] {
        assert!(implies(yes), "{yes} implies test");
    }
    for no in [
        "any(test, unix)",
        "not(test)",
        "feature = \"testing\"",
        "all(unix, not(test))",
        "any()",
        "testing",
    ] {
        assert!(!implies(no), "{no} does not imply test");
    }
}

/// Writes `files` under a fresh directory; returns it.
fn tree_of(files: &[(&str, &str)]) -> ScratchDir {
    let dir = ScratchDir::new("file-length-test").expect("scratch dir");
    for (path, text) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().expect("has a parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }
    dir
}

#[test]
fn excluded_range_spans_attributes_to_last_token() {
    let dir = tree_of(&[(
        "lib.rs",
        "fn a() {}\n\
         /// documented helper\n\
         #[cfg(test)]\n\
         fn helper() {\n\
         }\n\
         // a plain comment\n\
         #[cfg(test)]\n\
         fn other() {}\n\
         fn b() {}\n",
    )]);
    let tree = walk([dir.path().join("lib.rs")]);
    let module = tree.modules.values().next().expect("one module");
    // the doc comment (2) through the closing brace (5); the plain comment (6) counts
    assert_eq!(module.test_only, [(2, 5), (7, 8)]);
    assert_eq!(module.lines, 9);
    assert_eq!(module.production_lines(), 3);
}

#[test]
fn module_paths_resolve_like_rustc() {
    let dir = tree_of(&[
        // a mod-rs root: children beside it
        (
            "src/lib.rs",
            "mod plain;\nmod folder;\n#[path = \"elsewhere/moved.rs\"]\nmod moved;\n",
        ),
        // a non-mod-rs file: children in a directory named after it
        (
            "src/plain.rs",
            "mod kid;\nmod inline {\n    #[path = \"deep.rs\"]\n    mod deep;\n    mod nested;\n}\n",
        ),
        ("src/plain/kid.rs", ""),
        ("src/plain/inline/deep.rs", ""),
        ("src/plain/inline/nested.rs", ""),
        ("src/folder/mod.rs", "mod inner;\n"),
        ("src/folder/inner.rs", ""),
        // a #[path] file is mod-rs: its children sit beside it
        ("src/elsewhere/moved.rs", "mod sibling;\n"),
        ("src/elsewhere/sibling.rs", ""),
        // decoys where a wrong rule would look
        ("src/kid.rs", "compile_error!();\n"),
        ("src/elsewhere/moved/sibling.rs", "compile_error!();\n"),
    ]);
    let tree = walk([dir.path().join("src/lib.rs")]);
    assert!(tree.problems.is_empty(), "{:?}", tree.problems);
    let reached: BTreeSet<String> = tree
        .modules
        .keys()
        .map(|path| relative(dir.path(), path))
        .collect();
    let reached: Vec<&str> = reached.iter().map(String::as_str).collect();
    assert_eq!(
        reached,
        [
            "src/elsewhere/moved.rs",
            "src/elsewhere/sibling.rs",
            "src/folder/inner.rs",
            "src/folder/mod.rs",
            "src/lib.rs",
            "src/plain.rs",
            "src/plain/inline/deep.rs",
            "src/plain/inline/nested.rs",
            "src/plain/kid.rs",
        ]
    );
}

#[test]
fn physical_lines_count_an_unterminated_last_line() {
    assert_eq!(physical_lines(""), 0);
    assert_eq!(physical_lines("a"), 1);
    assert_eq!(physical_lines("a\n"), 1);
    assert_eq!(physical_lines("a\r\nb\r\n"), 2);
    assert_eq!(physical_lines("a\nb"), 2);
}

#[test]
fn seed_round_trips() {
    let (dir, _) = self_test_findings().expect("planted tree");
    let root = dir.path();
    let tree = walk([root.join("src/lib.rs")]);
    let seeded = seed(root, &tree);
    // the seed's empty exit and reason are refused until someone fills them in
    let empty = Allowlist::parse(&seeded).expect("the seed parses");
    let exits = Exits::from_parts(["0081".to_owned()], PLANTED_PLAN);
    let refused = scan(root, &tree, &empty, &exits);
    assert!(
        refused.iter().any(|f| matches!(f, Finding::BadExit { .. }))
            && refused
                .iter()
                .any(|f| matches!(f, Finding::NoReason { .. })),
        "{refused:?}"
    );
    let filled = seeded
        .replace("exit = \"\"", "exit = \"ADR-0081\"")
        .replace("reason = \"\"", "reason = \"filled in\"");
    let allow = Allowlist::parse(&filled).expect("parses");
    let findings = scan(root, &tree, &allow, &exits);
    // only the unresolvable module is left: every file over the limit is granted
    let kinds: Vec<&str> = findings.iter().map(|f| f.identity().1).collect();
    assert_eq!(kinds, ["walk"], "{findings:?}");
}

#[test]
fn the_real_allowlist_parses_and_names_real_exits() {
    let allow = Allowlist::parse(&crate::util::read(ALLOWLIST).expect("reads")).expect("parses");
    let exits = Exits::from_repo(&repo_root()).expect("exits");
    for entry in &allow.allow {
        assert_eq!(exits.check(&entry.exit), Ok(()), "{}", entry.path);
        assert!(entry.lines > LIMIT, "{}", entry.path);
    }
}
