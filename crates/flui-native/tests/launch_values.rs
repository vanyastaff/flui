//! A platform reads the values its process was launched with under one key on every host
//! (ADR-0288 §7): where it reads the environment, the variable the key spells.

use flui_native::headless_platform;
use flui_platform::LaunchKey;

#[test]
fn a_headless_platform_reads_launch_values_from_the_environment() {
    let platform = headless_platform();
    // Set wherever the tests run, so nothing here writes the process's environment.
    let path = LaunchKey::try_new("path").expect("a valid key");
    let expected = std::env::var("PATH").ok().filter(|value| !value.is_empty());
    assert!(expected.is_some(), "the tests run with a PATH");
    assert_eq!(platform.launch_value(path), expected);

    let unset = LaunchKey::try_new("flui.test.never_launched_with").expect("a valid key");
    assert_eq!(
        std::env::var_os(unset.environment_variable()),
        None,
        "the variable is not set"
    );
    assert_eq!(platform.launch_value(unset), None);
}
