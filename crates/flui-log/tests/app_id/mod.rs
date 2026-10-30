//! An application identifier is accepted only in the grammar every platform
//! takes as the application's name, and it is what the Apple subsystem
//! reports.

use flui_log::{AppId, AppIdentity, IdentityError, UNIDENTIFIED_APPLE_SUBSYSTEM};

#[test]
fn an_app_id_is_a_reverse_dns_name_every_platform_accepts() {
    let long = format!("com.{}", "a".repeat(251));
    let too_long = format!("com.{}", "a".repeat(252));
    let cases: [(&str, Option<IdentityError>); 14] = [
        ("com.example.notes", None),
        ("com.example.my_app", None),
        ("dev.Flui.App2", None),
        (&long, None),
        (
            "notes",
            Some(IdentityError::NotReverseDns("notes".to_owned())),
        ),
        (
            "com..notes",
            Some(IdentityError::EmptyLabel {
                identifier: "com..notes".to_owned(),
                position: 1,
            }),
        ),
        (
            "com.example.",
            Some(IdentityError::EmptyLabel {
                identifier: "com.example.".to_owned(),
                position: 2,
            }),
        ),
        (
            "com.my-company.notes",
            Some(IdentityError::InvalidCharacter {
                identifier: "com.my-company.notes".to_owned(),
                character: '-',
            }),
        ),
        (
            "com.example.2notes",
            Some(IdentityError::LabelStartsWithNonLetter {
                identifier: "com.example.2notes".to_owned(),
                position: 2,
            }),
        ),
        (
            "com.example.new",
            Some(IdentityError::ReservedLabel {
                identifier: "com.example.new".to_owned(),
                label: "new".to_owned(),
            }),
        ),
        (&too_long, Some(IdentityError::TooLong(too_long.clone()))),
        (
            "Aux.notes",
            Some(IdentityError::ReservedLabel {
                identifier: "Aux.notes".to_owned(),
                label: "Aux".to_owned(),
            }),
        ),
        (
            "com1.example",
            Some(IdentityError::ReservedLabel {
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
}

#[test]
fn the_apple_subsystem_is_the_app_id_or_the_unidentified_one() {
    let unidentified = AppIdentity::try_new("Notes").expect("a display name");
    assert_eq!(unidentified.app_id(), None);
    assert_eq!(unidentified.apple_subsystem(), UNIDENTIFIED_APPLE_SUBSYSTEM);

    let app_id = AppId::try_new("com.example.notes").expect("a reverse-DNS identifier");
    let identified = unidentified.with_app_id(app_id.clone());
    assert_eq!(identified.app_id(), Some(&app_id));
    assert_eq!(identified.apple_subsystem(), "com.example.notes");
    assert_eq!(identified.display_name(), "Notes");
}
