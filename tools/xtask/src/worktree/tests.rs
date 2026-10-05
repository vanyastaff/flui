use super::*;
use crate::util::ScratchDir;

fn accepts_lowercase_kebab_area_and_slug() {
    for name in ["tooling/worktree-command", "a/b", "ci2/x-1-y", "0/9"] {
        let branch = BranchName::parse(name).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert_eq!(branch.to_string(), name);
    }
    let branch = BranchName::parse("docs/agents-guide").expect("valid");
    assert_eq!(branch.slug(), "agents-guide");
}

fn rejects_every_other_shape() {
    for name in [
        "",
        "worktree-command",
        "Tooling/x",
        "tooling/X",
        "a/b/c",
        "a//b",
        "/b",
        "a/",
        "-a/b",
        "a-/b",
        "a--b/c",
        "a/b_c",
        "a/b.c",
        "a/b ",
        " a/b",
    ] {
        assert!(BranchName::parse(name).is_err(), "`{name}` was accepted");
    }
}

fn the_root_is_dot_worktrees_in_the_main_checkout() {
    let root = worktree_root(Path::new("/src/flui/.git")).expect("has a parent");
    assert_eq!(root, Path::new("/src/flui").join(ROOT_DIR));
}

fn porcelain_records_parse() {
    let porcelain = "\
worktree D:/flui
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree D:/flui/.worktrees/a
HEAD 2222222222222222222222222222222222222222
branch refs/heads/tooling/a
locked

worktree C:/elsewhere/b
HEAD 3333333333333333333333333333333333333333
detached
prunable gitdir file points to non-existent location

worktree D:/bare
bare
";
    let entries = parse_worktrees(porcelain).expect("valid porcelain");
    assert_eq!(
        entries,
        [
            Entry {
                path: PathBuf::from("D:/flui"),
                branch: Some("main".to_owned()),
                ..Entry::default()
            },
            Entry {
                path: PathBuf::from("D:/flui/.worktrees/a"),
                branch: Some("tooling/a".to_owned()),
                locked: true,
                ..Entry::default()
            },
            Entry {
                path: PathBuf::from("C:/elsewhere/b"),
                prunable: true,
                ..Entry::default()
            },
            Entry {
                path: PathBuf::from("D:/bare"),
                bare: true,
                ..Entry::default()
            },
        ]
    );
    assert!(parse_worktrees("HEAD abc\n").is_err());
}

fn an_upstream_is_gone_only_when_set_and_missing() {
    let refs = "\
refs/heads/main\0refs/remotes/origin/main
refs/heads/merged-pr\0refs/remotes/origin/merged-pr
refs/heads/local-only\0
refs/heads/pushed\0refs/remotes/origin/pushed
refs/remotes/origin/main\0
refs/remotes/origin/pushed\0
";
    let upstreams = parse_upstreams(refs);
    assert_eq!(
        upstreams.into_iter().collect::<Vec<_>>(),
        [
            ("local-only".to_owned(), false),
            ("main".to_owned(), false),
            ("merged-pr".to_owned(), true),
            ("pushed".to_owned(), false),
        ]
    );
}

fn status_reads_tasks_md_alone_as_no_work() {
    assert_eq!(Changes::from_status(""), Changes::None);
    assert_eq!(Changes::from_status("?? TASKS.md\0"), Changes::TasksOnly);
    assert_eq!(
        Changes::from_status("?? TASKS.md\0?? notes.txt\0"),
        Changes::Work
    );
    assert_eq!(Changes::from_status("?? docs/TASKS.md\0"), Changes::Work);
    assert_eq!(Changes::from_status(" M src/lib.rs\0"), Changes::Work);
    assert_eq!(Changes::from_status("R  new.rs\0old.rs\0"), Changes::Work);
}

