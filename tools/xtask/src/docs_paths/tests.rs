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
        // a name that only starts like another repository's is this one's
        (
            "packages/flutterish/README.md",
            &["packages/flutterish/README.md"],
        ),
        (
            "crates/gpuii/src/window.rs",
            &["crates/gpuii/src/window.rs"],
        ),
        // any filename character
        ("docs/不存在.md", &["docs/不存在.md"]),
        ("docs/foo+bar.md", &["docs/foo+bar.md"]),
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
        (
            "crates/flui-view/src/lib.rs:53,67",
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
        // a subshell groups commands
        ("(cargo test -p a)", &[(0, test, "a")]),
        ("x && (cd y; cargo build -p a)", &[(0, build, "a")]),
        // reserved words before a command
        ("{ cargo test -p a; }", &[(0, test, "a")]),
        ("if cargo test -p a; then x; fi", &[(0, test, "a")]),
        ("! cargo build -p a", &[(0, build, "a")]),
        // a here-document's body is data; the command after it is read
        (
            "cat <<'EOF'\ncargo test -p gone\nEOF\ncargo build -p a",
            &[(3, build, "a")],
        ),
        (
            "cat <<-EOF > x\n\tcargo test -p gone\n\tEOF\ncargo build -p a",
            &[(3, build, "a")],
        ),
        ("cat <<< x; cargo build -p a", &[(0, build, "a")]),
        // an unquoted here-document runs its command substitutions
        ("cat <<EOF\nx $(cargo test -p a)\nEOF", &[(1, test, "a")]),
        ("cat <<EOF\n`cargo build -p a`\nEOF", &[(1, build, "a")]),
        ("cat <<'EOF'\n$(cargo test -p gone)\nEOF", &[]),
        // a quoted `)` does not close a substitution
        (
            "cat <<EOF\n$(printf '%s' ')' ; cargo test -p a)\nEOF",
            &[(1, test, "a")],
        ),
        ("echo $(printf ')' ; cargo build -p a)", &[(0, build, "a")]),
        // ANSI-C quoting, `sudo`, and an appending assignment
        ("cargo test -p $'a'", &[(0, test, "a")]),
        // every ANSI-C escape form: hex, octal, Unicode
        ("cargo test -p $'flui\\x2dview'", &[(0, test, "flui-view")]),
        ("cargo test -p $'flui\\055view'", &[(0, test, "flui-view")]),
        (
            "cargo test -p $'flui\\u002dview'",
            &[(0, test, "flui-view")],
        ),
        // a `case` pattern's `)` does not close the substitution around it
        (
            "echo \"$(case x in x) cargo test -p a;; esac)\"",
            &[(0, test, "a")],
        ),
        // PowerShell's backtick continues the line
        ("cargo test `\n  -p a", &[(1, test, "a")]),
        ("sudo cargo test -p a", &[(0, test, "a")]),
        ("sudo -u root -E cargo build -p a", &[(0, build, "a")]),
        ("X+=y cargo test -p a", &[(0, test, "a")]),
        // an escape missing its digits stays as written
        (
            "cargo test -p $'flui\\x-view'",
            &[(0, test, "flui\\x-view")],
        ),
        // a comment's `esac` closes no `case`
        (
            "echo \"$(case x # esac\nin x) cargo test -p a;; esac)\"",
            &[(1, test, "a")],
        ),
        // an ANSI-C here-document delimiter
        ("cat <<$'END'\nx\nEND\ncargo build -p a", &[(3, build, "a")]),
        // a redirection target's substitution runs; a named descriptor is dropped
        ("echo >\"$(cargo test -p a)\"", &[(0, test, "a")]),
        ("{fd}>build.log cargo test -p a", &[(0, test, "a")]),
        // sudo's long options take their value too
        ("sudo --user root cargo test -p a", &[(0, test, "a")]),
        ("sudo -R /newroot cargo test -p a", &[(0, test, "a")]),
        (
            "sudo --chroot /newroot cargo build -p a",
            &[(0, build, "a")],
        ),
        // an escaped quote inside `$'…'` within a substitution
        (
            "echo \"$(printf '%s' $'\\')' ; cargo test -p a)\"",
            &[(0, test, "a")],
        ),
        // a keyword function's body runs when it is called
        (
            "function check { cargo test -p a; }; check",
            &[(0, test, "a")],
        ),
        // an array's elements are words, not a command
        ("args=(cargo test -p gone)", &[]),
        (
            "args+=(cargo test -p gone); cargo build -p a",
            &[(0, build, "a")],
        ),
        // …but a substitution among its elements runs
        ("args=($(cargo test -p a))", &[(0, test, "a")]),
        // a single-quoted redirection target is inert
        ("echo >'$(cargo test -p gone)'", &[]),
        // `nohup` runs its command
        ("nohup cargo test -p a &", &[(0, test, "a")]),
        // `env -S` splits arguments, not shell commands: `;` stays in the word
        (
            "env -S 'cargo test -p flui-view;'",
            &[(0, test, "flui-view;")],
        ),
        // an escaped quote inside a quoted here-document delimiter
        (
            "cat <<\"E\\\"OF\"\nx\nE\"OF\ncargo build -p a",
            &[(3, build, "a")],
        ),
        // a descriptor before `<<` is no program
        ("3<<EOF cargo test -p a\nx\nEOF", &[(0, test, "a")]),
        // a backslash-newline inside a here-document delimiter continues it
        (
            "cat <<EO\\\nF\nx\nEOF\ncargo build -p a",
            &[(4, build, "a")],
        ),
        // an unquoted body joins a backslash-newline before the terminator
        // check; a quoted one keeps its lines as written
        ("cat <<EOF\nEO\\\nF\ncargo build -p a", &[(3, build, "a")]),
        (
            "cat <<'EOF'\nEO\\\nF\nEOF\ncargo build -p a",
            &[(4, build, "a")],
        ),
        // a comment may follow an operator directly
        ("echo \"$(true;# )\ncargo test -p a\n)\"", &[(1, test, "a")]),
        // value-taking options inside a cluster
        ("env -iS 'cargo test -p a'", &[(0, test, "a")]),
        ("env -iu X cargo test -p a", &[(0, test, "a")]),
        ("sudo -Eu root cargo test -p a", &[(0, test, "a")]),
        // a shell's `-c` script is read
        ("sh -c 'cargo test -p a'", &[(0, test, "a")]),
        ("bash -lc \"cargo build -p a\"", &[(0, build, "a")]),
        ("bash script.sh -p x", &[]),
        ("bash -O extglob -c 'cargo test -p a'", &[(0, test, "a")]),
        ("bash -- -c 'cargo test -p gone'", &[]),
        ("nohup -- cargo test -p a", &[(0, test, "a")]),
        // an escaped quote keeps a redirection target whole
        ("echo >\"foo\\\"; cargo test -p gone\"", &[]),
        // help, version, list and explain exit before any selection
        ("cargo --version test -p gone", &[]),
        ("cargo test -p gone --help", &[]),
        ("cargo --explain E0001", &[]),
        ("cargo test -p a -- --help", &[(0, test, "a")]),
        ("cat <<\\EOF\n$(cargo test -p gone)\nEOF", &[]),
        (
            "cat <<'END MARK'\ncargo test -p gone\nEND MARK\ncargo build -p a",
            &[(3, build, "a")],
        ),
        // a command substitution is a command of its own
        ("echo \"$(cargo test -p a)\"", &[(0, test, "a")]),
        ("x=`cargo tree -p a`", &[(0, Some("tree"), "a")]),
        ("echo $(cd y && cargo build -p a)", &[(0, build, "a")]),
        // redirections and their targets are not words
        (">build.log cargo test -p a", &[(0, test, "a")]),
        ("cargo test -p a 2>&1 | tee log", &[(0, test, "a")]),
        ("cargo test -p a &>log", &[(0, test, "a")]),
        ("cargo build -p a < in", &[(0, build, "a")]),
        // a `<name>` placeholder is a word, not a redirection
        ("cargo tree -p <crate> -e normal", &[]),
        // a leading here-string is a redirection; `exec` runs its operand
        ("<<< input cargo test -p a", &[(0, test, "a")]),
        ("exec cargo test -p a", &[(0, test, "a")]),
        ("exec -c -a name cargo build -p a", &[(0, build, "a")]),
        // Windows' executable
        ("cargo.exe test -p a", &[(0, test, "a")]),
        ("C:/Rust/bin/cargo.exe build -p a", &[(0, build, "a")]),
        // the `command` builtin runs its operand; `-v` only looks it up
        ("command cargo test -p a", &[(0, test, "a")]),
        ("command -p cargo test -p a", &[(0, test, "a")]),
        ("command -v cargo test -p gone", &[]),
        // a fully qualified package-ID spec names its fragment's package
        (
            "cargo pkgid -p 'registry+https://github.com/rust-lang/crates.io-index#bitflags@2'",
            &[(0, Some("pkgid"), "bitflags")],
        ),
        (
            "cargo pkgid -p https://github.com/o/some-crate#1.2.3",
            &[(0, Some("pkgid"), "some-crate")],
        ),
        // the legacy `name:version` spec
        ("cargo test -p a:1.2.3", &[(0, test, "a")]),
        // `time` runs the command after its own options (`-p` is time's)
        ("time cargo tree -p a", &[(0, Some("tree"), "a")]),
        ("time -p cargo test -p a", &[(0, test, "a")]),
        ("/usr/bin/time -o out cargo test -p a", &[(0, test, "a")]),
        // a glob is a spec to check, not a placeholder
        ("cargo test -p 'flui-*'", &[(0, test, "flui-*")]),
        // `env -S` splits its value into the command it runs
        ("env -S 'cargo test -p a'", &[(0, test, "a")]),
        (
            "env --split-string='A=1 cargo test -p a'",
            &[(0, test, "a")],
        ),
        ("env -S'cargo build' -p a", &[(0, build, "a")]),
        // a short-option cluster, read as clap reads it
        ("cargo test -qpa", &[(0, test, "a")]),
        ("cargo test -qp a", &[(0, test, "a")]),
        ("cargo test -vqp=a", &[(0, test, "a")]),
        ("cargo build -j4", &[]),
        ("cargo -Zpolonius test", &[]),
        // script mode: what follows the manifest is the script's
        ("cargo -Zscript app.rs -p 8080", &[]),
        ("cargo -Z script app.rs -p 8080", &[]),
        ("cargo app.rs -p 8080", &[]),
        // a line continuation inside double quotes
        ("cargo test -p \"flui-\\\nview\"", &[(0, test, "flui-view")]),
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

/// A `Cargo.lock` of `(name, version)` packages.
fn locked(packages: &[(&str, &str)]) -> BTreeMap<String, BTreeSet<String>> {
    let mut locked: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (name, version) in packages {
        locked
            .entry((*name).to_owned())
            .or_default()
            .insert((*version).to_owned());
    }
    locked
}

/// crates.io's source, as `Cargo.lock` spells it.
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

/// A `Cargo.lock` of `(name, version, source)` packages.
fn lockfile(packages: &[(&str, &str, Option<&str>)]) -> BTreeMap<String, Vec<LockedVersion>> {
    let mut locked: BTreeMap<String, Vec<LockedVersion>> = BTreeMap::new();
    for (name, version, source) in packages {
        locked
            .entry((*name).to_owned())
            .or_default()
            .push(LockedVersion {
                version: (*version).to_owned(),
                source: source.map(str::to_owned),
            });
    }
    locked
}

fn a_lockfile_package_is_selected_only_by_update_and_tree() {
    let packages = Packages {
        local: locked(&[
            ("flui-view", "0.2.0-dev"),
            ("flui-app", "0.2.0"),
            ("alpha", "1.2.3+meta"),
        ]),
        locked: lockfile(&[
            ("wgpu", "25.0.0", Some(CRATES_IO)),
            ("bitflags", "1.3.2", Some(CRATES_IO)),
            ("bitflags", "2.13.2", Some(CRATES_IO)),
            // a git dependency named like a workspace package
            (
                "flui-view",
                "2.0.0",
                Some("git+https://example.com/flui?rev=1#abc"),
            ),
            (
                "gitdep",
                "1.2.3",
                Some("git+file:///tmp/gitdep?rev=abc#abc"),
            ),
            (
                "_helper",
                "1.2.3",
                Some("git+https://host/repo?rev=abc#abc"),
            ),
        ]),
    };
    for (code, selects) in [
        ("cargo test -p flui-view", true),
        // a local package's version is checked too
        ("cargo test -p flui-app@0.2", true),
        ("cargo test -p flui-app@0.2.0", true),
        ("cargo test -p flui-view@999", false),
        // a graph subcommand picks the dependency the local version misses
        ("cargo tree -p flui-view@2", true),
        // a bare name is ambiguous to a graph subcommand when a dependency
        // shares it; `clean` takes both
        ("cargo tree -p flui-view", false),
        ("cargo clean -p flui-view", true),
        // a git source's `?rev=` query is part of it; its `#revision` is not
        (
            "cargo pkgid -p 'git+file:///tmp/gitdep?rev=abc#gitdep@1.2.3'",
            true,
        ),
        (
            "cargo pkgid -p 'git+file:///tmp/gitdep#gitdep@1.2.3'",
            false,
        ),
        // a version-only fragment names the package by the URL's path, not its query
        (
            "cargo pkgid -p 'git+file:///tmp/gitdep?rev=abc#1.2.3'",
            true,
        ),
        // a fragment starting with `_` names a package
        (
            "cargo pkgid -p 'git+https://host/repo?rev=abc#_helper@1.2.3'",
            true,
        ),
        // cargo canonicalizes the source kind's case and a default port
        (
            "cargo pkgid -p 'REGISTRY+https://github.com:443/rust-lang/crates.io-index#bitflags@2.13.2'",
            true,
        ),
        ("cargo test -p flui-view@2", false),
        // a prerelease matches only its full version
        ("cargo test -p flui-view@0.2.0-dev", true),
        ("cargo pkgid -p flui-view@0.2", false),
        ("cargo pkgid -p flui-view@0.2.0", false),
        // a glob must match a package the command can select
        ("cargo test -p flui-*", true),
        ("cargo test -p flui-?iew", true),
        ("cargo test -p definitely-*", false),
        ("cargo test -p wg*", false),
        // a graph subcommand's `-p` is a package-ID spec: no pattern
        ("cargo tree -p wg*", false),
        // …but `cargo tree` takes a workspace pattern
        ("cargo tree -p 'flui-*'", true),
        ("cargo update -p 'flui-*'", false),
        ("cargo update -p 'bitf*'", false),
        // `uninstall` names an installed binary, not a checkout package
        ("cargo uninstall -p cargo-nextest", true),
        // build metadata need not be spelled, but must match when it is
        ("cargo test -p alpha@1.2.3", true),
        ("cargo test -p alpha@1.2.3+meta", true),
        ("cargo test -p alpha@1.2.3+other", false),
        ("cargo test -p alpha:1.2.3", true),
        ("cargo pkgid -p flui-view:0.2.0-dev", true),
        ("cargo pkgid -p flui-view:0.2", false),
        (
            "cargo pkgid -p 'registry+https://github.com/rust-lang/crates.io-index#bitflags@2.13.2'",
            true,
        ),
        // the source must be the locked package's, its kind optional
        (
            "cargo pkgid -p 'registry+https://example.com/index#bitflags@2.13.2'",
            false,
        ),
        (
            "cargo pkgid -p 'https://github.com/rust-lang/crates.io-index#bitflags@2.13.2'",
            true,
        ),
        // a path source is one machine's absolute directory: never portable
        ("cargo pkgid -p 'path+file:///repo/crates/flui-app'", false),
        (
            "cargo pkgid -p 'file:///repo/crates/flui-app#flui-app@0.2.0'",
            false,
        ),
        // a source's scheme and host match in any case, its path does not
        (
            "cargo pkgid -p 'registry+HTTPS://GITHUB.COM/rust-lang/crates.io-index#bitflags@2.13.2'",
            true,
        ),
        (
            "cargo pkgid -p 'registry+https://github.com/RUST-LANG/crates.io-index#bitflags@2.13.2'",
            false,
        ),
        // glob classes, as cargo's package patterns take them
        ("cargo test -p 'flui-[v]iew'", true),
        ("cargo test -p 'flui-[a-z]pp'", true),
        ("cargo test -p 'flui-[!v]iew'", false),
        // cargo negates with `!` only: `[^v]` is the class of `^` and `v`
        ("cargo test -p 'flui-[^v]pp'", false),
        ("cargo test -p 'flui-[^a]pp'", true),
        // many stars take linear steps, not exponential backtracking
        ("cargo test -p '*******************************z'", false),
        ("cargo test -p '**f**l**u**i**-**a**p**p**'", true),
        ("cargo update -p wgpu", true),
        ("cargo tree -p wgpu", true),
        ("cargo pkgid -p wgpu", true),
        ("cargo clean -p wgpu", true),
        ("cargo update -p wgpu@25", true),
        ("cargo update -p wgpu@24", false),
        ("cargo test -p wgpu", false),
        ("cargo -p wgpu", false),
        ("cargo update -p flui-types", false),
        // two locked versions: a bare name is ambiguous but to `clean`
        ("cargo tree -p bitflags", false),
        ("cargo update -p bitflags", false),
        ("cargo pkgid -p bitflags", false),
        ("cargo clean -p bitflags", true),
        ("cargo tree -p bitflags@2", true),
        ("cargo pkgid -p bitflags@1.3.2", true),
        // `@2.1` is no prefix of `2.13.2` at a version boundary
        ("cargo tree -p bitflags@2.1", false),
    ] {
        let selected = extract::packages(code);
        assert_eq!(selected.len(), 1, "{code:?}");
        assert_eq!(packages.selects(&selected[0]), selects, "{code:?}");
    }
}

fn headings_give_github_anchors() {
    let markdown = "# Start here\n## The `View` tree: a guide!\n## Start here\n\
                    ## Custom {#own-id}\n\n```\n# not a heading\n```\n\
                    # Foo\n# Foo-1\n# Foo\n# The $x$ value\n# Foo--bar\n";
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
        "the-x-value",
        // no smart punctuation: `--` stays two dashes
        "foo--bar",
    ]
    .map(str::to_owned)
    .into();
    assert_eq!(extract::anchors(markdown), want);
}

