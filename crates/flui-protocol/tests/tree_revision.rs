//! A tree's revision only grows: two different publishes never read as one revision.

use flui_protocol::tree::TreeRevision;

#[test]
fn each_revision_follows_the_one_before() {
    let first = TreeRevision::ZERO.next();
    assert_eq!(first.get(), 1);
    assert!(first.next() > first);
    assert_eq!(TreeRevision::default(), TreeRevision::ZERO);
}

#[cfg(feature = "serde")]
#[test]
#[should_panic(expected = "BUG: a semantics tree's revision space is spent")]
fn the_last_revision_has_no_next() {
    let last: TreeRevision =
        serde_json::from_str(&u64::MAX.to_string()).expect("a revision reads as its number");
    let _ = last.next();
}
