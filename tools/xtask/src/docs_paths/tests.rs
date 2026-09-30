use super::*;

/// A checkout with a facade, two crates and a doc in each place the scan reads.
fn known() -> Known {
    Known::new(
        [
            "Cargo.toml",
            "llms.txt",
            "README.md",
            "src/lib.rs",
            "docs/testing.md",
            "docs/adr/ADR-0081-tiers.md",
            "crates/flui-view/Cargo.toml",
            "crates/flui-view/ARCHITECTURE.md",
            "crates/flui-view/src/lib.rs",
            "crates/flui-view/src/platforms/mod.rs",
            "crates/flui-view/tests/main.rs",
            "crates/flui-view/docs/NOTES.md",
            "crates/flui-app/Cargo.toml",
            "crates/flui-app/tests/realm.rs",
        ]
        .map(str::to_owned),
    )
}

fn a_path_needs_a_known_root_and_a_slash() {
    for (span, want) in [
        (
            "crates/flui-view/ARCHITECTURE.md",
            &["crates/flui-view/ARCHITECTURE.md"][..],
        ),
        ("docs/adr/", &["docs/adr/"]),
        // `.` and `..` resolve, so a stale path behind them is still checked
        ("docs/./adr/../testing.md", &["docs/testing.md"]),
        ("docs/../removed.md", &["removed.md"]),
        ("crates/flui-view/../flui-app/", &["crates/flui-app/"]),
        // a misspelt crate is this repository's path, not another's
        (
            "crates/fluu-view/src/lib.rs",
            &["crates/fluu-view/src/lib.rs"],
        ),
        (
            ".rust-studio/specs/x/plan.md",
            &[".rust-studio/specs/x/plan.md"],
        ),
        (
            "see docs/testing.md and crates/flui-view",
            &["docs/testing.md", "crates/flui-view"],
        ),
        // identifiers, module paths, types, a bare root, an unknown root
        ("flui_view::element::Element", &[]),
        ("Cargo.toml", &[]),
        ("crates/", &[]),
        ("target/debug/xtask", &[]),
        ("a/b", &[]),
        ("https://example.com/docs/x.md", &[]),
    ] {
        assert_eq!(extract::paths(span), want, "{span:?}");
    }
}

fn a_path_loses_its_line_anchor_and_item_suffix() {
    for (span, want) in [
        (
            "crates/flui-view/src/lib.rs:42",
            "crates/flui-view/src/lib.rs",
        ),
        (
            "crates/flui-view/src/lib.rs:42:7",
            "crates/flui-view/src/lib.rs",
        ),
        (
            "crates/flui-view/src/lib.rs:346-366",
            "crates/flui-view/src/lib.rs",
        ),
        ("docs/testing.md#the-harness", "docs/testing.md"),
        (
            "crates/flui-view/src/lib.rs::Element",
            "crates/flui-view/src/lib.rs",
        ),
        ("(docs/testing.md),", "docs/testing.md"),
        ("\"docs/testing.md\"", "docs/testing.md"),
    ] {
        assert_eq!(extract::paths(span), [want], "{span:?}");
    }
}

fn a_pattern_a_placeholder_or_a_foreign_layout_is_not_a_path() {
    for span in [
        "crates/*/ARCHITECTURE.md",
        "docs/adr/ADR-NNNN-*.md",
        "crates/flui-view/src/{a,b}.rs",
        "crates/<name>/src/lib.rs",
        "crates/$CRATE/src",
        "crates/…/src",
        "crates/.../src",
        "docs//x.md",
        // climbing above the root, or resolving to the root itself
        "docs/../../x.md",
        "docs/..",
        "src/semantics/semantics.dart",
        "packages/flutter/lib/src/rendering/object.dart",
        "packages/flutter_test/lib/x",
        "crates/gpui/src/window.rs",
        "crates/gpui_macos/src/display_link.rs",
        "crates/bevy_animation/src/lib.rs",
    ] {
        assert_eq!(extract::paths(span), [] as [&str; 0], "{span:?}");
    }
}