fn a_powershell_fence_is_lexed_as_powershell() {
    let markdown = "```powershell\n<# cargo test -p gone\n#>\necho x >\"prefix `$(cargo test -p gone)\"\necho x >\"$(cargo test -p c)\"\necho x >\"foo\\\"bar; cargo test -p d\necho \"$(Write-Output `); cargo test -p e)\"\ncargo test -p a`-b `\n  -p x\\y\n```\n\n```bash\ncargo test -p a`x`\n```\n";
    let code = extract::code(markdown);
    let dialects: Vec<shell::Dialect> = code.iter().map(|code| code.dialect).collect();
    assert_eq!(
        dialects,
        [shell::Dialect::PowerShell, shell::Dialect::Posix]
    );
    let names = |code: &extract::Code| -> Vec<String> {
        extract::packages_in(&code.text, code.dialect)
            .into_iter()
            .map(|selected| selected.name)
            .collect()
    };
    // a backtick escapes and continues the line; `\` is a plain character
    // a redirection target's `$(…)` runs, unless a backtick escapes its `$`;
    // `\"` closes a target's quote (the backslash is plain), and a backtick
    // keeps a `)` inside a subexpression
    assert_eq!(names(&code[0]), ["d", "a-b", "x\\y", "c", "e"]);
    // in bash the backtick opens a substitution
    assert_eq!(names(&code[1]), [] as [&str; 0]);
}

