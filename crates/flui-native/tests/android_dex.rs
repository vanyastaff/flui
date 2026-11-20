//! The Android host's checked-in dex (`android/classes.dex`) is the one
//! compiled from its Java sources (ADR-0247 item 1, ADR-0377 §2), and the
//! rules by which a crate's declarations and sources are read.

use std::path::Path;

#[path = "support/declared_dex.rs"]
mod declared_dex;

use declared_dex::{DeclaredDex, Record, declared_dex, sources_digest};

#[test]
fn the_checked_in_dex_is_built_from_its_java_sources() {
    declared_dex::assert_declared_dex_is_built_from_its_sources(Path::new(env!(
        "CARGO_MANIFEST_DIR"
    )));
}

#[test]
fn the_source_digest_covers_paths_and_bytes() {
    let one = vec![("a/A.java".to_owned(), b"class A {}".to_vec())];
    let renamed = vec![("a/B.java".to_owned(), b"class A {}".to_vec())];
    let edited = vec![("a/A.java".to_owned(), b"class A { }".to_vec())];
    let split = vec![
        ("a/A.java".to_owned(), b"class".to_vec()),
        ("a/A.java".to_owned(), b" A {}".to_vec()),
    ];
    let digest = sources_digest(&one);
    assert_ne!(digest, sources_digest(&renamed));
    assert_ne!(digest, sources_digest(&edited));
    assert_ne!(digest, sources_digest(&split));
    // What the workflow's `printf 'a/A.java\0class A {}\0' | sha256sum` prints.
    assert_eq!(
        digest,
        "9ca59dc02e868874e974de7e02ec9b96f433a0a396a92173ed39be17e201d629"
    );
}

/// The dex come from what the manifest declares, at the top and under its
/// features; a manifest that declares neither key has none, and one that
/// declares one key of the pair is refused.
#[test]
fn the_declared_dex_are_what_the_manifest_declares() {
    let manifest = |text: &str| -> toml::Table { text.parse().expect("the manifest is TOML") };
    let declared = manifest(
        r#"
        [package.metadata.flui.declares]
        schema = 4
        android = { dex = "android/classes.dex", java = "android/java" }
        [package.metadata.flui.declares.features.camera.android]
        dex = "./camera/classes.dex"
        java = "camera/java"
        [package.metadata.flui.declares.features.http.android]
        permissions = ["android.permission.INTERNET"]
        "#,
    );
    assert_eq!(
        declared_dex(&declared).expect("declared"),
        [
            DeclaredDex {
                sources: "android/java".to_owned(),
                dex: "android/classes.dex".to_owned(),
                record: "android/classes.dex.toml".to_owned(),
            },
            DeclaredDex {
                sources: "camera/java".to_owned(),
                dex: "camera/classes.dex".to_owned(),
                record: "camera/classes.dex.toml".to_owned(),
            },
        ]
    );
    let none = manifest("[package.metadata.flui]\ntier = \"pkg\"\n");
    assert_eq!(
        declared_dex(&none).expect("none"),
        Vec::<DeclaredDex>::new()
    );
    let half = manifest("[package.metadata.flui.declares.android]\ndex = \"a.dex\"\n");
    assert!(declared_dex(&half).is_err());
    let not_a_path = manifest("[package.metadata.flui.declares.android]\ndex = 1\njava = \"j\"\n");
    assert!(declared_dex(&not_a_path).is_err());
}

/// A record names both digests and the tools that compiled the dex, which
/// the workflow's `rebuild` mode compiles with again.
#[test]
fn a_record_names_its_digests_and_tools() {
    let full = "sources = \"aa\"\ndex = \"bb\"\njavac = \"21.0.12.1\"\n\
                build-tools = \"35.0.0\"\nplatform = \"android-35\"\nmin-api = 23\n";
    assert_eq!(
        Record::parse(full).expect("parses"),
        Record {
            sources: "aa".to_owned(),
            dex: "bb".to_owned(),
        }
    );
    for key in ["sources", "dex", "javac", "build-tools", "platform"] {
        let without = full
            .lines()
            .filter(|line| !line.starts_with(&format!("{key} =")))
            .collect::<Vec<&str>>()
            .join("\n");
        assert!(
            Record::parse(&without).is_err(),
            "a record without `{key}` is refused"
        );
    }
}