fn packages_are_read_only_from_cargo_commands() {
    let (test, build) = (Some("test"), Some("build"));
    for (code, want) in [
        ("cargo test -p flui-view", &[(0, test, "flui-view")][..]),
        (
            "cargo nextest run --package flui-app",
            &[(0, Some("nextest"), "flui-app")],
        ),
        ("cargo build -p a -p b", &[(0, build, "a"), (0, build, "b")]),
        (
            "cargo run --package=flui-cli -p=flui-app",
            &[(0, Some("run"), "flui-cli"), (0, Some("run"), "flui-app")],
        ),
        ("cargo test -pflui-view", &[(0, test, "flui-view")]),
        (
            "cargo update -p wgpu@25.0.0",
            &[(0, Some("update"), "wgpu")],
        ),
        ("cargo +nightly miri test -p a", &[(0, Some("miri"), "a")]),
        ("cargo --locked test -p a", &[(0, test, "a")]),
        ("cargo -p a test", &[(0, None, "a")]),
        (
            "cargo --color always update -p w",
            &[(0, Some("update"), "w")],
        ),
        ("cargo --config x=1 -Z y test -p a", &[(0, test, "a")]),
        (
            "cargo test --package='a' -p\"b\"",
            &[(0, test, "a"), (0, test, "b")],
        ),
        (
            "~/.cargo/bin/cargo test -p flui-view",
            &[(0, test, "flui-view")],
        ),
        ("cargo test \\\n  -p flui-view", &[(1, test, "flui-view")]),
        (
            "RUSTFLAGS=x; cargo build -p flui-view",
            &[(0, build, "flui-view")],
        ),
        ("x\ncargo test -p flui-view", &[(1, test, "flui-view")]),
        ("A=1 B=2 cargo test -p a", &[(0, test, "a")]),
        (
            "$env:RUSTFLAGS='-C x'; cargo build -p a",
            &[(0, build, "a")],
        ),
        (
            "cargo run -p flui-cli -- -p 8080",
            &[(0, Some("run"), "flui-cli")],
        ),
        // `env` runs the command after its options and assignments
        ("env RUSTFLAGS=x cargo test -p a", &[(0, test, "a")]),
        ("env -i -u X -C dir cargo build -p a", &[(0, build, "a")]),
        ("/usr/bin/env cargo test -p a", &[(0, test, "a")]),
        // a malformed name is taken as written, for the check to reject
        (
            "cargo test -p definitely.missing",
            &[(0, test, "definitely.missing")],
        ),
        // not cargo, or cargo's command ended
        ("env echo cargo test -p gone", &[]),
        ("env RUSTFLAGS=x", &[]),
        ("mkdir -p target/x", &[]),
        ("cargo build && mkdir -p out", &[]),
        ("cargo build&& mkdir -p out", &[]),
        ("cargo build||mkdir -p out", &[]),
        ("cargo build|grep -p x", &[]),
        ("cargo build; mkdir -p out", &[]),
        ("cargo build | grep -p x", &[]),
        ("cargo test\nmkdir -p out", &[]),
        ("rg -p flui-view", &[]),
        ("echo cargo test -p gone", &[]),
        ("echo \"cargo test -p gone\"", &[]),
        ("# cargo test -p gone", &[]),
        ("cargo test --profile ci", &[]),
        // placeholders
        ("cargo test -p <crate>", &[]),
        ("cargo test -p $CRATE", &[]),
        ("cargo test -p {name}", &[]),
        ("cargo test -p …", &[]),
    ] {
        let selected = extract::packages(code);
        let got: Vec<(usize, Option<&str>, &str)> = selected
            .iter()
            .map(|selected| {
                let subcommand = selected.subcommand.as_deref();
                (selected.line, subcommand, selected.name.as_str())
            })
            .collect();
        assert_eq!(got, want, "{code:?}");
    }
}

fn a_lockfile_package_is_selected_only_by_update_and_tree() {
    let packages = Packages {
        local: ["flui-view".to_owned()].into(),
        locked: ["wgpu".to_owned()].into(),
    };
    for (code, selects) in [
        ("cargo test -p flui-view", true),
        ("cargo update -p wgpu", true),
        ("cargo tree -p wgpu", true),
        ("cargo pkgid -p wgpu", true),
        ("cargo clean -p wgpu", true),
        ("cargo test -p wgpu", false),
        ("cargo -p wgpu", false),
        ("cargo update -p flui-types", false),
    ] {
        let selected = extract::packages(code);
        assert_eq!(selected.len(), 1, "{code:?}");
        assert_eq!(packages.selects(&selected[0]), selects, "{code:?}");
    }
}

