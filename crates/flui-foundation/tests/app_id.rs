//! An application identifier is accepted only in the grammar every platform
//! takes as the application's name, and only as long as the tightest name it is
//! part of; an application without one is known by its executable.

use flui_foundation::{APP_ID_MAX_LEN, AppId, AppIdError, executable_class};

#[test]
fn an_app_id_is_a_reverse_dns_name_every_platform_accepts() {
    let long = format!("com.{}", "a".repeat(APP_ID_MAX_LEN - 4));
    let too_long = format!("com.{}", "a".repeat(APP_ID_MAX_LEN - 3));
    let cases: [(&str, Option<AppIdError>); 14] = [
        ("com.example.notes", None),
        ("com.example.my_app", None),
        ("dev.Flui.App2", None),
        (&long, None),
        ("notes", Some(AppIdError::NotReverseDns("notes".to_owned()))),
        (
            "com..notes",
            Some(AppIdError::EmptyLabel {
                identifier: "com..notes".to_owned(),
                position: 1,
            }),
        ),
        (
            "com.example.",
            Some(AppIdError::EmptyLabel {
                identifier: "com.example.".to_owned(),
                position: 2,
            }),
        ),
        (
            "com.my-company.notes",
            Some(AppIdError::InvalidCharacter {
                identifier: "com.my-company.notes".to_owned(),
                character: '-',
            }),
        ),
        (
            "com.example.2notes",
            Some(AppIdError::LabelStartsWithNonLetter {
                identifier: "com.example.2notes".to_owned(),
                position: 2,
            }),
        ),
        (
            "com.example.new",
            Some(AppIdError::ReservedLabel {
                identifier: "com.example.new".to_owned(),
                label: "new".to_owned(),
            }),
        ),
        (&too_long, Some(AppIdError::TooLong(too_long.clone()))),
        (
            "Aux.notes",
            Some(AppIdError::ReservedLabel {
                identifier: "Aux.notes".to_owned(),
                label: "Aux".to_owned(),
            }),
        ),
        (
            "com1.example",
            Some(AppIdError::ReservedLabel {
                identifier: "com1.example".to_owned(),
                label: "com1".to_owned(),
            }),
        ),
        ("com.aux.notes", None),
    ];
    for (identifier, refusal) in cases {
        match (AppId::try_new(identifier), refusal) {
            (Ok(app_id), None) => assert_eq!(app_id.as_str(), identifier),
            (Err(error), Some(expected)) => assert_eq!(error, expected, "{identifier}"),
            (made, expected) => panic!("{identifier}: made {made:?}, expected {expected:?}"),
        }
    }
    assert_eq!(
        format!(r"\\.\pipe\flui-{long}-4294967295").len(),
        256,
        "the longest identifier names the longest Windows pipe"
    );
    assert!(
        format!("{long}.desktop").len() <= 255,
        "and a desktop entry"
    );
}

/// An application without an id is known by its executable's stem in ASCII,
/// cut to the identifier's bound, never empty.
#[test]
fn an_executable_is_classed_in_ascii() {
    let long = "a".repeat(APP_ID_MAX_LEN + 10);
    let cases = [
        ("notes", "notes".to_owned()),
        ("notes-app.bin_2", "notes-app.bin_2".to_owned()),
        ("nötes", "n_tes".to_owned()),
        ("my notes", "my_notes".to_owned()),
        ("", "flui".to_owned()),
        (long.as_str(), "a".repeat(APP_ID_MAX_LEN)),
    ];
    for (stem, class) in cases {
        assert_eq!(executable_class(stem), class, "{stem:?}");
    }
}
