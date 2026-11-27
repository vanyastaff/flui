use std::path::PathBuf;
use std::str::FromStr as _;

use cargo_platform::Cfg;

use super::{BuildDirs, CargoRustflags, Remap, RustflagsSource, SourcePathRemap};

const TRIPLE: &str = "x86_64-unknown-linux-gnu";

fn table(text: &str) -> toml::Table {
    text.parse().expect("a fixture configuration")
}

fn linux_cfgs() -> Vec<Cfg> {
    ["unix", "target_os=\"linux\"", "target_arch=\"x86_64\""]
        .into_iter()
        .map(|cfg| Cfg::from_str(cfg).expect("a fixture cfg"))
        .collect()
}

/// One way an author gives rustflags, and the source cargo reads for it.
struct SourceCase {
    label: &'static str,
    vars: &'static [(&'static str, &'static str)],
    configs: Vec<toml::Table>,
    expected: RustflagsSource,
}

impl SourceCase {
    fn new(
        label: &'static str,
        vars: &'static [(&'static str, &'static str)],
        configs: Vec<toml::Table>,
        expected: RustflagsSource,
    ) -> Self {
        Self {
            label,
            vars,
            configs,
            expected,
        }
    }
}

/// The source cargo reads, for each way an author gives rustflags: the
/// variables before the files, a target's table before `build`, a
/// `cfg(…)` table only where it matches the target.
#[test]
fn the_rustflags_source_is_the_one_cargo_reads() {
    let target_file = table(&format!(
        "[target.{TRIPLE}]\nrustflags = [\"-Ctarget-cpu=native\"]\n"
    ));
    let cfg_linux =
        table("[target.'cfg(target_os = \"linux\")']\nrustflags = [\"-Cforce-frame-pointers\"]\n");
    let cfg_windows =
        table("[target.'cfg(windows)']\nrustflags = [\"-Ctarget-feature=+crt-static\"]\n");
    let linker_only = table(&format!("[target.{TRIPLE}]\nlinker = \"clang\"\n"));
    let build = table("[build]\nrustflags = [\"-Cdebuginfo=1\"]\n");
    let target = RustflagsSource::Target(TRIPLE.into());
    let cases = [
        SourceCase::new("nothing", &[], vec![], RustflagsSource::Build),
        SourceCase::new(
            "encoded over plain and files",
            &[
                ("CARGO_ENCODED_RUSTFLAGS", "-Ca\x1f-Cb"),
                ("RUSTFLAGS", "-Cc"),
            ],
            vec![target_file.clone()],
            RustflagsSource::Encoded("-Ca\x1f-Cb".into()),
        ),
        SourceCase::new(
            "an empty encoded variable is still the source",
            &[("CARGO_ENCODED_RUSTFLAGS", ""), ("RUSTFLAGS", "-Cc")],
            vec![],
            RustflagsSource::Encoded(String::new()),
        ),
        SourceCase::new(
            "plain over files",
            &[("RUSTFLAGS", "-Cc -Cd")],
            vec![target_file.clone()],
            RustflagsSource::Plain("-Cc -Cd".into()),
        ),
        SourceCase::new(
            "the target's variable",
            &[("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS", "-Ce")],
            vec![build.clone()],
            target.clone(),
        ),
        SourceCase::new(
            "the target's table over build",
            &[],
            vec![build.clone(), target_file],
            target.clone(),
        ),
        SourceCase::new(
            "a matching cfg table",
            &[],
            vec![cfg_linux, build.clone()],
            target,
        ),
        SourceCase::new(
            "a cfg table of another target",
            &[],
            vec![cfg_windows, build.clone()],
            RustflagsSource::Build,
        ),
        SourceCase::new(
            "a target table without rustflags",
            &[],
            vec![linker_only, build],
            RustflagsSource::Build,
        ),
        SourceCase::new(
            "a string in the nearest file replaces a farther one",
            &[],
            vec![
                table("[build]\nrustflags = \"-Ca  -Cb\"\n"),
                table("[build]\nrustflags = \"-Cfar\"\n"),
            ],
            RustflagsSource::Configured(vec!["-Ca".into(), "-Cb".into()]),
        ),
        SourceCase::new(
            "a string, then the variable",
            &[("CARGO_BUILD_RUSTFLAGS", "-Cvar")],
            vec![table("[build]\nrustflags = \"-Ca\"\n")],
            RustflagsSource::Configured(vec!["-Ca".into(), "-Cvar".into()]),
        ),
        SourceCase::new(
            "a target's string beside another table's arrays, farthest first",
            &[],
            vec![
                table(&format!(
                    "[target.{TRIPLE}]\nrustflags = \"-Ct\"\n[target.'cfg(unix)']\nrustflags = [\"-Cnear\"]\n"
                )),
                table("[target.'cfg(unix)']\nrustflags = [\"-Cfar\"]\n"),
            ],
            RustflagsSource::Configured(vec!["-Ct".into(), "-Cfar".into(), "-Cnear".into()]),
        ),
    ];
    let mut failures = Vec::new();
    for SourceCase {
        label,
        vars,
        configs,
        expected,
    } in cases
    {
        let found = RustflagsSource::of_build(
            TRIPLE,
            |name| {
                vars.iter()
                    .find(|(var, _)| *var == name)
                    .map(|(_, value)| (*value).to_owned())
            },
            &configs,
            || Ok(linux_cfgs()),
        )
        .expect("a source");
        if found != expected {
            failures.push(format!("{label}: {found:?}, expected {expected:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// rustc is asked for the target's cfgs only when a `cfg(…)` table needs
/// them: a build with none starts no `rustc`.
#[test]
fn the_target_cfgs_are_asked_only_for_a_cfg_table() {
    let source = RustflagsSource::of_build(
        TRIPLE,
        |_| None,
        &[table("[build]\nrustflags = [\"-Cdebuginfo=1\"]\n")],
        || panic!("no cfg table, so no rustc"),
    )
    .expect("a source");
    assert_eq!(source, RustflagsSource::Build);
}

fn remap() -> SourcePathRemap {
    SourcePathRemap {
        remaps: vec![
            Remap {
                from: PathBuf::from(
                    "/home/me/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f",
                ),
                to: String::new(),
            },
            Remap {
                from: PathBuf::from("/home/me/code with space/dep/"),
                to: "dep-0.1.0/".into(),
            },
        ],
    }
}

const FLAGS: [&str; 2] = [
    "--remap-path-prefix=/home/me/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/=",
    "--remap-path-prefix=/home/me/code with space/dep/=dep-0.1.0/",
];

/// The rewrites join the author's flags in the source cargo reads them from,
/// after them, and a path with a space stays one flag.
#[cfg(unix)]
#[test]
fn the_rewrites_join_the_authors_flags() {
    let encoded = |flags: &[&str]| CargoRustflags {
        args: Vec::new(),
        env: vec![("CARGO_ENCODED_RUSTFLAGS", flags.join("\x1f"))],
    };
    let config = |key: &str| {
        let array = FLAGS.map(|flag| format!("{flag:?}")).join(", ");
        CargoRustflags {
            args: vec!["--config".into(), format!("{key}=[{array}]")],
            env: Vec::new(),
        }
    };
    let cases = [
        (
            RustflagsSource::Encoded("-Ca\x1f-Cb=c d".into()),
            encoded(&["-Ca", "-Cb=c d", FLAGS[0], FLAGS[1]]),
        ),
        (RustflagsSource::Encoded(String::new()), encoded(&FLAGS)),
        (
            RustflagsSource::Plain("  -Ca   -Cb ".into()),
            encoded(&["-Ca", "-Cb", FLAGS[0], FLAGS[1]]),
        ),
        (
            RustflagsSource::Target(TRIPLE.into()),
            config(&format!("target.\"{TRIPLE}\".rustflags")),
        ),
        (RustflagsSource::Build, config("build.rustflags")),
        (
            RustflagsSource::Configured(vec!["-Ca".into(), "-Cb".into()]),
            encoded(&["-Ca", "-Cb", FLAGS[0], FLAGS[1]]),
        ),
    ];
    let mut failures = Vec::new();
    for (source, expected) in cases {
        let found = remap().onto(&source);
        if found != expected {
            failures.push(format!("{source:?}: {found:?}\n  expected {expected:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A `--config` value is TOML cargo parses back to the same flags, a
/// Windows path's backslashes included.
#[test]
fn a_config_value_parses_back_to_the_flags() {
    let windows = SourcePathRemap {
        remaps: vec![Remap {
            from: PathBuf::from(
                r"C:\Users\me\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f",
            ),
            to: String::new(),
        }],
    };
    for remap in [remap(), windows] {
        let CargoRustflags { args, .. } = remap.onto(&RustflagsSource::Build);
        let [flag, value] = args.as_slice() else {
            panic!("one --config: {args:?}");
        };
        assert_eq!(flag, "--config");
        let parsed: toml::Table = value.parse().expect("the value is TOML");
        let flags: Vec<String> = parsed["build"]["rustflags"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|flag| flag.as_str().expect("a string").to_owned())
            .collect();
        assert_eq!(flags, remap.flags());
    }
}

/// No rewrite gives cargo nothing: the author's flags stay where they are.
#[test]
fn no_rewrite_adds_nothing() {
    let none = SourcePathRemap::default();
    for source in [
        RustflagsSource::Encoded("-Ca".into()),
        RustflagsSource::Plain("-Ca".into()),
        RustflagsSource::Build,
    ] {
        assert_eq!(none.onto(&source), CargoRustflags::default());
    }
}

/// A cargo home and a target directory inside the workspace, a path
/// dependency elsewhere, a package of the workspace and one of a registry, and
/// a build directory that is the target directory: each directory gets its
/// one rewrite, after every directory that holds it.
#[cfg(unix)]
#[test]
fn each_directory_is_rewritten_after_the_directories_that_hold_it() {
    let path = PathBuf::from;
    let registry = path("/work/app/.cargo-home/registry/src/index.crates.io-1949cf8c6b5b557f");
    let remap = SourcePathRemap::of_dirs(BuildDirs {
        root: path("/work/app"),
        target: path("/work/app/target"),
        build: Some(path("/work/app/target")),
        packages: vec![
            (path("/work/app"), "app-0.1.0".into()),
            (path("/work/app/crates/member"), "member-0.1.0".into()),
            (registry.join("serde-1.0.0"), "serde-1.0.0".into()),
            (path("/work/elsewhere/far-dep"), "far-dep-0.3.1".into()),
            (path("/work/elsewhere/far-dep"), "far-dep-0.3.1".into()),
        ],
        registries: vec![registry.clone()],
        git_checkouts: Some(path("/work/app/.cargo-home/git/checkouts")),
    });
    assert_eq!(
        remap.flags(),
        [
            "--remap-path-prefix=/work/app/=".to_owned(),
            "--remap-path-prefix=/work/app/target/=target/".to_owned(),
            "--remap-path-prefix=/work/elsewhere/far-dep/=far-dep-0.3.1/".to_owned(),
            "--remap-path-prefix=/work/app/.cargo-home/git/checkouts/=git/".to_owned(),
            format!("--remap-path-prefix={}/=", registry.display()),
        ],
    );
}

/// A build directory apart from the target directory gets its own rewrite.
#[cfg(unix)]
#[test]
fn a_separate_build_directory_is_rewritten() {
    let path = PathBuf::from;
    let remap = SourcePathRemap::of_dirs(BuildDirs {
        root: path("/work/app"),
        target: path("/work/app/target"),
        build: Some(path("/scratch/build-dir")),
        ..BuildDirs::default()
    });
    assert_eq!(
        remap.flags(),
        [
            "--remap-path-prefix=/scratch/build-dir/=build/",
            "--remap-path-prefix=/work/app/=",
            "--remap-path-prefix=/work/app/target/=target/",
        ],
    );
}
