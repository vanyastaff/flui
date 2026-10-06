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

fn the_main_checkout_is_its_own_top_level_or_a_first_record_holding_git() {
    let scratch = ScratchDir::new("worktree-main").expect("scratch dir");
    let checkout = scratch.path().join("checkout");
    std::fs::create_dir_all(checkout.join(".git")).expect("mkdir");
    let git_dir = scratch.path().join("separate-git-dir");
    std::fs::create_dir_all(&git_dir).expect("mkdir");
    let entry = |path: &Path, bare| Entry {
        path: path.to_path_buf(),
        bare,
        ..Entry::default()
    };
    let linked = entry(&checkout.join(ROOT_DIR).join("a"), false);
    assert_eq!(
        main_checkout(&[entry(&checkout, false), linked.clone()], None).expect("a checkout"),
        checkout
    );
    // a separate git dir reported in the checkout's place
    let separate = [entry(&git_dir, false), linked];
    assert_eq!(
        main_checkout(&separate, Some(&checkout)).expect("the top level wins"),
        checkout
    );
    assert!(main_checkout(&separate, None).is_err());
    assert!(main_checkout(&[entry(&git_dir, true)], None).is_err());
    assert!(main_checkout(&[], None).is_err());
}

fn porcelain_records_parse() {
    // `-z` output: every line NUL-terminated, a record ended by an empty line;
    // paths verbatim, never C-quoted
    let porcelain = [
        "worktree D:/flui",
        "HEAD 1111111111111111111111111111111111111111",
        "branch refs/heads/main",
        "",
        "worktree D:/flui/.worktrees/t\u{e9}st dir",
        "HEAD 2222222222222222222222222222222222222222",
        "branch refs/heads/tooling/a",
        "locked",
        "",
        "worktree C:/elsewhere/b",
        "HEAD 3333333333333333333333333333333333333333",
        "detached",
        "prunable gitdir file points to non-existent location",
        "",
        "worktree D:/bare",
        "bare",
        "",
    ]
    .map(|line| format!("{line}\0"))
    .concat();
    let entries = parse_worktrees(porcelain.as_bytes()).expect("valid porcelain");
    assert_eq!(
        entries,
        [
            Entry {
                path: PathBuf::from("D:/flui"),
                branch: Some("main".to_owned()),
                ..Entry::default()
            },
            Entry {
                path: PathBuf::from("D:/flui/.worktrees/t\u{e9}st dir"),
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
    assert!(parse_worktrees(b"HEAD abc\0").is_err());
}

/// git prints a path's bytes verbatim; on Unix they need not be UTF-8, and
/// `list` and `prune` must still read every record.
fn a_non_utf8_path_is_read_as_the_platform_spells_it() {
    let porcelain = b"worktree /w/dir\xff\0branch refs/heads/t/a\0\0";
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let entries = parse_worktrees(porcelain).expect("a Unix path is any bytes");
        assert_eq!(
            entries,
            [Entry {
                path: PathBuf::from(OsStr::from_bytes(b"/w/dir\xff")),
                branch: Some("t/a".to_owned()),
                ..Entry::default()
            }]
        );
    }
    #[cfg(not(unix))]
    {
        let error = parse_worktrees(porcelain).expect_err("git for Windows prints UTF-8");
        assert!(format!("{error:#}").contains("non-UTF-8 path"), "{error:#}");
    }
    // a non-UTF-8 ignored entry is judged on its bytes and named lossily
    assert_eq!(
        Changes::from_status(b"!! key\xff.env\0!! build/target/\0?? TASKS.md\0"),
        Changes::Ignored(vec!["key\u{fffd}.env".to_owned()])
    );
    assert_eq!(
        Changes::from_status(b"!! target/\0?? src\xff.rs\0"),
        Changes::Work
    );
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
    assert_eq!(Changes::from_status(b""), Changes::None);
    assert_eq!(Changes::from_status(b"?? TASKS.md\0"), Changes::TasksOnly);
    assert_eq!(
        Changes::from_status(b"?? TASKS.md\0?? notes.txt\0"),
        Changes::Work
    );
    assert_eq!(Changes::from_status(b"?? docs/TASKS.md\0"), Changes::Work);
    assert_eq!(Changes::from_status(b" M src/lib.rs\0"), Changes::Work);
    assert_eq!(Changes::from_status(b"R  new.rs\0old.rs\0"), Changes::Work);
}

fn status_keeps_ignored_entries_that_are_not_disposable() {
    let ignored =
        |paths: &[&str]| Changes::Ignored(paths.iter().map(|p| (*p).to_owned()).collect());
    let cases = [
        ("!! target/\0", Changes::None),
        (
            "!! TASKS.md\0!! target/\0!! tools/x/target/\0",
            Changes::None,
        ),
        ("?? TASKS.md\0!! target/\0", Changes::TasksOnly),
        ("!! .env\0", ignored(&[".env"])),
        (
            "!! target/\0!! .env\0!! keys/\0",
            ignored(&[".env", "keys/"]),
        ),
        ("?? TASKS.md\0!! .env\0", ignored(&[".env"])),
        ("!! docs/TASKS.md\0", ignored(&["docs/TASKS.md"])),
        (
            "!! target.bak\0!! mytarget/\0",
            ignored(&["target.bak", "mytarget/"]),
        ),
        ("!! .env\0 M src/lib.rs\0", Changes::Work),
    ];
    for (status, expected) in cases {
        assert_eq!(
            Changes::from_status(status.as_bytes()),
            expected,
            "{status:?}"
        );
    }
    let many = Reason::Ignored(["a", "b", "c", "d", "e"].map(str::to_owned).to_vec());
    assert_eq!(many.to_string(), "ignored files: a, b, c (+2 more)");
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
            facts(
                Role::Other,
                MERGED,
                Changes::Ignored(vec![".env".to_owned()]),
            ),
            Decision::Keep(Reason::Ignored(vec![".env".to_owned()])),
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
        Self::init(&[])
    }

    /// A main checkout whose git dir lives outside it (`--separate-git-dir`).
    fn separate_git_dir() -> Self {
        Self::init(&["--separate-git-dir", "main-git-dir"])
    }

    /// `init_args` go to the main checkout's `git init`, run in the scratch dir.
    fn init(init_args: &[&str]) -> Self {
        let scratch = ScratchDir::new("worktree").expect("scratch dir");
        let origin = scratch.path().join("origin.git");
        let main = scratch.path().join("main");
        let top = Git::new(scratch.path().to_path_buf());
        top.run(&["init", "-q", "--bare", "-b", "main", utf8(&origin)])
            .expect("init origin");
        let mut init = vec!["init", "-q", "-b", "main"];
        init.extend_from_slice(init_args);
        init.push(utf8(&main));
        top.run(&init).expect("init main");
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
        git.run(&["remote", "add", "origin", utf8(&origin)])
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

fn utf8(path: &Path) -> &str {
    path.to_str().expect("scratch paths are UTF-8")
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

fn work_written_after_the_verdict_survives_removal() {
    let fixture = Fixture::new();
    let tasks = fixture.new_worktree("t/tasks");
    commit(&tasks, "tasks.txt");
    fixture.merge("t/tasks");
    std::fs::write(
        tasks.join("TASKS.md"),
        "- [x] done
",
    )
    .expect("write");
    let worktree = fixture.survey_branch("t/tasks");
    assert_eq!(classify(&worktree.facts), Decision::Remove { force: true });
    // someone starts new work between the survey and the removal
    std::fs::write(
        tasks.join("late.txt"),
        "unsaved
",
    )
    .expect("write");
    assert!(remove(&fixture.git(), &worktree, true).is_err());
    assert_eq!(
        std::fs::read_to_string(tasks.join("late.txt")).expect("late work survives"),
        "unsaved
"
    );
    assert!(tasks.join("TASKS.md").exists() && fixture.has_branch("t/tasks"));
}

fn a_kept_worktree_keeps_its_target() {
    let fixture = Fixture::new();
    let busy = fixture.new_worktree("t/busy");
    commit(&busy, "busy.txt");
    fixture.merge("t/busy");
    let worktree = fixture.survey_branch("t/busy");
    assert_eq!(classify(&worktree.facts), Decision::Remove { force: false });
    // new work arrives before removal, while a build is using `target/`
    std::fs::create_dir_all(busy.join("target")).expect("mkdir");
    std::fs::write(busy.join("target").join("blob"), [0_u8; 64]).expect("write");
    std::fs::write(
        busy.join("late.txt"),
        "unsaved
",
    )
    .expect("write");
    assert!(remove(&fixture.git(), &worktree, false).is_err());
    assert!(
        busy.join("target").join("blob").exists(),
        "a kept worktree keeps its build cache"
    );
}

/// A merged worktree with `target/` and, unless `secret` is `None`, an
/// ignored file of that name; returns its path after a real prune.
fn prune_merged_with_ignored(secret: Option<&str>) -> (Fixture, PathBuf, PruneReport) {
    let fixture = Fixture::new();
    let merged = fixture.new_worktree("t/merged");
    commit(&merged, "merged.txt");
    fixture.merge("t/merged");
    let target = merged.join("target");
    std::fs::create_dir(&target).expect("mkdir");
    std::fs::write(target.join("blob"), [0_u8; 64]).expect("write");
    if let Some(secret) = secret {
        let exclude = fixture.main.join(".git").join("info").join("exclude");
        std::fs::write(&exclude, format!("{secret}\n")).expect("write");
        std::fs::write(merged.join(secret), "KEY=1\n").expect("write");
    }
    let report = prune(&fixture.git(), false).expect("prune");
    assert!(!report.failed, "{:?}", report.lines);
    (fixture, merged, report)
}

fn an_ignored_file_keeps_a_merged_worktree() {
    let (fixture, merged, report) = prune_merged_with_ignored(Some(".env"));
    assert!(merged.join(".env").is_file(), "{:?}", report.lines);
    assert!(fixture.has_branch("t/merged"));
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.starts_with("kept:") && l.ends_with("ignored files: .env")),
        "{:?}",
        report.lines
    );
}

fn a_merged_worktree_holding_only_target_is_removed() {
    let (fixture, merged, report) = prune_merged_with_ignored(None);
    assert!(!merged.exists(), "{:?}", report.lines);
    assert!(!fixture.has_branch("t/merged"));
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

/// A fetch that fast-forwards origin/main onto an unmerged branch while the
/// survey runs: the survey judges against the origin/main it started from,
/// not the old first-parent chain paired with the new ancestry.
fn a_fetch_during_the_survey_does_not_merge_a_branch() {
    let fixture = Fixture::new();
    let ahead = fixture.new_worktree("t/ahead");
    commit(&ahead, "ahead.txt");
    let main = fixture.main.clone();
    let fetched = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut git = fixture.git();
    git.after = Some(std::rc::Rc::new({
        let fetched = std::rc::Rc::clone(&fetched);
        move |args: &[String]| {
            if !fetched.get() && args.starts_with(&["rev-list".into(), "--first-parent".into()]) {
                fetched.set(true);
                Git::new(main.clone())
                    .run(&[
                        "update-ref",
                        "refs/remotes/origin/main",
                        "refs/heads/t/ahead",
                    ])
                    .expect("fast-forward origin/main");
            }
        }
    }));
    let worktree = survey(&git)
        .expect("survey")
        .into_iter()
        .find(|w| w.entry.branch.as_deref() == Some("t/ahead"))
        .expect("the branch has a worktree");
    assert!(
        fetched.get(),
        "the survey never read the first-parent chain"
    );
    assert_eq!(
        classify(&worktree.facts),
        Decision::Keep(Reason::Unmerged {
            ahead: 1,
            upstream_gone: false
        })
    );
}

/// `git worktree prune --verbose` names a stale record on stderr; both a dry
/// run and a real prune report it, and only the real one drops it.
fn a_stale_record_is_reported() {
    let fixture = Fixture::new();
    let gone = fixture.new_worktree("t/gone");
    std::fs::remove_dir_all(&gone).expect("rmdir");
    let recorded = || {
        fixture
            .git()
            .run(&["worktree", "list", "--porcelain"])
            .expect("list")
            .contains("/gone")
    };
    for dry_run in [true, false] {
        let report = prune(&fixture.git(), dry_run).expect("prune");
        assert!(!report.failed, "{:?}", report.lines);
        assert!(
            report
                .lines
                .iter()
                .any(|l| l.starts_with("stale: ") && l.contains("gone")),
            "dry run {dry_run}: {:?}",
            report.lines
        );
        assert_eq!(recorded(), dry_run, "dry run {dry_run}");
    }
}

/// Merges a worktree that holds nothing, or only a root `TASKS.md` when
/// `tasks`, surveys it, then writes an ignored `.env` before [`remove`] runs.
fn ignored_file_written_after_the_verdict(tasks: bool) {
    let fixture = Fixture::new();
    let raced = fixture.new_worktree("t/raced");
    commit(&raced, "raced.txt");
    fixture.merge("t/raced");
    if tasks {
        std::fs::write(raced.join(TASKS_FILE), "- [x] done\n").expect("write");
    }
    let worktree = fixture.survey_branch("t/raced");
    assert_eq!(classify(&worktree.facts), Decision::Remove { force: tasks });
    let exclude = fixture.main.join(".git").join("info").join("exclude");
    std::fs::write(&exclude, ".env\n").expect("write");
    std::fs::write(raced.join(".env"), "KEY=1\n").expect("write");
    let error = remove(&fixture.git(), &worktree, tasks).expect_err("removal is refused");
    assert!(
        format!("{error:#}").contains("ignored files: .env"),
        "{error:#}"
    );
    assert_eq!(
        std::fs::read_to_string(raced.join(".env")).expect("the ignored file survives"),
        "KEY=1\n"
    );
    assert!(fixture.has_branch("t/raced"));
    assert_eq!(raced.join(TASKS_FILE).is_file(), tasks);
}

fn an_ignored_file_written_after_the_verdict_survives_removal() {
    ignored_file_written_after_the_verdict(false);
}

fn an_ignored_file_written_beside_tasks_md_survives_removal() {
    ignored_file_written_after_the_verdict(true);
}

/// A merged worktree with a `target/`, surveyed and judged removable.
fn merged_with_target(fixture: &Fixture) -> (PathBuf, Worktree) {
    let merged = fixture.new_worktree("t/merged");
    commit(&merged, "merged.txt");
    fixture.merge("t/merged");
    std::fs::create_dir(merged.join("target")).expect("mkdir");
    std::fs::write(merged.join("target").join("blob"), [0_u8; 64]).expect("write");
    let worktree = fixture.survey_branch("t/merged");
    assert_eq!(classify(&worktree.facts), Decision::Remove { force: false });
    (merged, worktree)
}

/// The `locked` line of the worktree at `path`, `None` when unlocked.
fn lock_of(fixture: &Fixture, path: &Path) -> Option<String> {
    let porcelain = fixture
        .git()
        .run(&["worktree", "list", "--porcelain"])
        .expect("list");
    let name = path.file_name().expect("a name").to_string_lossy();
    porcelain
        .split("\n\n")
        .find(|record| record.lines().next().is_some_and(|l| l.ends_with(&*name)))
        .expect("the worktree is recorded")
        .lines()
        .find_map(|line| {
            line.strip_prefix("locked")
                .map(str::trim)
                .map(str::to_owned)
        })
}

/// An ignored file written after the first recheck, while `target/` is being
/// deleted, is seen by the check made after it and kept; the worktree is left
/// unlocked.
fn an_ignored_file_written_while_target_is_deleted_survives() {
    let fixture = Fixture::new();
    let (merged, worktree) = merged_with_target(&fixture);
    let written = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut git = fixture.git();
    git.after = Some(std::rc::Rc::new({
        let (written, main, merged) = (
            std::rc::Rc::clone(&written),
            fixture.main.clone(),
            merged.clone(),
        );
        move |args: &[String]| {
            if !written.get() && args.first().is_some_and(|a| a == "status") {
                written.set(true);
                let exclude = main.join(".git").join("info").join("exclude");
                std::fs::write(exclude, ".env\n").expect("write");
                std::fs::write(merged.join(".env"), "KEY=1\n").expect("write");
            }
        }
    }));
    let error = remove(&git, &worktree, false).expect_err("removal is refused");
    assert!(written.get(), "removal never checked the worktree");
    assert!(
        format!("{error:#}").contains("ignored files: .env"),
        "{error:#}"
    );
    assert_eq!(
        std::fs::read_to_string(merged.join(".env")).expect("the ignored file survives"),
        "KEY=1\n"
    );
    assert!(fixture.has_branch("t/merged"));
    assert_eq!(lock_of(&fixture, &merged), None, "the lock was released");
}

/// A worktree somebody locks after the survey is not prune's to remove: its
/// `target/` and their lock stay.
fn a_worktree_locked_after_the_verdict_is_kept_whole() {
    let fixture = Fixture::new();
    let (merged, worktree) = merged_with_target(&fixture);
    fixture
        .git()
        .run(&[
            OsStr::new("worktree"),
            OsStr::new("lock"),
            OsStr::new("--reason"),
            OsStr::new("mine"),
            merged.as_os_str(),
        ])
        .expect("lock");
    let error = remove(&fixture.git(), &worktree, false).expect_err("removal is refused");
    assert!(format!("{error:#}").contains("locked"), "{error:#}");
    assert!(
        merged.join("target").join("blob").is_file(),
        "a kept worktree keeps its build cache"
    );
    assert_eq!(lock_of(&fixture, &merged).as_deref(), Some("mine"));
    assert!(fixture.has_branch("t/merged"));
}

fn a_separate_git_dir_checkout_roots_worktrees_in_the_checkout() {
    let fixture = Fixture::separate_git_dir();
    assert!(fixture.main.join(".git").is_file(), "`.git` is a gitfile");
    let created = fixture.new_worktree("t/sep");
    assert_eq!(created, fixture.main.join(ROOT_DIR).join("sep"));
    assert!(created.is_dir());
    assert!(!fixture.survey_branch("t/sep").outside_root);
    // git records the checkout nowhere a linked worktree can read it: refused,
    // not rooted in the git dir
    let from_linked = new(
        &Git::new(created),
        &BranchName::parse("t/other").expect("valid"),
    );
    assert!(from_linked.is_err(), "{from_linked:?}");
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
                "the_main_checkout_is_its_own_top_level_or_a_first_record_holding_git",
                the_main_checkout_is_its_own_top_level_or_a_first_record_holding_git,
            ),
            ("porcelain_records_parse", porcelain_records_parse),
            (
                "a_non_utf8_path_is_read_as_the_platform_spells_it",
                a_non_utf8_path_is_read_as_the_platform_spells_it,
            ),
            (
                "an_upstream_is_gone_only_when_set_and_missing",
                an_upstream_is_gone_only_when_set_and_missing,
            ),
            (
                "status_reads_tasks_md_alone_as_no_work",
                status_reads_tasks_md_alone_as_no_work,
            ),
            (
                "status_keeps_ignored_entries_that_are_not_disposable",
                status_keeps_ignored_entries_that_are_not_disposable,
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
            (
                "work_written_after_the_verdict_survives_removal",
                work_written_after_the_verdict_survives_removal,
            ),
            (
                "a_kept_worktree_keeps_its_target",
                a_kept_worktree_keeps_its_target,
            ),
            (
                "an_ignored_file_keeps_a_merged_worktree",
                an_ignored_file_keeps_a_merged_worktree,
            ),
            (
                "a_merged_worktree_holding_only_target_is_removed",
                a_merged_worktree_holding_only_target_is_removed,
            ),
            ("a_dry_run_fetches_nothing", a_dry_run_fetches_nothing),
            ("a_stale_record_is_reported", a_stale_record_is_reported),
            (
                "a_fetch_during_the_survey_does_not_merge_a_branch",
                a_fetch_during_the_survey_does_not_merge_a_branch,
            ),
            (
                "an_ignored_file_written_after_the_verdict_survives_removal",
                an_ignored_file_written_after_the_verdict_survives_removal,
            ),
            (
                "an_ignored_file_written_beside_tasks_md_survives_removal",
                an_ignored_file_written_beside_tasks_md_survives_removal,
            ),
            (
                "an_ignored_file_written_while_target_is_deleted_survives",
                an_ignored_file_written_while_target_is_deleted_survives,
            ),
            (
                "a_worktree_locked_after_the_verdict_is_kept_whole",
                a_worktree_locked_after_the_verdict_is_kept_whole,
            ),
            (
                "a_separate_git_dir_checkout_roots_worktrees_in_the_checkout",
                a_separate_git_dir_checkout_roots_worktrees_in_the_checkout,
            ),
        ],
    );
}
