//! The Apple subsystem is the application identifier, or the fixed unidentified
//! one (the identifier's grammar is `flui-foundation`'s, tested there).

use flui_foundation::AppId;
use flui_log::{AppIdentity, UNIDENTIFIED_APPLE_SUBSYSTEM};

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