fn headings_give_github_anchors() {
    let markdown = "# Start here\n## The `View` tree: a guide!\n## Start here\n\
                    ## Custom {#own-id}\n\n```\n# not a heading\n```\n\
                    # Foo\n# Foo-1\n# Foo\n";
    // GitHub renders `{#own-id}` as text; it is no anchor of its own. The
    // second `Foo` takes `foo-2`: `foo-1` is a heading's already
    let want: BTreeSet<String> = [
        "custom-own-id",
        "foo",
        "foo-1",
        "foo-2",
        "start-here",
        "start-here-1",
        "the-view-tree-a-guide",
    ]
    .map(str::to_owned)
    .into();
    assert_eq!(extract::anchors(markdown), want);
}

fn code_spans_and_blocks_carry_their_lines() {
    // a code span labelling a permalink to a commit cites the file as it was
    // then; any other link's label, a branch (or `main` misspelt) too, is still
    // a path to check
    let markdown = "# T\n\nSee `docs/x.md`.\n\n```bash\ncargo test\ncargo run -p a\n```\n\n    indented\n\n\
                    [l](docs/y.md) ![i](/z.png) \
                    [`docs/old.md`](https://github.com/vanyastaff/flui/blob/e30ab71/docs/old.md) \
                    [`docs/now.md`](https://github.com/vanyastaff/flui/blob/mian/docs/now.md) \
                    [`docs/testng.md`](docs/testing.md)\n";
    let code = extract::code(markdown);
    assert_eq!(
        code,
        [
            extract::Code {
                line: 3,
                text: "docs/x.md".to_owned(),
                block: false,
            },
            extract::Code {
                line: 6,
                text: "cargo test\ncargo run -p a\n".to_owned(),
                block: true,
            },
            extract::Code {
                line: 10,
                text: "indented\n".to_owned(),
                block: true,
            },
            extract::Code {
                line: 12,
                text: "docs/now.md".to_owned(),
                block: false,
            },
            extract::Code {
                line: 12,
                text: "docs/testng.md".to_owned(),
                block: false,
            },
        ]
    );
    assert_eq!(
        extract::links(markdown),
        [
            (12, "docs/y.md".to_owned()),
            (12, "/z.png".to_owned()),
            (
                12,
                "https://github.com/vanyastaff/flui/blob/e30ab71/docs/old.md".to_owned()
            ),
            (
                12,
                "https://github.com/vanyastaff/flui/blob/mian/docs/now.md".to_owned()
            ),
            (12, "docs/testing.md".to_owned()),
        ]
    );
}

fn a_path_resolves_from_the_root_the_doc_or_its_package() {
    let known = known();
    let doc = "crates/flui-view/docs/NOTES.md";
    for (path, resolves) in [
        ("crates/flui-view/src/lib.rs", true),
        ("docs/testing.md", true),
        // the doc's directory, the package, the package's `src/`
        ("docs/NOTES.md", true),
        ("tests/main.rs", true),
        ("src/platforms/mod.rs", true),
        // a package's layout, in whichever package has it
        ("tests/realm.rs", true),
        ("src/lib.rs", true),
        // a directory, and one asked for as a directory
        ("crates/flui-view/src", true),
        ("crates/flui-view/src/", true),
        ("crates/flui-view/src/lib.rs/", false),
        // gone, a different case, and a layout path no package has
        ("crates/flui-types/src/lib.rs", false),
        ("crates/flui-view/src/Lib.rs", false),
        ("tests/gone.rs", false),
        ("docs/gone/", false),
    ] {
        assert_eq!(known.resolves(doc, path), resolves, "{path:?}");
    }
    // outside a package, only the root and the doc's directory, and package layouts
    assert!(!known.resolves("docs/testing.md", "platforms/mod.rs"));
    assert!(known.resolves("docs/testing.md", "adr/ADR-0081-tiers.md"));
}

