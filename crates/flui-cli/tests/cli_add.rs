//! Integration tests for `flui add`.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;

/// The `flui` binary, offline: every fixture crate is a path dependency.
fn flui() -> Command {
    let mut command = cargo_bin_cmd!("flui");
    command.env("CARGO_NET_OFFLINE", "true");
    command
}

/// A crate's manifest; `rest` follows its `[package]` table.
fn manifest(name: &str, rest: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{rest}")
}

/// A FLUI checkout in miniature, beside two applications: `app` on the
/// facade, whose `system` feature enables the capability crate `flui-sys`
/// and `plain` a crate that is not one; `bare` on `flui-core` alone, from
/// the checkout that also has the capability crate `flui-direct`.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let capability = "[package.metadata.flui.declares]\nschema = 1\n\
                      [package.metadata.flui.declares.android]\n\
                      permissions = [\"android.permission.INTERNET\"]\n\
                      [[package.metadata.flui.declares.apple.privacy.accessed-apis]]\n\
                      category = \"NSPrivacyAccessedAPICategoryFileTimestamp\"\n\
                      reasons = [\"C617.1\"]\n";
    let files = [
        (
            "ws/Cargo.toml",
            "[workspace]\nmembers = [\"flui\", \"sys\", \"plain\", \"core\", \"direct\"]\n\
             resolver = \"3\"\n"
                .to_owned(),
        ),
        (
            "ws/flui/Cargo.toml",
            manifest(
                "flui",
                "[dependencies]\nflui-sys = { path = \"../sys\", optional = true }\n\
                 plain = { path = \"../plain\", optional = true }\n\
                 [features]\nsystem = [\"dep:flui-sys\"]\nplain = [\"dep:plain\"]\n",
            ),
        ),
        (
            "ws/sys/Cargo.toml",
            manifest(
                "flui-sys",
                &format!(
                    "[package.metadata.flui.capability]\nplugin = \"SysPlugin\"\n{capability}"
                ),
            ),
        ),
        ("ws/plain/Cargo.toml", manifest("plain", "")),
        ("ws/core/Cargo.toml", manifest("flui-core", "")),
        (
            "ws/direct/Cargo.toml",
            manifest(
                "flui-direct",
                "[package.metadata.flui.capability]\nplugin = \"services::DirectPlugin\"\n",
            ),
        ),
        (
            "app/Cargo.toml",
            manifest(
                "app",
                "[workspace]\n[dependencies]\nflui = { path = \"../ws/flui\" }\n",
            ),
        ),
        (
            "bare/Cargo.toml",
            manifest(
                "bare",
                "[workspace]\n[dependencies]\nflui-core = { path = \"../ws/core\" }\n",
            ),
        ),
    ];
    for (path, contents) in files {
        let path = dir.path().join(path);
        let crate_dir = path.parent().expect("a crate directory");
        std::fs::create_dir_all(crate_dir.join("src")).expect("the directories");
        std::fs::write(crate_dir.join("src/lib.rs"), "").expect("the source");
        std::fs::write(&path, contents).expect("the manifest");
    }
    dir
}

/// The one `add.done` event of a `--json` run.
fn added(stdout: &[u8]) -> serde_json::Value {
    let events: Vec<serde_json::Value> = String::from_utf8_lossy(stdout)
        .lines()
        .map(|line| serde_json::from_str(line).expect("an NDJSON line"))
        .collect();
    let [event] = events.as_slice() else {
        panic!("one event: {events:#?}");
    };
    assert_eq!(event["event"], "add.done");
    event.clone()
}

/// `flui add system` enables the facade feature that links the capability
/// crate and prints the line that registers its plugin, with what the crate
/// brings to the manifests; a second run changes nothing; a feature whose
/// crate states no plugin is refused and undone; an unknown name lists what
/// the facade offers; without the facade, the crate is added from the
/// checkout the application's FLUI dependencies come from.
#[test]
fn add_links_a_capability_and_shows_its_registration() {
    let dir = fixture();
    let app = dir.path().join("app");
    let read = |path: &str| std::fs::read_to_string(dir.path().join(path)).expect("a file");

    let output = flui()
        .current_dir(&app)
        .args(["--json", "add", "system"])
        .output()
        .expect("flui add");
    assert!(output.status.success(), "{output:?}");
    let event = added(&output.stdout);
    assert_eq!(event["crate"], "flui-sys");
    assert_eq!(event["feature"], "system");
    assert_eq!(event["changed"], true);
    assert_eq!(
        event["snippet"],
        ".plugin(flui::services::system::SysPlugin::default())"
    );
    assert_eq!(
        event["android_permissions"],
        serde_json::json!(["android.permission.INTERNET"])
    );
    assert_eq!(
        event["apple_accessed_apis"],
        serde_json::json!([{
            "category": "NSPrivacyAccessedAPICategoryFileTimestamp",
            "reason": "C617.1",
        }])
    );
    let enabled = read("app/Cargo.toml");
    assert!(
        enabled.contains("flui = { path = \"../ws/flui\", features = [\"system\"] }"),
        "{enabled}"
    );

    let again = flui()
        .current_dir(&app)
        .args(["--json", "add", "system"])
        .output()
        .expect("flui add");
    assert!(again.status.success(), "{again:?}");
    assert_eq!(added(&again.stdout)["changed"], false);
    assert_eq!(read("app/Cargo.toml"), enabled);

    let lock = read("app/Cargo.lock");
    let plain = flui()
        .current_dir(&app)
        .args(["add", "plain"])
        .output()
        .expect("flui add");
    assert!(!plain.status.success(), "{plain:?}");
    let stderr = String::from_utf8_lossy(&plain.stderr);
    assert!(stderr.contains("not a capability crate"), "{stderr}");
    assert_eq!(read("app/Cargo.toml"), enabled, "the feature is undone");
    assert_eq!(read("app/Cargo.lock"), lock, "the lockfile is undone");

    let unknown = flui()
        .current_dir(&app)
        .args(["add", "location"])
        .output()
        .expect("flui add");
    assert_eq!(unknown.status.code(), Some(2), "{unknown:?}");
    let stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(
        stderr.contains("no capability feature `location`; it offers system"),
        "{stderr}"
    );

    let bare = flui()
        .current_dir(dir.path().join("bare"))
        .args(["--json", "add", "flui-direct"])
        .output()
        .expect("flui add");
    assert!(bare.status.success(), "{bare:?}");
    let event = added(&bare.stdout);
    assert_eq!(event["feature"], serde_json::Value::Null);
    assert_eq!(
        event["snippet"],
        ".plugin(flui_direct::services::DirectPlugin::default())"
    );
    let manifest = read("bare/Cargo.toml");
    assert!(
        manifest.contains("flui-direct") && manifest.contains("ws/direct"),
        "{manifest}"
    );
}
