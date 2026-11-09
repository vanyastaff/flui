//! Every `GapId` names an entry of `docs/gaps.md` by its heading's anchor, and
//! the entries carry no number a reference could drift with.

use std::collections::BTreeSet;

use flui_native::testing::gap::GapId;

const GAPS: &str = include_str!("../../../docs/gaps.md");

/// The `### ` headings of the document's "Entries" section.
fn entry_headings() -> Vec<&'static str> {
    GAPS.lines()
        .skip_while(|line| *line != "## Entries")
        .skip(1)
        .take_while(|line| !line.starts_with("## "))
        .filter_map(|line| line.strip_prefix("### "))
        .collect()
}

/// A heading's anchor as GitHub renders it: lowercase, punctuation dropped,
/// each space a hyphen.
fn anchor(heading: &str) -> String {
    heading
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

#[test]
fn every_gap_id_is_a_gaps_entry() {
    let anchors: BTreeSet<String> = entry_headings().into_iter().map(anchor).collect();
    assert!(
        !anchors.is_empty(),
        "docs/gaps.md has no entries under `## Entries`"
    );
    for gap in GapId::ALL {
        assert!(
            anchors.contains(gap.slug()),
            "{gap:?} names `{gap}`, which no heading of docs/gaps.md renders as: \
             rename the slug with the heading"
        );
    }
}

#[test]
fn gaps_entries_are_named_by_unique_headings() {
    let headings = entry_headings();
    let mut seen = BTreeSet::new();
    for heading in &headings {
        let rank = heading.split_once(". ").map(|(rank, _)| rank);
        assert!(
            rank.is_none_or(|rank| rank.parse::<u32>().is_err()),
            "`### {heading}` carries its rank: an entry is named by its heading alone, \
             and its rank is its place in the list"
        );
        assert!(
            seen.insert(anchor(heading)),
            "`### {heading}` renders the anchor of an earlier entry"
        );
    }
}