const MERGED: Option<Tip> = Some(Tip {
    reachable: true,
    first_parent: false,
    ahead: 0,
    upstream_gone: true,
});
/// What `worktree new` leaves: the tip is origin/main's own head.
const UNSTARTED: Option<Tip> = Some(Tip {
    reachable: true,
    first_parent: true,
    ahead: 0,
    upstream_gone: false,
});
const AHEAD: Option<Tip> = Some(Tip {
    reachable: false,
    first_parent: false,
    ahead: 2,
    upstream_gone: true,
});

fn facts(role: Role, tip: Option<Tip>, changes: Changes) -> Facts {
    Facts {
        role,
        tip,
        changes,
        locked: false,
        missing: false,
    }
}

fn a_tip_is_merged_only_off_mains_first_parent_chain() {
    let tip = |reachable, first_parent| Tip {
        reachable,
        first_parent,
        ahead: u64::from(!reachable),
        upstream_gone: false,
    };
    let cases = [
        (
            tip(true, false),
            Integration::Merged {
                upstream_gone: false,
            },
        ),
        (
            tip(true, true),
            Integration::Unstarted {
                upstream_gone: false,
            },
        ),
        (
            tip(false, false),
            Integration::Unmerged {
                ahead: 1,
                upstream_gone: false,
            },
        ),
    ];
    for (tip, expected) in cases {
        assert_eq!(tip.integration(), expected, "{tip:?}");
    }
    assert_eq!(Integration::of(None), Integration::Detached);
}

fn only_a_clean_merged_ordinary_worktree_is_removed() {
    let cases = [
        (
            facts(Role::Other, UNSTARTED, Changes::None),
            Decision::Keep(Reason::Unstarted),
        ),
        (
            facts(Role::Other, UNSTARTED, Changes::TasksOnly),
            Decision::Keep(Reason::Unstarted),
        ),
        (
            facts(Role::Other, UNSTARTED, Changes::Work),
            Decision::Keep(Reason::Dirty),
        ),
        (
            facts(Role::Other, MERGED, Changes::None),
            Decision::Remove { force: false },
        ),
        (
            facts(Role::Other, MERGED, Changes::TasksOnly),
            Decision::Remove { force: true },
        ),
        (
            facts(Role::Main, MERGED, Changes::None),
            Decision::Keep(Reason::MainCheckout),
        ),
        (
            facts(Role::Current, MERGED, Changes::None),
            Decision::Keep(Reason::Current),
        ),
        (
            facts(Role::Other, MERGED, Changes::Work),
            Decision::Keep(Reason::Dirty),
        ),
        (
            facts(Role::Other, AHEAD, Changes::None),
            Decision::Keep(Reason::Unmerged {
                ahead: 2,
                upstream_gone: true,
            }),
        ),
        (
            facts(Role::Other, AHEAD, Changes::Work),
            Decision::Keep(Reason::Dirty),
        ),
        (
            facts(Role::Other, None, Changes::None),
            Decision::Keep(Reason::Detached),
        ),
        (
            Facts {
                locked: true,
                ..facts(Role::Other, MERGED, Changes::None)
            },
            Decision::Keep(Reason::Locked),
        ),
        (
            Facts {
                missing: true,
                ..facts(Role::Other, MERGED, Changes::None)
            },
            Decision::Keep(Reason::Missing),
        ),
    ];
    for (facts, expected) in cases {
        assert_eq!(classify(&facts), expected, "{facts:?}");
    }
}

/// A repository with an `origin` remote, in a scratch directory.
struct Fixture {
    _scratch: ScratchDir,
    main: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let scratch = ScratchDir::new("worktree").expect("scratch dir");
        let origin = scratch.path().join("origin.git");
        let main = scratch.path().join("main");
        let top = Git::new(scratch.path().to_path_buf());
        top.run(&[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            utf8(&origin).expect("utf8"),
        ])
        .expect("init origin");
        top.run(&["init", "-q", "-b", "main", utf8(&main).expect("utf8")])
            .expect("init main");
        let fixture = Self {
            _scratch: scratch,
            main,
        };
        let git = fixture.git();
        for (key, value) in [
            ("user.name", "t"),
            ("user.email", "t@example.invalid"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
        ] {
            git.run(&["config", key, value]).expect("config");
        }
        std::fs::write(fixture.main.join(".gitignore"), "/target/\n").expect("write");
        git.run(&["add", ".gitignore"]).expect("add");
        commit(&fixture.main, "base");
        git.run(&["remote", "add", "origin", utf8(&origin).expect("utf8")])
            .expect("remote");
        git.run(&["push", "-q", "origin", "main"]).expect("push");
        fixture
    }

