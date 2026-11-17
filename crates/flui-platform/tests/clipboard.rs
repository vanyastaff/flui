//! The clipboard write's contract: how long an expiring entry may live, and
//! what an expiring write is (ADR-0362).

use std::time::Duration;

use flui_platform::{ClipboardExpiry, ClipboardExpiryError, ClipboardWrite};

/// An expiry is a second at least and a day at most: zero, a moment no one
/// can paste in and a time past any clock's day are refused.
#[test]
fn an_expiry_is_at_least_a_second_and_at_most_a_day() {
    for (after, expected) in [
        (Duration::ZERO, Err(ClipboardExpiryError::TooShort)),
        (
            Duration::from_millis(999),
            Err(ClipboardExpiryError::TooShort),
        ),
        (ClipboardExpiry::MIN, Ok(ClipboardExpiry::MIN)),
        (Duration::from_secs(30), Ok(Duration::from_secs(30))),
        (ClipboardExpiry::MAX, Ok(ClipboardExpiry::MAX)),
        (
            ClipboardExpiry::MAX + Duration::from_nanos(1),
            Err(ClipboardExpiryError::TooLong),
        ),
        (Duration::MAX, Err(ClipboardExpiryError::TooLong)),
    ] {
        assert_eq!(
            ClipboardExpiry::try_new(after).map(ClipboardExpiry::get),
            expected,
            "{after:?}"
        );
    }
}

/// An expiring write is a concealed one with its expiry and an entry no
/// other write names; a clone names the same entry, concealing it again
/// keeps it expiring, and its description never shows the text.
#[test]
fn an_expiring_write_is_concealed_and_names_its_own_entry() {
    let thirty = ClipboardExpiry::try_new(Duration::from_secs(30)).expect("a valid expiry");
    let ordinary = ClipboardWrite::text("hunter2");
    assert_eq!((ordinary.expiry(), ordinary.entry()), (None, None));
    let concealed = ClipboardWrite::text("hunter2").concealed();
    assert!(concealed.is_concealed());
    assert_eq!((concealed.expiry(), concealed.entry()), (None, None));

    let first = ClipboardWrite::text("hunter2").expires_after(thirty);
    let second = ClipboardWrite::text("hunter2").expires_after(thirty);
    assert!(first.is_concealed(), "an expiring write is concealed");
    assert_eq!(first.expiry(), Some(thirty));
    assert!(first.entry().is_some());
    assert_ne!(first.entry(), second.entry(), "each write its own entry");
    assert_eq!(
        first.clone().entry(),
        first.entry(),
        "a clone is the same entry"
    );
    let again = first.clone().concealed();
    assert_eq!(
        (again.expiry(), again.entry()),
        (first.expiry(), first.entry()),
        "concealing an expiring write keeps it expiring"
    );
    let shown = format!("{first:?}");
    assert!(!shown.contains("hunter2"), "{shown}");
}