fn code_spans_and_blocks_carry_their_lines() {
    // a code span labelling a permalink to a commit (its scheme, host and
    // repository in any case) is pinned: its path is cited as it was
    // then; any other link's label, a branch (or `main` misspelt) too, is still
    // a path to check
    let markdown = "# T\n\nSee `docs/x.md`.\n\n```bash\ncargo test\ncargo run -p a\n```\n\n    indented\n\n\
                    [l](docs/y.md) ![i](/z.png) \
                    [`docs/old.md`](HTTPS://GitHub.com/VANYASTAFF/FLUI/blob/e30ab71/docs/old.md) \
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
                dialect: shell::Dialect::Posix,
                pinned: false,
            },
            extract::Code {
                line: 6,
                text: "cargo test\ncargo run -p a\n".to_owned(),
                block: true,
                dialect: shell::Dialect::Posix,
                pinned: false,
            },
            extract::Code {
                line: 10,
                text: "indented\n".to_owned(),
                block: true,
                dialect: shell::Dialect::Posix,
                pinned: false,
            },
            extract::Code {
                line: 12,
                text: "docs/old.md".to_owned(),
                block: false,
                dialect: shell::Dialect::Posix,
                pinned: true,
            },
            extract::Code {
                line: 12,
                text: "docs/now.md".to_owned(),
                block: false,
                dialect: shell::Dialect::Posix,
                pinned: false,
            },
            extract::Code {
                line: 12,
                text: "docs/testng.md".to_owned(),
                block: false,
                dialect: shell::Dialect::Posix,
                pinned: false,
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
                "HTTPS://GitHub.com/VANYASTAFF/FLUI/blob/e30ab71/docs/old.md".to_owned()
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
        // a doc inside a package names its own layout, not another's
        ("tests/realm.rs", false),
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
    // outside a package, only the root and the doc's directory, and package
    // layouts, in whichever package has the path
    assert!(!known.resolves("docs/testing.md", "platforms/mod.rs"));
    assert!(known.resolves("docs/testing.md", "adr/ADR-0081-tiers.md"));
    assert!(known.resolves("docs/testing.md", "tests/realm.rs"));
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
        // a scheme and a host match in any case
        (
            "HTTPS://GitHub.COM/VanyaStaff/FLUI/tree/main/crates/flui-view",
            Some(Some("crates/flui-view")),
        ),
        ("docs/../README.md", Some(Some("README.md"))),
        // the repository root
        ("./", Some(Some(""))),
        ("/", Some(Some(""))),
        ("../README.md", Some(None)),
        // not local
        ("https://example.com/x.md", None),
        ("https://github.com/vanyastaff/flui/issues/1", None),
        ("mailto:a@b.c", None),
        ("//example.com/docs", None),
        // a `:` in a query or an anchor is data, not a scheme
        ("docs/removed.md?at=12:00", Some(Some("docs/removed.md"))),
        (
            "docs/removed.md#:~:text=probe",
            Some(Some("docs/removed.md")),
        ),
        // an empty path before a query is the doc itself
        ("?view=compact#missing", Some(Some("llms.txt"))),
        // a percent-escaped name is the file's name
        ("docs/review%20probe.md", Some(Some("docs/review probe.md"))),
        ("#start-here", Some(Some("llms.txt"))),
    ] {
        let got = link_target("llms.txt", dest);
        assert_eq!(got.as_ref().map(|path| path.as_deref()), target, "{dest:?}");
    }
}

