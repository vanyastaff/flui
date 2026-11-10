//! The presentation's synthetic identity source: released slots come back
//! under a new generation, and a published synthetic identity carries the
//! packed key's reserved domain bit, so it never aliases a render identity.

use flui_foundation::RenderId;
use flui_foundation::generation::{DOMAIN_BIT, Generation};
use flui_semantics::{AccessibilityNodeId, SemanticsIdentitySource};

/// The slot half of a published identity.
fn slot(id: AccessibilityNodeId) -> u64 {
    id.as_u64() & 0xFFFF_FFFF
}

#[test]
fn a_released_identity_returns_its_slot_under_a_new_generation() {
    let mut source = SemanticsIdentitySource::default();
    let ids = source.allocate(2).expect("two identities");
    let (first, second) = (ids[0], ids[1]);
    assert!(source.release(first), "a held identity is released");
    assert!(!source.release(first), "a second release changes nothing");

    let recycled = source
        .allocate(1)
        .expect("an identity from the released slot")[0];
    assert_eq!(
        slot(recycled.accessibility_id()),
        slot(first.accessibility_id()),
        "the released slot is taken again before a fresh one"
    );
    assert_ne!(
        recycled.accessibility_id(),
        first.accessibility_id(),
        "under its next generation"
    );
    assert!(!source.release(first), "a stale identity releases nothing");
    assert!(source.release(recycled));
    assert!(source.release(second));

    // Churn spends no identity space: two live identities at a time keep
    // using the same two slots.
    let mut slots = std::collections::BTreeSet::new();
    for _ in 0..64 {
        let ids = source.allocate(2).expect("churn reuses released slots");
        for id in ids {
            slots.insert(slot(id.accessibility_id()));
            assert!(source.release(id));
        }
    }
    assert_eq!(slots.len(), 2, "churn stays on the released slots");
}

#[test]
fn a_synthetic_identity_never_aliases_a_render_identity() {
    let mut source = SemanticsIdentitySource::default();
    let synthetic = source.allocate(1).expect("an identity")[0].accessibility_id();
    let render = AccessibilityNodeId::from(RenderId::new_gen(0, Generation::FIRST));
    assert_eq!(
        synthetic.as_u64() & DOMAIN_BIT,
        DOMAIN_BIT,
        "a synthetic identity publishes the domain bit"
    );
    assert_eq!(
        render.as_u64() & DOMAIN_BIT,
        0,
        "a render identity never does"
    );
    assert_eq!(
        synthetic.as_u64() & !DOMAIN_BIT,
        render.as_u64(),
        "the first slot and generation of both domains"
    );
    assert_ne!(synthetic, render, "publish different identities");
    assert_eq!(
        RenderId::from_u64(synthetic.as_u64()),
        None,
        "a synthetic identity does not decode as a render identity"
    );
}
