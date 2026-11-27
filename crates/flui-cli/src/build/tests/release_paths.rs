//! A release build leaves no absolute path of the build machine in the
//! binary, and keeps the flags the project configures: a path dependency
//! outside the workspace and a file it generates in a separate build
//! directory, whose panic locations rustc writes into the binary, are named
//! by package and by `build/`, and a rustflags string an included
//! configuration file gives still reaches rustc.
use std::path::Path;

use crate::build::desktop::DesktopBuilder;
use crate::build::{BuildUnit, BuilderContextBuilder, Platform, Profile};

/// What the application prints only where the project's own `--cfg` flag
/// reached rustc.
const AUTHOR_FLAG_MARK: &str = "the-authors-cfg-reached-rustc";

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("file parent")).expect("directory");
    std::fs::write(path, contents).expect("fixture file");
}

/// The binary `profile` builds of an application that indexes a slice in a
/// path dependency outside its workspace and in a file that dependency
/// generates (two panic locations), with its intermediate outputs in a build
/// directory of their own and a `--cfg` given as a string by an included
/// configuration file.
fn built(machine: &Path, profile: Profile) -> Vec<u8> {
    let app = machine.join("app");
    write(
        &app,
        "Cargo.toml",
        "[workspace]\n[package]\nname = \"paths-probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
         [dependencies]\nfar-dep = { path = \"../elsewhere/far-dep\" }\n\
         [lints.rust]\nunexpected_cfgs = { level = \"allow\" }\n",
    );
    write(
        &app,
        "src/main.rs",
        &format!(
            "fn main() {{\n    let n = std::env::args().count();\n    \
             println!(\"{{}} {{}}\", far_dep::at(&[1, 2], n), far_dep::generated(&[3, 4], n));\n    \
             #[cfg(probe_flag)]\n    println!(\"{AUTHOR_FLAG_MARK}\");\n}}\n"
        ),
    );
    let build_dir = toml::Value::String(machine.join("build-dir").to_string_lossy().into_owned());
    write(
        &app,
        ".cargo/config.toml",
        &format!("include = [\"flags.toml\"]\n[build]\nbuild-dir = {build_dir}\n"),
    );
    write(
        &app,
        ".cargo/flags.toml",
        "[build]\nrustflags = \"--cfg probe_flag\"\n",
    );
    write(
        machine,
        "elsewhere/far-dep/Cargo.toml",
        "[package]\nname = \"far-dep\"\nversion = \"0.3.1\"\nedition = \"2024\"\n",
    );
    write(
        machine,
        "elsewhere/far-dep/build.rs",
        "fn main() {\n    let out = std::env::var(\"OUT_DIR\").expect(\"OUT_DIR\");\n    \
         std::fs::write(std::path::Path::new(&out).join(\"generated.rs\"), \
         \"#[inline(never)] pub fn generated(values: &[u8], index: usize) -> u8 { values[index] }\")\
         .expect(\"generated.rs\");\n}\n",
    );
    write(
        machine,
        "elsewhere/far-dep/src/lib.rs",
        "#[inline(never)]\npub fn at(values: &[u8], index: usize) -> u8 { values[index] }\n\
         include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\n",
    );
    let ctx = BuilderContextBuilder::new(app.clone())
        .platform(Platform::Desktop { target: None })
        .profile(profile)
        .target(BuildUnit::DefaultBinary)
        .output_dir(app.join("staged"))
        .build();
    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    let artifacts = runtime
        .block_on(DesktopBuilder::new().build_rust(&ctx))
        .expect("the fixture builds");
    std::fs::read(artifacts.executable.expect("an executable")).expect("the binary")
}

fn holds(binary: &[u8], text: &str) -> bool {
    binary
        .windows(text.len())
        .any(|window| window == text.as_bytes())
}

#[test]
fn a_release_binary_names_no_directory_of_the_build_machine() {
    let machine = tempfile::tempdir().expect("a fixture machine");
    // Canonical, so that the path checked is the one cargo hands rustc where
    // the temporary directory sits behind a symlink (`/var` on macOS).
    let machine_dir = machine
        .path()
        .canonicalize()
        .expect("the fixture directory");
    let dependency = machine_dir
        .join("elsewhere")
        .join("far-dep")
        .to_string_lossy()
        .into_owned();
    let build_dir = machine_dir.join("build-dir").to_string_lossy().into_owned();

    let debug = built(&machine_dir, Profile::Debug);
    for (what, path) in [
        ("the dependency's", &dependency),
        ("the build directory's", &build_dir),
    ] {
        assert!(
            holds(&debug, path),
            "a debug build keeps {what} path {path}, so the release checks below read the same \
             locations"
        );
    }

    let release = built(&machine_dir, Profile::Release);
    let machine_dir = machine_dir.to_string_lossy().into_owned();
    assert!(
        !holds(&release, &machine_dir),
        "the release binary names the fixture's directory {machine_dir}"
    );
    let separator = std::path::MAIN_SEPARATOR;
    for renamed in [
        format!("far-dep-0.3.1{separator}src"),
        format!("build{separator}"),
    ] {
        assert!(holds(&release, &renamed), "a location reads {renamed}…");
    }
    assert!(
        holds(&release, AUTHOR_FLAG_MARK),
        "the included rustflags string reached rustc"
    );
}
