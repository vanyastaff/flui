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

/// An Android manifest whose activity carries `filters`.
fn android_opening(filters: &str) -> String {
    format!(
        r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:tools="http://schemas.android.com/tools" package="a.b">
    <application>
        <activity android:name="Main">
{filters}
        </activity>
    </application>
</manifest>
"#
    )
}

const SCHEME_FILTER: &str = r#"            <intent-filter>
                <action android:name="android.intent.action.VIEW" />
                <category android:name="android.intent.category.DEFAULT" />
                <category android:name="android.intent.category.BROWSABLE" />
                <data android:scheme="Notes" />
            </intent-filter>"#;

const FILE_FILTER: &str = r#"            <intent-filter>
                <action android:name="android.intent.action.VIEW" />
                <category android:name="android.intent.category.DEFAULT" />
                <data android:mimeType="application/x-notes-note" />
            </intent-filter>"#;

/// A domain filter, verified when `verified`, marked for removal when
/// `removed`.
fn domain_filter(verified: bool, removed: bool) -> String {
    format!(
        r#"            <intent-filter{}{}>
                <action android:name="android.intent.action.VIEW" />
                <category android:name="android.intent.category.DEFAULT" />
                <category android:name="android.intent.category.BROWSABLE" />
                <data android:scheme="https" />
                <data android:host="notes.example.com" />
            </intent-filter>"#,
        if verified {
            r#" android:autoVerify="true""#
        } else {
            ""
        },
        if removed {
            r#" tools:node="remove""#
        } else {
            ""
        },
    )
}

/// An `Info.plist` with `entries` in its top-level dictionary.
fn info_plist(entries: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.example.notes</string>
{entries}
</dict>
</plist>
"#
    )
}

const INFO_SCHEMES: &str = r"    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleURLSchemes</key>
            <array><string>notes</string></array>
        </dict>
    </array>";

const INFO_DOCUMENTS: &str = r"    <key>UTExportedTypeDeclarations</key>
    <array>
        <dict>
            <key>UTTypeIdentifier</key>
            <string>com.example.notes.note</string>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key>
                <array><string>note</string></array>
            </dict>
        </dict>
    </array>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>LSItemContentTypes</key>
            <array><string>com.example.notes.note</string></array>
        </dict>
    </array>";

