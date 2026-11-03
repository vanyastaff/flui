//! A window's id comes from the mint of the host that opened it, which never
//! issues one twice (ADR-0039 §4).

use flui_platform::WindowIdMint;

/// Each id a mint issues is new: increasing from 1, so a window opened after
/// another closed never gets the closed one's id.
#[test]
fn a_window_id_mint_issues_every_id_once() {
    let ids = WindowIdMint::new();
    let issued: Vec<_> = (0..64)
        .map(|_| ids.next().expect("the mint issues an id"))
        .collect();
    assert_eq!(issued[0].get().get(), 1, "the first id is 1, never 0");
    for pair in issued.windows(2) {
        assert!(
            pair[0].get() < pair[1].get(),
            "{:?} after {:?}",
            pair[1],
            pair[0]
        );
    }
    // The id means nothing outside the host that issued it: another host's
    // mint starts over.
    assert_eq!(
        WindowIdMint::new().next().expect("an id"),
        issued[0],
        "a fresh mint starts over"
    );
}