    fn git(&self) -> Git {
        Git::new(self.main.clone())
    }

    fn new_worktree(&self, name: &str) -> PathBuf {
        new(&self.git(), &BranchName::parse(name).expect("valid")).expect("worktree new")
    }

    /// Merges `branch` into main with a merge commit, as a pull request
    /// lands, and pushes it.
    fn merge(&self, branch: &str) {
        let git = self.git();
        git.run(&["merge", "-q", "--no-ff", "-m", branch, branch])
            .expect("merge");
        git.run(&["push", "-q", "origin", "main"]).expect("push");
    }

    fn survey_branch(&self, name: &str) -> Worktree {
        survey(&self.git())
            .expect("survey")
            .into_iter()
            .find(|w| w.entry.branch.as_deref() == Some(name))
            .expect("the branch has a worktree")
    }

    fn has_branch(&self, name: &str) -> bool {
        self.git()
            .succeeds(&[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{name}"),
            ])
            .expect("show-ref")
    }
}

/// Commits a new file `name` in the checkout at `dir`.
fn commit(dir: &Path, name: &str) {
    std::fs::write(dir.join(name), name).expect("write");
    let git = Git::new(dir.to_path_buf());
    git.run(&["add", name]).expect("add");
    git.run(&["commit", "-qm", name]).expect("commit");
}

fn new_then_prune_removes_only_merged_clean_worktrees() {
    let fixture = Fixture::new();
    let merged = fixture.new_worktree("t/merged");
    assert_eq!(merged, fixture.main.join(ROOT_DIR).join("merged"));
    commit(&merged, "merged.txt");
    let tasks = fixture.new_worktree("t/tasks");
    commit(&tasks, "tasks.txt");
    std::fs::write(tasks.join(TASKS_FILE), "- [ ] x\n").expect("write");
    fixture.merge("t/merged");
    fixture.merge("t/tasks");
    // created after the merges, so its tip is origin/main's head
    let fresh = fixture.new_worktree("t/fresh");
    let target = merged.join("target");
    std::fs::create_dir(&target).expect("mkdir");
    std::fs::write(target.join("blob"), [0_u8; 64]).expect("write");
    let ahead = fixture.new_worktree("t/ahead");
    commit(&ahead, "ahead.txt");
    let dirty = fixture.new_worktree("t/dirty");
    std::fs::write(dirty.join("notes.txt"), "wip").expect("write");

    assert!(
        new(
            &fixture.git(),
            &BranchName::parse("t/merged").expect("valid")
        )
        .is_err(),
        "an existing branch and path are refused"
    );

    let listed = survey(&fixture.git()).expect("survey");
    assert_eq!(listed.len(), 6);
    assert!(listed.iter().all(|w| !w.outside_root));

    let dry = prune(&fixture.git(), true).expect("dry run");
    assert!(!dry.failed);
    assert!(
        merged.is_dir() && tasks.is_dir(),
        "a dry run changes nothing"
    );
    assert_eq!(
        dry.lines
            .iter()
            .filter(|l| l.starts_with("would remove"))
            .count(),
        2,
        "{:?}",
        dry.lines
    );

    let real = prune(&fixture.git(), false).expect("prune");
    assert!(!real.failed, "{:?}", real.lines);
    assert!(!merged.exists() && !tasks.exists());
    assert!(!fixture.has_branch("t/merged") && !fixture.has_branch("t/tasks"));
    assert!(ahead.is_dir() && dirty.join("notes.txt").is_file());
    assert!(fixture.has_branch("t/ahead") && fixture.has_branch("t/dirty"));
    assert!(fresh.is_dir() && fixture.has_branch("t/fresh"));
    assert!(fixture.main.is_dir());
}