fn an_llms_link_resolves_like_a_github_link() {
    for (dest, target) in [
        ("docs/testing.md", Some(Some("docs/testing.md"))),
        ("./docs/testing.md#harness", Some(Some("docs/testing.md"))),
        ("/docs/testing.md?plain=1", Some(Some("docs/testing.md"))),
        (
            "https://github.com/vanyastaff/flui/blob/main/docs/testing.md",
            Some(Some("docs/testing.md")),
        ),
        (
            "https://github.com/vanyastaff/flui/tree/main/crates/flui-view",
            Some(Some("crates/flui-view")),
        ),
        ("docs/../README.md", Some(Some("README.md"))),
        ("../README.md", Some(None)),
        // not local
        ("https://example.com/x.md", None),
        ("https://github.com/vanyastaff/flui/issues/1", None),
        ("mailto:a@b.c", None),
        ("//example.com/docs", None),
        // a percent-escaped name is the file's name
        ("docs/review%20probe.md", Some(Some("docs/review probe.md"))),
        ("#start-here", Some(Some("llms.txt"))),
    ] {
        let got = link_target("llms.txt", dest);
        assert_eq!(got.as_ref().map(|path| path.as_deref()), target, "{dest:?}");
    }
}

fn a_doc_reports_each_stale_name_once() {
    let known = known();
    let packages = Packages {
        local: ["flui-view", "flui-app"].map(str::to_owned).into(),
        locked: ["wgpu".to_owned()].into(),
    };
    let read = |path: &str| (path == "docs/testing.md").then(|| "# The harness\n".to_owned());
    let text = "`crates/flui-view/src/lib.rs` `crates/flui-types/` `crates/flui-types/`\n\
                `cargo test -p flui_view`\n\n\
                ```sh\ncargo update -p wgpu\ncargo test -p flui-types\ncargo test -p wgpu\n```\n\n\
                [ok](docs/testing.md) [gone](docs/gone.md) [out](../x.md) [web](https://a.b/)\n\
                [h](docs/testing.md#the-harness) [no](docs/testing.md#no-heading) \
                [dir](crates/flui-view#x) [self](#no-heading) \
                [escaped](docs/testing.md#the%2Dharness)\n";
    let names = |doc: &str| -> Vec<(usize, Kind, String)> {
        stale(doc, text, &known, &packages, &read)
            .into_iter()
            .map(|stale| (stale.line, stale.kind, stale.name))
            .collect()
    };
    let mut want = vec![
        (1, Kind::Path, "crates/flui-types/".to_owned()),
        (2, Kind::Package, "flui_view".to_owned()),
        (6, Kind::Package, "flui-types".to_owned()),
        // a lockfile package, selected by a command that takes members only
        (7, Kind::Package, "wgpu".to_owned()),
    ];
    // Markdown links are lychee's; `llms.txt`'s are this gate's
    assert_eq!(names("README.md"), want);
    want.extend([
        (10, Kind::Link, "docs/gone.md".to_owned()),
        (10, Kind::Link, "../x.md".to_owned()),
        (11, Kind::Link, "docs/testing.md#no-heading".to_owned()),
        (11, Kind::Link, "#no-heading".to_owned()),
    ]);
    want.sort();
    assert_eq!(names("llms.txt"), want);
}

fn the_docs_are_live_markdown_and_llms_txt() {
    let known = Known::new(
        [
            "README.md",
            "llms.txt",
            "notes.txt",
            "CHANGELOG.md",
            "crates/flui-view/CHANGELOG.md",
            "changelog.d/x.md",
            "docs/plans/x.md",
            "docs/plans.md",
            "docs/research/x.md",
            "crates/flui-view/specs/x.md",
        ]
        .map(str::to_owned),
    );
    assert_eq!(
        docs(&known),
        [
            "README.md",
            "crates/flui-view/specs/x.md",
            "docs/plans.md",
            "llms.txt"
        ]
    );
}

