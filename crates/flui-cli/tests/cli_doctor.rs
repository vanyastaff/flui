//! Integration tests for `flui doctor`.
//!
//! Human-mode narration goes to stderr (see `src/ui.rs`); stdout is reserved
//! for machine output (`--json`) and must stay clean in every mode.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;

/// Get a command for the `flui` binary.
fn flui() -> Command {
    cargo_bin_cmd!("flui")
}

/// `--json` must produce nothing but NDJSON on stdout: every line parses,
/// each carries an `event`, there is at least one `doctor.check` and
/// exactly one `doctor.summary`.
#[test]
fn doctor_json_stdout_is_pure_ndjson() {
    let assert = flui().args(["--json", "doctor"]).assert();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut checks = 0usize;
    let mut summaries = 0usize;
    for line in stdout.lines() {
        assert!(!line.trim().is_empty(), "blank line in NDJSON stdout");
        let value: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("non-JSON line on stdout: {line:?}: {e}"));
        match value["event"].as_str() {
            Some("doctor.check") => checks += 1,
            Some("doctor.summary") => summaries += 1,
            other => panic!("unexpected event {other:?} in {line:?}"),
        }
    }
    assert!(checks >= 1, "expected at least one doctor.check event");
    assert_eq!(summaries, 1, "expected exactly one doctor.summary event");
}

/// Writes `files` as `(relative path, contents)` under a new temporary
/// directory.
fn project(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory");
    for (path, contents) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the directory");
        std::fs::write(path, contents).expect("the file");
    }
    dir
}

/// The `doctor.check` events of a `--json` run, by id and path.
fn checks(stdout: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("an NDJSON line"))
        .filter(|event| event["event"] == "doctor.check")
        .collect()
}

const ANDROID_NONE: &str = r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="a.b">
    <uses-permission android:name="android.permission.VIBRATE" />
</manifest>
"#;

const ANDROID_REMOVED: &str = r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:tools="http://schemas.android.com/tools" package="a.b">
    <uses-permission android:name="android.permission.INTERNET" tools:node="remove" />
</manifest>
"#;

const ANDROID_ALL: &str = r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="a.b">
    <uses-permission-sdk-23 android:name="android.permission.INTERNET" />
</manifest>
"#;

fn privacy(reasons: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>NSPrivacyTracking</key>
    <false/>
    <key>NSPrivacyAccessedAPITypes</key>
    <array>
        <dict>
            <key>NSPrivacyAccessedAPIType</key>
            <string>NSPrivacyAccessedAPICategoryFileTimestamp</string>
            <key>NSPrivacyAccessedAPITypeReasons</key>
            <array>{reasons}</array>
        </dict>
    </array>
</dict>
</plist>
"#
    )
}

/// `flui doctor` in a FLUI project reads the declarations of the crates it
/// links and refuses a hand-written Android manifest naming what it lacks;
/// each `--manifest` FLUI does not write is checked against the same
/// declarations, with the lines to add, and an Android one that is not
/// Gradle's merged manifest says so. The ids are stable for `--json`
/// readers.
#[test]
fn doctor_lists_what_a_manifest_lacks_of_the_crates_declarations() {
    let lacking_privacy = privacy("<string>3B52.1</string>");
    let full_privacy = privacy("<string>C617.1</string>");
    let dir = project(&[
        (
            "Cargo.toml",
            b"[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
              [workspace]\n[dependencies]\nnet = { path = \"net\" }\n",
        ),
        ("src/lib.rs", b""),
        ("flui.toml", b""),
        (
            "net/Cargo.toml",
            b"[package]\nname = \"net\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
              [package.metadata.flui.declares]\nschema = 1\n\
              [package.metadata.flui.declares.android]\n\
              permissions = [\"android.permission.INTERNET\"]\n\
              [[package.metadata.flui.declares.apple.privacy.accessed-apis]]\n\
              category = \"NSPrivacyAccessedAPICategoryFileTimestamp\"\n\
              reasons = [\"C617.1\"]\n",
        ),
        ("net/src/lib.rs", b""),
        (
            "platforms/android/app/src/main/AndroidManifest.xml",
            ANDROID_NONE.as_bytes(),
        ),
        ("host/none.xml", ANDROID_NONE.as_bytes()),
        ("host/removed.xml", ANDROID_REMOVED.as_bytes()),
        ("host/all.xml", ANDROID_ALL.as_bytes()),
        ("host/merged_manifests/all.xml", ANDROID_ALL.as_bytes()),
        ("host/lacking.xcprivacy", lacking_privacy.as_bytes()),
        ("host/full.xcprivacy", full_privacy.as_bytes()),
        ("host/binary.xcprivacy", b"bplist00\x00\x01"),
    ]);
    let manifests = [
        "host/none.xml",
        "host/removed.xml",
        "host/all.xml",
        "host/merged_manifests/all.xml",
        "host/lacking.xcprivacy",
        "host/full.xcprivacy",
        "host/binary.xcprivacy",
    ];
    let mut command = flui();
    command
        .current_dir(dir.path())
        .args(["--json", "doctor", "--web"]);
    for manifest in manifests {
        command.args(["--manifest", manifest]);
    }
    let output = command.output().expect("flui doctor");
    assert_eq!(
        output.status.code(),
        Some(3),
        "a failing check fails the run"
    );
    let checks = checks(&output.stdout);
    let by_id = |id: &str| -> Vec<&serde_json::Value> {
        checks.iter().filter(|check| check["id"] == id).collect()
    };

    let declarations = by_id("project.declarations");
    assert_eq!(declarations.len(), 1, "{checks:#?}");
    assert_eq!(declarations[0]["status"], "ok");
    assert_eq!(
        declarations[0]["detail"],
        "1 Android permission(s), 1 Apple accessed-API reason(s)"
    );

    let own = by_id("project.android-manifest");
    assert_eq!(own.len(), 1, "{checks:#?}");
    assert_eq!(own[0]["status"], "error");
    let detail = own[0]["detail"].as_str().expect("a detail");
    assert!(
        detail.contains("refuses it") && detail.contains("android.permission.INTERNET (net)"),
        "{detail}"
    );

    let manifest = by_id("project.manifest");
    assert_eq!(manifest.len(), manifests.len(), "{checks:#?}");
    let mut failures = Vec::new();
    // (path, status, a detail or fix fragment, a note)
    let expected: [(&str, &str, &str, bool); 7] = [
        (
            "host/none.xml",
            "error",
            r#"<uses-permission android:name="android.permission.INTERNET" />"#,
            true,
        ),
        (
            "host/removed.xml",
            "error",
            "android.permission.INTERNET (net)",
            true,
        ),
        ("host/all.xml", "ok", "declares everything", true),
        (
            "host/merged_manifests/all.xml",
            "ok",
            "declares everything",
            false,
        ),
        (
            "host/lacking.xcprivacy",
            "error",
            "NSPrivacyAccessedAPICategoryFileTimestamp C617.1 (net)",
            false,
        ),
        ("host/full.xcprivacy", "ok", "declares everything", false),
        (
            "host/binary.xcprivacy",
            "error",
            "plutil -convert xml1",
            false,
        ),
    ];
    for ((path, status, fragment, noted), check) in expected.into_iter().zip(&manifest) {
        let text = format!(
            "{} {}",
            check["detail"].as_str().unwrap_or_default(),
            check["fix"].as_str().unwrap_or_default()
        );
        if check["path"] != path
            || check["status"] != status
            || !text.contains(fragment)
            || check["note"].is_string() != noted
        {
            failures.push(format!("{path}: {check}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