fn a_new_worktree_survives_prune() {
    let fixture = Fixture::new();
    let fresh = fixture.new_worktree("t/fresh");
    let report = prune(&fixture.git(), false).expect("prune");
    assert!(!report.failed, "{:?}", report.lines);
    assert!(fresh.is_dir() && fixture.has_branch("t/fresh"));
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.starts_with("kept:") && l.contains("own commits")),
        "{:?}",
        report.lines
    );
}

fn a_commit_made_after_the_verdict_keeps_the_branch() {
    let fixture = Fixture::new();
    let raced = fixture.new_worktree("t/raced");
    commit(&raced, "raced.txt");
    fixture.merge("t/raced");
    let worktree = fixture.survey_branch("t/raced");
    assert_eq!(classify(&worktree.facts), Decision::Remove { force: false });
    // someone commits between the survey and the removal; the worktree stays clean
    commit(&raced, "late.txt");
    let late = fixture
        .git()
        .run(&["rev-parse", "refs/heads/t/raced"])
        .expect("rev-parse");
    let removed = remove(&fixture.git(), &worktree, false).expect("remove");
    assert!(!raced.exists());
    assert!(removed.branch_kept.is_some(), "the branch was deleted");
    assert!(fixture.has_branch("t/raced"));
    assert_eq!(
        fixture
            .git()
            .run(&["rev-parse", "refs/heads/t/raced"])
            .expect("rev-parse"),
        late
    );
}

fn a_dry_run_fetches_nothing() {
    let fixture = Fixture::new();
    let git = fixture.git();
    // a remote-tracking ref origin lacks: `fetch --prune` would delete it
    git.run(&["update-ref", "refs/remotes/origin/ghost", "HEAD"])
        .expect("update-ref");
    let report = prune(&git, true).expect("dry run");
    assert!(
        git.succeeds(&[
            "show-ref",
            "--verify",
            "--quiet",
            "refs/remotes/origin/ghost"
        ])
        .expect("show-ref"),
        "the dry run pruned remote-tracking refs"
    );
    assert!(
        !fixture.main.join(".git").join("FETCH_HEAD").exists(),
        "the dry run fetched"
    );
    assert!(
        report.lines.iter().any(|l| l.contains("not fetched")),
        "{:?}",
        report.lines
    );
}

#[test]
fn worktree_contract() {
    crate::table_test::run_table(
        "worktree_contract",
        &[
            (
                "accepts_lowercase_kebab_area_and_slug",
                accepts_lowercase_kebab_area_and_slug as fn(),
            ),
            ("rejects_every_other_shape", rejects_every_other_shape),
            (
                "the_root_is_dot_worktrees_in_the_main_checkout",
                the_root_is_dot_worktrees_in_the_main_checkout,
            ),
            ("porcelain_records_parse", porcelain_records_parse),
            (
                "an_upstream_is_gone_only_when_set_and_missing",
                an_upstream_is_gone_only_when_set_and_missing,
            ),
            (
                "status_reads_tasks_md_alone_as_no_work",
                status_reads_tasks_md_alone_as_no_work,
            ),
            (
                "a_tip_is_merged_only_off_mains_first_parent_chain",
                a_tip_is_merged_only_off_mains_first_parent_chain,
            ),
            (
                "only_a_clean_merged_ordinary_worktree_is_removed",
                only_a_clean_merged_ordinary_worktree_is_removed,
            ),
            (
                "new_then_prune_removes_only_merged_clean_worktrees",
                new_then_prune_removes_only_merged_clean_worktrees,
            ),
            (
                "a_new_worktree_survives_prune",
                a_new_worktree_survives_prune,
            ),
            (
                "a_commit_made_after_the_verdict_keeps_the_branch",
                a_commit_made_after_the_verdict_keeps_the_branch,
            ),
            ("a_dry_run_fetches_nothing", a_dry_run_fetches_nothing),
        ],
    );
}