fn a_doc_reports_every_stale_occurrence() {
    let known = known();
    let packages = Packages {
        local: locked(&[("flui-view", "0.2.0"), ("flui-app", "0.2.0")]),
        locked: lockfile(&[("wgpu", "25.0.0", Some(CRATES_IO))]),
    };
    let read = |path: &str| (path == "docs/testing.md").then(|| "# The harness\n".to_owned());
    let text = "`crates/flui-view/src/lib.rs` `crates/flui-types/` `crates/flui-types/`\n\
                `cargo test -p flui_view`\n\n\
                ```sh\ncargo update -p wgpu\ncargo test -p flui-types\ncargo test -p wgpu\n```\n\n\
                [ok](docs/testing.md) [gone](docs/gone.md) [out](../x.md) [web](https://a.b/)\n\
                [h](docs/testing.md#the-harness) [no](docs/testing.md#no-heading) \
                [dir](crates/flui-view#x) [self](#no-heading) \
                [escaped](docs/testing.md#the%2Dharness) [root](./) [root](/) [top](#) [top](docs/testing.md#)\n";
    let names = |doc: &str| -> Vec<(usize, Kind, String)> {
        stale(doc, text, &known, &packages, &read)
            .into_iter()
            .map(|stale| (stale.line, stale.kind, stale.name))
            .collect()
    };
    // the same stale name twice on a line is two occurrences
    let mut want = vec![
        (1, Kind::Path, "crates/flui-types/".to_owned()),
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

fn a_pinned_label_is_no_path_but_its_command_is_read() {
    let packages = Packages {
        local: locked(&[("flui-view", "0.2.0")]),
        locked: BTreeMap::new(),
    };
    // a commit hash is hex in either case
    let text = "[`docs/gone.md`](https://github.com/vanyastaff/flui/blob/e30ab71/docs/gone.md) \
                [`docs/gone2.md`](https://github.com/vanyastaff/flui/blob/E30AB71/docs/gone2.md) \
                [`cargo test -p gone`](https://github.com/vanyastaff/flui/blob/e30ab71/x.md)\n";
    let found: Vec<(Kind, String)> = stale("README.md", text, &known(), &packages, &|_| None)
        .into_iter()
        .map(|stale| (stale.kind, stale.name))
        .collect();
    assert_eq!(found, [(Kind::Package, "gone".to_owned())]);
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
                "a_powershell_fence_is_lexed_as_powershell",
                a_powershell_fence_is_lexed_as_powershell as fn(),
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
                "a_doc_reports_every_stale_occurrence",
                a_doc_reports_every_stale_occurrence as fn(),
            ),
            (
                "a_pinned_label_is_no_path_but_its_command_is_read",
                a_pinned_label_is_no_path_but_its_command_is_read as fn(),
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
