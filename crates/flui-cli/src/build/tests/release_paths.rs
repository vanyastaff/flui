//! A release build leaves no absolute path of the build machine in the
//! binary: a path dependency outside the workspace, whose panic location
//! rustc writes into the binary, is named by its package instead.
use std::path::Path;

use crate::build::desktop::DesktopBuilder;
use crate::build::{BuildUnit, BuilderContextBuilder, Platform, Profile};

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("file parent")).expect("directory");
    std::fs::write(path, contents).expect("fixture file");
}

/// The binary `profile` builds of an application that indexes a slice in a
/// path dependency outside its workspace (a panic location in that crate).
fn built(machine: &Path, profile: Profile) -> Vec<u8> {
    let app = machine.join("app");
    write(
        &app,
        "Cargo.toml",
        "[workspace]\n[package]\nname = \"paths-probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
         [dependencies]\nfar-dep = { path = \"../elsewhere/far-dep\" }\n",
    );
    write(
        &app,
        "src/main.rs",
        "fn main() { let n = std::env::args().count(); println!(\"{}\", far_dep::at(&[1, 2], n)); }\n",
    );
    write(
        machine,
        "elsewhere/far-dep/Cargo.toml",
        "[package]\nname = \"far-dep\"\nversion = \"0.3.1\"\nedition = \"2024\"\n",
    );
    write(
        machine,
        "elsewhere/far-dep/src/lib.rs",
        "#[inline(never)]\npub fn at(values: &[u8], index: usize) -> u8 { values[index] }\n",
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
    let dependency = machine_dir.join("elsewhere").join("far-dep");
    let absolute = dependency.to_string_lossy().into_owned();

    let debug = built(&machine_dir, Profile::Debug);
    assert!(
        holds(&debug, &absolute),
        "a debug build keeps the dependency's path {absolute}, so the release check below reads \
         the same location"
    );

    let release = built(&machine_dir, Profile::Release);
    assert!(
        !holds(&release, &absolute),
        "the release binary names {absolute}"
    );
    let machine_dir = machine_dir.to_string_lossy().into_owned();
    assert!(
        !holds(&release, &machine_dir),
        "the release binary names the fixture's directory {machine_dir}"
    );
    let renamed = format!("far-dep-0.3.1{}src", std::path::MAIN_SEPARATOR);
    assert!(
        holds(&release, &renamed),
        "the dependency's location reads {renamed}…"
    );
}