/// What the application opens is checked in a manifest FLUI does not write:
/// an Android manifest needs a browsable filter per scheme, a verified
/// `https` filter per domain and a `VIEW` filter per MIME type, a filter the
/// merger removes counting as absent; an `Info.plist` needs each scheme in
/// its URL types and each extension in a document type, and is not asked
/// for the domains, which are an entitlement.
#[test]
fn doctor_lists_what_a_manifest_lacks_of_what_the_application_opens() {
    let all_android = android_opening(&format!(
        "{SCHEME_FILTER}\n{}\n{FILE_FILTER}",
        domain_filter(true, false)
    ));
    let unverified = android_opening(&format!(
        "{SCHEME_FILTER}\n{}\n{FILE_FILTER}",
        domain_filter(false, false)
    ));
    let removed = android_opening(&format!(
        "{SCHEME_FILTER}\n{}\n{FILE_FILTER}",
        domain_filter(true, true)
    ));
    let no_scheme = android_opening(&format!("{}\n{FILE_FILTER}", domain_filter(true, false)));
    let all_info = info_plist(&format!("{INFO_SCHEMES}\n{INFO_DOCUMENTS}"));
    let no_documents = info_plist(INFO_SCHEMES);
    let no_schemes = info_plist(INFO_DOCUMENTS);
    let dir = project(&[
        (
            "Cargo.toml",
            b"[package]\nname = \"notes\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n\
              [package.metadata.flui.declares]\nschema = 3\nurl-schemes = [\"notes\"]\n\
              associated-domains = [\"notes.example.com\"]\n\
              [[package.metadata.flui.declares.file-types]]\nextension = \"note\"\n\
              mime = \"application/x-notes-note\"\nname = \"Note\"\n",
        ),
        ("src/lib.rs", b""),
        ("flui.toml", b""),
        ("host/all.xml", all_android.as_bytes()),
        ("host/unverified.xml", unverified.as_bytes()),
        ("host/removed.xml", removed.as_bytes()),
        ("host/no-scheme.xml", no_scheme.as_bytes()),
        ("host/all.plist", all_info.as_bytes()),
        ("host/no-documents.plist", no_documents.as_bytes()),
        ("host/no-schemes.plist", no_schemes.as_bytes()),
    ]);
    // (path, status, a detail or fix fragment)
    let expected: [(&str, &str, &str); 7] = [
        ("host/all.xml", "ok", "declares everything"),
        (
            "host/unverified.xml",
            "error",
            "associated domain notes.example.com (notes)",
        ),
        (
            "host/removed.xml",
            "error",
            r#"<intent-filter android:autoVerify="true">"#,
        ),
        (
            "host/no-scheme.xml",
            "error",
            r#"<data android:scheme="notes" />"#,
        ),
        ("host/all.plist", "ok", "declares everything"),
        (
            "host/no-documents.plist",
            "error",
            "file type .note (application/x-notes-note) (notes)",
        ),
        (
            "host/no-schemes.plist",
            "error",
            "notes in a CFBundleURLTypes entry's CFBundleURLSchemes",
        ),
    ];
    let mut command = flui();
    command
        .current_dir(dir.path())
        .args(["--json", "doctor", "--web"]);
    for (manifest, _, _) in expected {
        command.args(["--manifest", manifest]);
    }
    let output = command.output().expect("flui doctor");
    let checks = checks(&output.stdout);
    let declarations: Vec<&serde_json::Value> = checks
        .iter()
        .filter(|check| check["id"] == "project.declarations")
        .collect();
    assert_eq!(
        declarations
            .first()
            .and_then(|check| check["detail"].as_str()),
        Some(
            "0 Android permission(s), 0 Apple accessed-API reason(s); the application opens 1 \
             URL scheme(s), 1 associated domain(s), 1 file type(s)"
        ),
        "{checks:#?}"
    );
    // Built without flui's `single-instance` feature, the Linux and Windows
    // builds of an application that opens links and files are refused.
    let one_instance = checks
        .iter()
        .find(|check| check["id"] == "project.single-instance")
        .unwrap_or_else(|| panic!("a one-instance check: {checks:#?}"));
    let detail = one_instance["detail"].as_str().unwrap_or_default();
    assert!(
        one_instance["status"] == "error"
            && ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"]
                .iter()
                .all(|target| detail
                    .contains(&format!("opens `notes:` links, `.note` files on {target}")))
            && one_instance["fix"]
                .as_str()
                .is_some_and(|fix| fix.contains("features = [\"single-instance\"]")),
        "{one_instance:#?}"
    );
    let manifests: Vec<&serde_json::Value> = checks
        .iter()
        .filter(|check| check["id"] == "project.manifest")
        .collect();
    assert_eq!(manifests.len(), expected.len(), "{checks:#?}");
    let mut failures = Vec::new();
    for ((path, status, fragment), check) in expected.into_iter().zip(&manifests) {
        let text = format!(
            "{} {}",
            check["detail"].as_str().unwrap_or_default(),
            check["fix"].as_str().unwrap_or_default()
        );
        if check["path"] != path || check["status"] != status || !text.contains(fragment) {
            failures.push(format!("{path}: {check}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Without `--domains` `flui doctor` fetches nothing: no domain is
/// checked, and the declarations naming an associated domain say how to
/// ask for it. Outside a FLUI project `--domains` fails with the reason.
#[test]
fn doctor_checks_the_associated_domains_only_when_asked() {
    let dir = project(&[
        (
            "Cargo.toml",
            b"[package]\nname = \"notes\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n\
              [package.metadata.flui.declares]\nschema = 3\n\
              associated-domains = [\"notes.example.com\"]\n",
        ),
        ("src/lib.rs", b""),
        (
            "flui.toml",
            b"[app]\nname = \"notes\"\nversion = \"0.1.0\"\nid = \"com.example.notes\"\n",
        ),
    ]);
    let output = flui()
        .current_dir(dir.path())
        .args(["--json", "doctor", "--web"])
        .output()
        .expect("flui doctor");
    let inside = checks(&output.stdout);
    let declarations = inside
        .iter()
        .find(|check| check["id"] == "project.declarations")
        .expect("the declarations check");
    assert!(
        declarations["note"]
            .as_str()
            .is_some_and(|note| note.contains("--domains")),
        "{declarations}"
    );
    assert!(
        inside.iter().all(|check| check["url"].is_null()),
        "fetched without --domains: {inside:#?}"
    );

    let elsewhere = tempfile::tempdir().expect("a temporary directory");
    let output = flui()
        .current_dir(elsewhere.path())
        .args(["--json", "doctor", "--web", "--domains"])
        .output()
        .expect("flui doctor");
    let outside = checks(&output.stdout);
    let domains = outside
        .iter()
        .find(|check| check["id"] == "project.domains")
        .expect("the domains check");
    assert_eq!(domains["status"], "error", "{domains}");
    assert!(
        domains["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("no flui.toml")),
        "{domains}"
    );
}
