//! The crate's checked-in dex (`android/classes.dex`) is the one compiled
//! from its Java sources (ADR-0247 item 1, ADR-0377 §2). The check is
//! `flui-native`'s, which also holds the tests of its rules: one reading of
//! the digest the `Android dex` workflow writes.

use std::path::Path;

#[path = "../../flui-native/tests/support/declared_dex.rs"]
mod declared_dex;

#[test]
fn the_checked_in_dex_is_built_from_its_java_sources() {
    declared_dex::assert_declared_dex_is_built_from_its_sources(Path::new(env!(
        "CARGO_MANIFEST_DIR"
    )));
}
