//! How a locator's text criterion compares (ADR-0238 §4): exact, contains and
//! prefix, case-sensitive unless told otherwise, folded by Unicode when it is.

use flui_protocol::TextMatch;

#[test]
fn a_text_match_compares_the_way_it_says() {
    let table: &[(&str, TextMatch, &str, bool)] = &[
        (
            "exact, the whole text",
            TextMatch::exact("Save"),
            "Save",
            true,
        ),
        (
            "exact is not a part",
            TextMatch::exact("Save"),
            "Save as",
            false,
        ),
        (
            "contains, in the middle",
            TextMatch::contains("ve a"),
            "Save as",
            true,
        ),
        (
            "contains, not there",
            TextMatch::contains("ve b"),
            "Save as",
            false,
        ),
        (
            "prefix, at the start",
            TextMatch::prefix("Sav"),
            "Save",
            true,
        ),
        (
            "prefix is not a part",
            TextMatch::prefix("ave"),
            "Save",
            false,
        ),
        (
            "case matters by default",
            TextMatch::exact("save"),
            "Save",
            false,
        ),
        (
            "ignoring case",
            TextMatch::exact("save").ignoring_case(),
            "SAVE",
            true,
        ),
        (
            "sharp s folds to ss",
            TextMatch::exact("STRASSE").ignoring_case(),
            "Straße",
            true,
        ),
        (
            "ss folds to sharp s",
            TextMatch::exact("straße").ignoring_case(),
            "STRASSE",
            true,
        ),
        (
            "accented capitals fold",
            TextMatch::exact("école").ignoring_case(),
            "ÉCOLE",
            true,
        ),
        (
            "final and medial sigma fold together",
            TextMatch::exact("σας").ignoring_case(),
            "ΣΑΣ",
            true,
        ),
        (
            "greek folding in a prefix",
            TextMatch::prefix("ΣΑ").ignoring_case(),
            "σας",
            true,
        ),
        (
            "composed and decomposed stay apart",
            TextMatch::exact("\u{e9}"),
            "e\u{301}",
            false,
        ),
        (
            "the empty text is in every text",
            TextMatch::contains(""),
            "anything",
            true,
        ),
    ];
    for (row, criterion, text, expected) in table {
        assert_eq!(criterion.matches(text), *expected, "{row}");
    }
}

#[test]
fn a_criterion_names_the_text_it_looks_for() {
    assert_eq!(TextMatch::prefix("Sav").ignoring_case().needle(), "Sav");
}
