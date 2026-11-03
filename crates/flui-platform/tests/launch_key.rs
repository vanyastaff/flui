//! A launch key is one name an Android intent extra and an environment variable can both be
//! spelled from.

use flui_platform::{LaunchKey, MAX_LAUNCH_KEY_LEN};

#[test]
fn a_launch_key_is_a_dotted_lowercase_name_with_an_environment_spelling() {
    let longest: &'static str = "a".repeat(MAX_LAUNCH_KEY_LEN).leak();
    let too_long: &'static str = "a".repeat(MAX_LAUNCH_KEY_LEN + 1).leak();
    let cases: [(&'static str, Option<&str>); 14] = [
        ("flui.agent.endpoint", Some("FLUI_AGENT_ENDPOINT")),
        (
            "flui.agent.exit_on_disconnect",
            Some("FLUI_AGENT_EXIT_ON_DISCONNECT"),
        ),
        ("path", Some("PATH")),
        ("a1.b_2", Some("A1_B_2")),
        (longest, Some(longest.to_ascii_uppercase().leak())),
        ("", None),
        (too_long, None),
        (".flui", None),
        ("flui.", None),
        ("flui..agent", None),
        ("1flui", None),
        ("_flui", None),
        ("Flui.agent", None),
        ("flui-agent", None),
    ];
    for (key, environment) in cases {
        let parsed = LaunchKey::try_new(key);
        // A key read at run time is checked by the same rules.
        assert_eq!(LaunchKey::is_spelling(key), parsed.is_some(), "{key:?}");
        assert_eq!(
            parsed.map(LaunchKey::environment_variable).as_deref(),
            environment,
            "{key:?}"
        );
        if let Some(parsed) = parsed {
            assert_eq!(parsed.as_str(), key);
        }
    }
}
