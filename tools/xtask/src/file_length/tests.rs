use super::modules::walk;
use super::*;

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

fn the_real_allowlist_parses_and_names_real_exits() {
    let allow = Allowlist::parse(&crate::util::read(ALLOWLIST).expect("reads")).expect("parses");
    let exits = Exits::from_repo(&repo_root()).expect("exits");
    for entry in &allow.allow {
        assert_eq!(exits.check(&entry.exit), Ok(()), "{}", entry.path);
        assert!(entry.lines > LIMIT, "{}", entry.path);
    }
}

#[test]
fn file_length_contract() {
    crate::table_test::run_table(
        "file_length_contract",
        &[
            ("seed_round_trips", seed_round_trips as fn()),
            (
                "the_real_allowlist_parses_and_names_real_exits",
                the_real_allowlist_parses_and_names_real_exits as fn(),
            ),
        ],
    );
}
