//! An HTTP client's authority: equal for every handle over one client's
//! state, and never equal to another's, a dead one included.

use std::sync::Arc;

use flui_platform::http::HttpAuthority;

/// Handles over one state share its authority; two states never do, and an
/// authority whose state is gone matches none of the states allocated after
/// it, even where the allocator would reuse its address.
///
/// Red-check: compare authorities by `Weak::upgrade` and value, or by an
/// address no weak reference holds.
#[test]
fn an_authority_is_its_states_allocation_and_outlives_it() {
    let pool = Arc::new(7_u64);
    assert_eq!(
        HttpAuthority::of(&pool),
        HttpAuthority::of(&Arc::clone(&pool))
    );
    assert_ne!(
        HttpAuthority::of(&pool),
        HttpAuthority::of(&Arc::new(7_u64))
    );

    let dead = HttpAuthority::of(&pool);
    drop(pool);
    for _ in 0..1024 {
        let fresh = Arc::new(7_u64);
        assert_ne!(
            dead,
            HttpAuthority::of(&fresh),
            "a dropped pool's authority never matches a later one"
        );
    }
}
