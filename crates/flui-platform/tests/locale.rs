use flui_platform::Locale;

#[test]
fn an_explicit_script_determines_text_direction_before_the_language() {
    for (tag, expected_rtl) in [
        ("az-Arab", true),
        ("ckb-Arab", true),
        ("ar-Latn", false),
        ("he-Latn", false),
        ("en-Arab", true),
        ("fa-Latn", false),
    ] {
        let locale = tag.parse::<Locale>().expect("valid test locale");
        assert_eq!(locale.is_rtl(), expected_rtl, "{tag}");
        assert_eq!(locale.is_ltr(), !expected_rtl, "{tag}");
    }
}

#[test]
fn a_locale_without_a_script_uses_the_shared_rtl_language_table() {
    for language in [
        "ar", "ckb", "dv", "fa", "he", "ks", "ps", "sd", "ug", "ur", "yi",
    ] {
        let locale = Locale::try_new(language, None::<&str>).expect("valid test language");
        assert!(locale.is_rtl(), "{language}");
    }
    for language in ["az", "en", "fr", "ku"] {
        let locale = Locale::try_new(language, None::<&str>).expect("valid test language");
        assert!(locale.is_ltr(), "{language}");
    }
}