fn the_allowlist_counts_exactly() {
    let stale = |doc: &str, kind| Stale {
        doc: doc.to_owned(),
        line: 1,
        kind,
        name: "x".to_owned(),
    };
    let found = [
        stale("a.md", Kind::Path),
        stale("a.md", Kind::Path),
        stale("b.md", Kind::Path),
        stale("c.md", Kind::Package),
        stale("d.md", Kind::Link),
    ];
    let scanned = ["a.md", "b.md", "c.md", "d.md", "e.md"].map(str::to_owned);
    let allow: Allowlist = toml::from_str(
        r#"
        [[allow]]
        path = "a.md"
        kind = "path"
        count = 1
        reason = "grew"
        [[allow]]
        path = "b.md"
        kind = "path"
        count = 2
        reason = "shrank"
        [[allow]]
        path = "c.md"
        kind = "package"
        count = 1
        reason = ""
        [[allow]]
        path = "c.md"
        kind = "package"
        count = 1
        reason = "twice"
        [[allow]]
        path = "e.md"
        kind = "link"
        count = 1
        reason = "nothing left"
        [[allow]]
        path = "gone.md"
        kind = "path"
        count = 1
        reason = "gone"
        [[allow]]
        path = "e.md"
        kind = "anchor"
        count = 1
        reason = "unknown"
        "#,
    )
    .expect("parses");
    let kinds: Vec<String> = judge(&found, &scanned, &allow)
        .iter()
        .map(|finding| {
            let label = match finding {
                Finding::Stale(stale) => format!("stale {}", stale.doc),
                Finding::Grew { doc, .. } => format!("grew {doc}"),
                Finding::Shrank { doc, count, .. } => format!("shrank {doc} to {count}"),
                Finding::Zero { doc, .. } => format!("zero {doc}"),
                Finding::Gone { doc, .. } => format!("gone {doc}"),
                Finding::UnknownKind { doc, .. } => format!("unknown {doc}"),
                Finding::Duplicate { doc, .. } => format!("duplicate {doc}"),
                Finding::NoReason { doc, .. } => format!("no reason {doc}"),
            };
            // every finding prints the file the maintainer edits
            assert!(finding.to_string().contains(".md"), "{finding}");
            label
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "grew a.md",
            "shrank b.md to 1",
            "no reason c.md",
            "duplicate c.md",
            "zero e.md",
            "gone gone.md",
            "unknown e.md",
            "stale d.md",
        ]
    );
}

fn the_seed_is_an_allowlist_that_passes() {
    let found = [
        Stale {
            doc: "a.md".to_owned(),
            line: 1,
            kind: Kind::Path,
            name: "docs/x.md".to_owned(),
        },
        Stale {
            doc: "a.md".to_owned(),
            line: 2,
            kind: Kind::Path,
            name: "docs/y.md".to_owned(),
        },
    ];
    let mut allow: Allowlist = toml::from_str(&seed(&found)).expect("the seed parses");
    assert_eq!(allow.allow.len(), 1);
    allow.allow[0].reason = "filled in".to_owned();
    assert!(judge(&found, &["a.md".to_owned()], &allow).is_empty());
}

#[test]
fn docs_paths_contract() {
    crate::table_test::run_table(
        "docs_paths_contract",
        &[
            (
                "a_path_needs_a_known_root_and_a_slash",
                a_path_needs_a_known_root_and_a_slash as fn(),
            ),
            (
                "a_path_loses_its_line_anchor_and_item_suffix",
                a_path_loses_its_line_anchor_and_item_suffix as fn(),
            ),
            (
                "a_pattern_a_placeholder_or_a_foreign_layout_is_not_a_path",
                a_pattern_a_placeholder_or_a_foreign_layout_is_not_a_path as fn(),
            ),
            (
                "packages_are_read_only_from_cargo_commands",
                packages_are_read_only_from_cargo_commands as fn(),
            ),
            (
                "a_lockfile_package_is_selected_only_by_update_and_tree",
                a_lockfile_package_is_selected_only_by_update_and_tree as fn(),
            ),
            (
                "headings_give_github_anchors",
                headings_give_github_anchors as fn(),
            ),
            (
                "code_spans_and_blocks_carry_their_lines",
                code_spans_and_blocks_carry_their_lines as fn(),
            ),
            (
                "a_path_resolves_from_the_root_the_doc_or_its_package",
                a_path_resolves_from_the_root_the_doc_or_its_package as fn(),
            ),
            (
                "an_llms_link_resolves_like_a_github_link",
                an_llms_link_resolves_like_a_github_link as fn(),
            ),
            (
                "a_doc_reports_each_stale_name_once",
                a_doc_reports_each_stale_name_once as fn(),
            ),
            (
                "the_docs_are_live_markdown_and_llms_txt",
                the_docs_are_live_markdown_and_llms_txt as fn(),
            ),
            (
                "the_allowlist_counts_exactly",
                the_allowlist_counts_exactly as fn(),
            ),
            (
                "the_seed_is_an_allowlist_that_passes",
                the_seed_is_an_allowlist_that_passes as fn(),
            ),
        ],
    );
}
