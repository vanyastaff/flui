//! An application data directory is a name every platform's file system
//! accepts as one directory under its data roots, and nothing that leaves
//! them.

use flui_platform::AppDataDir;

#[test]
fn an_app_data_dir_is_one_portable_directory_name() {
    let cases: [(&str, bool); 16] = [
        ("com.example.notes", true),
        ("acme-notes", true),
        ("Acme_Notes2", true),
        ("9lives", true),
        (&"a".repeat(255), true),
        ("", false),
        (&"a".repeat(256), false),
        (".hidden", false),
        ("-flag", false),
        ("../notes", false),
        ("notes/sub", false),
        ("notes\\sub", false),
        ("notes.", false),
        ("con", false),
        ("Con.example", false),
        ("lpt1.notes", false),
    ];
    for (name, accepted) in cases {
        let made = AppDataDir::try_new(name);
        assert_eq!(made.is_ok(), accepted, "{name:?}: {made:?}");
        match made {
            Ok(dir) => assert_eq!(dir.as_str(), name),
            Err(refusal) => assert_eq!(refusal.name(), name),
        }
    }
}
