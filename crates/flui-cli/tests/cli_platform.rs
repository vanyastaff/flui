//! Integration tests for `flui platform` command.
//!
//! Tests the platform list subcommand and validates output.
//! Note: cliclack writes all interactive output to stderr.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

/// Get a command for the `flui` binary. The project's own Cargo
/// configuration decides its target-dir, not one the test run inherits.
fn flui() -> Command {
    let mut command = cargo_bin_cmd!("flui");
    command
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR");
    command
}

/// A temp dir with a `flui.toml` that already targets `android`, so
/// `platform remove android` has something to remove.
fn project_with_android_configured() -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::write(
        tmp.path().join("flui.toml"),
        "[app]\nname = \"test-app\"\nversion = \"0.1.0\"\norganization = \"com.example\"\n\
         [build]\ntarget_platforms = [\"android\"]\n",
    )
    .expect("write flui.toml");
    tmp
}

#[test]
fn platform_remove_with_yes_skips_the_prompt_and_updates_the_config() {
    let tmp = project_with_android_configured();

    flui()
        .current_dir(tmp.path())
        .args(["platform", "remove", "android", "--yes"])
        .assert()
        .success();

    let config = std::fs::read_to_string(tmp.path().join("flui.toml")).expect("read flui.toml");
    assert!(
        !config.contains("android"),
        "android should be removed from target_platforms: {config}"
    );
}

/// A project created with every platform, inside its own git repository.
fn project_with_every_platform() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().expect("temp dir");
    flui()
        .args([
            "create",
            "all-platforms",
            "--org",
            "com.test",
            "--template",
            "empty",
            "--platforms",
            "android,ios,web,windows,linux,macos",
            "--no-check",
        ])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success();
    let project = tmp.path().join("all-platforms");
    if !project.join(".git").exists() {
        let status = std::process::Command::new("git")
            .arg("init")
            .arg("--quiet")
            .current_dir(&project)
            .status()
            .expect("run git init");
        assert!(status.success(), "git init failed");
    }
    (tmp, project)
}

/// Whether the project's `.gitignore` files ignore `rel_path`.
fn git_ignores(project: &std::path::Path, rel_path: &str) -> bool {
    let status = std::process::Command::new("git")
        .args(["check-ignore", "--quiet", "--no-index", rel_path])
        .current_dir(project)
        .status()
        .expect("run git check-ignore");
    match status.code() {
        Some(0) => true,
        Some(1) => false,
        _ => panic!("git check-ignore failed on {rel_path}: {status}"),
    }
}

/// What a platform build writes under `platforms/`: Gradle's cache, its
/// root-project and `app` module outputs (the module one holds the APK
/// `flui build android` copies out), the native libraries the builder
/// copies in before Gradle runs, xcodebuild's output, and each build's
/// deliverables under `<target-dir>/flui-out/<project>/<platform>`, and the
/// CLI's record of claimed `--output` directories in `.flui/`.
const BUILD_OUTPUTS: &[&str] = &[
    "platforms/android/.gradle/8.9/checksums/checksums.lock",
    "platforms/android/build/reports/problems/problems-report.html",
    "platforms/android/app/build/outputs/apk/debug/app-debug.apk",
    "platforms/android/app/src/main/jniLibs/arm64-v8a/liball_platforms.so",
    "platforms/ios/build/Debug/iphoneos/flui.app/Info.plist",
    "target/flui-out/all-platforms/android/all-platforms-debug.apk",
    "target/flui-out/all-platforms/ios/flui.app/Info.plist",
    "target/flui-out/all-platforms/web/pkg/app_bg.wasm",
    "target/flui-out/all-platforms/web/index.html",
    "target/flui-out/all-platforms/desktop/all-platforms.exe",
    ".flui/out-dirs/web/0000000000000000",
];

/// Scaffolded or user-added source the builds read, which must stay
/// tracked: the Gradle wrapper's presence is what makes
/// `flui build android` package through Gradle instead of the SDK's
/// build-tools.
const SOURCES: &[&str] = &[
    "platforms/android/gradlew",
    "platforms/android/gradlew.bat",
    "platforms/android/gradle/wrapper/gradle-wrapper.jar",
    "platforms/android/gradle/wrapper/gradle-wrapper.properties",
    "platforms/android/build.gradle.kts",
    "platforms/android/app/build.gradle.kts",
    "platforms/android/app/src/main/AndroidManifest.xml",
    "platforms/ios/README.md",
    "platforms/web/index.html",
    "platforms/web/manifest.json",
    "platforms/web/icons/icon-192.png",
    "platforms/windows/.gitignore",
    "platforms/linux/.gitignore",
    "platforms/macos/.gitignore",
];

#[test]
fn scaffolded_platforms_ignore_and_clean_what_builds_write() {
    let (_tmp, project) = project_with_every_platform();

    let wrongly_tracked: Vec<_> = BUILD_OUTPUTS
        .iter()
        .filter(|path| !git_ignores(&project, path))
        .collect();
    assert!(
        wrongly_tracked.is_empty(),
        "build outputs the scaffolded .gitignore files track: {wrongly_tracked:?}"
    );
    let wrongly_ignored: Vec<_> = SOURCES
        .iter()
        .filter(|path| git_ignores(&project, path))
        .collect();
    assert!(
        wrongly_ignored.is_empty(),
        "sources the scaffolded .gitignore files ignore: {wrongly_ignored:?}"
    );

    for path in BUILD_OUTPUTS {
        let path = project.join(path);
        std::fs::create_dir_all(path.parent().expect("output has a parent"))
            .expect("create output dir");
        std::fs::write(&path, b"").expect("write output");
    }
    for platform in ["android", "ios", "web", "desktop"] {
        flui()
            .current_dir(&project)
            .args(["clean", "--platform", platform])
            .assert()
            .success();
    }
    let left: Vec<_> = BUILD_OUTPUTS
        .iter()
        .filter(|path| project.join(path).exists())
        .collect();
    assert!(left.is_empty(), "`flui clean` left build outputs: {left:?}");
}

/// The build outputs live in the target-dir cargo reports for the project
/// (here one `.cargo/config.toml` moves beside it, shared with another
/// project), under the project's own name: a clean removes this project's
/// outputs there, not the other project's, and not a `target/` cargo never
/// uses.
#[test]
fn clean_follows_cargos_target_dir_and_keeps_other_projects_outputs() {
    let (tmp, project) = project_with_every_platform();
    std::fs::create_dir_all(project.join(".cargo")).expect(".cargo");
    std::fs::write(
        project.join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"../shared-target\"\n",
    )
    .expect("config.toml");
    let shared = tmp.path().join("shared-target/flui-out");
    let ours = shared.join("all-platforms/web/index.html");
    let theirs = shared.join("other-app/web/index.html");
    let unused = project.join("target/flui-out/all-platforms/web/index.html");
    for path in [&ours, &theirs, &unused] {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("output dir");
        std::fs::write(path, b"").expect("output");
    }

    flui()
        .current_dir(&project)
        .args(["clean", "--platform", "web"])
        .assert()
        .success();

    assert!(
        !ours.exists(),
        "the project's output in the shared target-dir survived"
    );
    assert!(theirs.exists(), "clean removed another project's output");
    assert!(
        unused.exists(),
        "clean looked in a target/ cargo does not use"
    );
}

/// `flui build android` reads the Gradle APK from Gradle's default module
/// build directory, `app/build/outputs/apk/<profile>`; a scaffolded build
/// script that relocates `buildDirectory` sends the APK where neither the
/// builder, the `.gitignore` nor `flui clean` looks.
#[test]
fn scaffolded_android_gradle_keeps_the_default_build_directory() {
    let (_tmp, project) = project_with_every_platform();
    for script in [
        "build.gradle.kts",
        "app/build.gradle.kts",
        "settings.gradle.kts",
    ] {
        let text = std::fs::read_to_string(project.join("platforms/android").join(script))
            .unwrap_or_else(|error| panic!("read {script}: {error}"));
        assert!(
            !text.contains("buildDirectory.value") && !text.contains("buildDir ="),
            "platforms/android/{script} relocates Gradle's build directory:\n{text}"
        );
    }
}
