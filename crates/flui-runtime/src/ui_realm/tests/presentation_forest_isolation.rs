use super::*;

/// `PresentationState::new` wires `RealmCapabilities::
/// global_key_scope` into `BuildOwner::set_global_key_scope` FIRST —
/// before this presentation's own focus/IME wiring, before
/// attach/mount (see that constructor's own doc comment). The pin:
/// two presentations of ONE realm, sharing that exact scope,
/// mounting the IDENTICAL `GlobalKey` must conflict eagerly —
/// `GlobalKeyScope`'s cross-owner uniqueness domain — naming both
/// owner tags in the panic (`global_key_scope.rs::claim_and_register`'s
/// sole conflict branch).
///
/// Unpinned before this test: deleting the `set_global_key_scope`
/// call from `PresentationState::new` leaves both the flui-view and
/// flui-app suites green, because each presentation's `BuildOwner`
/// then lazily creates its OWN private, single-tenant scope on first
/// use (`claim_and_register`'s `get_or_insert_with` fallback) — the
/// two owners never actually share one scope, so mounting the same
/// key in both silently succeeds in each instead of conflicting.
/// Mutant-verified: removing that one setter call from
/// `PresentationState::new` makes this test fail (no panic);
/// restoring it makes the eager panic fire again.
#[test]
#[should_panic(expected = "is already claimed by")]
fn duplicate_global_key_across_presentations_panics_eagerly_naming_both_owners() {
    let mut realm = UiRealm::for_test();
    let b_id = realm.install_second_presentation_for_test();

    let key = flui_view::GlobalKey::<()>::new();
    let element_in_a = flui_foundation::ElementId::new(1);
    let element_in_b = flui_foundation::ElementId::new(2);

    realm.widgets().with_build_owner_mut(|owner| {
        owner.register_global_key(&key, element_in_a);
    });

    // B shares A's realm's exact GlobalKeyScope
    // (install_second_presentation_for_test wires it that way, the
    // same capability-threading order PresentationState::new uses
    // in production) -- mounting the SAME key here must conflict
    // eagerly, never silently succeed in a second, private scope.
    realm
        .presentation_widgets_for_test(b_id)
        .with_build_owner_mut(|owner| {
            owner.register_global_key(&key, element_in_b);
        });
}

fn segment_constraints() -> BoxConstraints {
    BoxConstraints::tight(flui_foundation::geometry::Size::new(800.0, 600.0))
}

/// Oracle: FLUSH COUNTS (`PresentationState::flush_count`), never
/// rebuild counts — a presentation with a settled, never-rebuilding
/// tree still flushes every segment its own dirty state (or wake
/// bit) causes to run, and a presentation that was never dirtied
/// must NOT flush just because its sibling did.
#[test]
fn sibling_presentations_flush_independently() {
    let mut realm = UiRealm::for_test();
    let second_id = realm.install_second_presentation_for_test();

    // Mark ONLY the second presentation dirty — reaching its own
    // wake bit directly, the same seam a later addressed-routing
    // slice would call through (no addressed entry point exists
    // yet).
    realm
        .presentations
        .get(second_id)
        .expect("second presentation installed")
        .mark_redraw_pending();

    let _ = realm.enter(|realm| realm.draw_frame_entered(segment_constraints()));

    assert_eq!(
        realm.presentations.primary().flush_count(),
        0,
        "the primary presentation was never marked dirty and must \
         not flush just because its sibling did"
    );
    assert_eq!(
        realm
            .presentations
            .get(second_id)
            .expect("still installed")
            .flush_count(),
        1,
        "the second presentation's own wake bit must cause exactly \
         one flush"
    );

    // A second pump with nothing newly dirty must flush neither —
    // both presentations are now settled.
    let _ = realm.enter(|realm| realm.draw_frame_entered(segment_constraints()));
    assert_eq!(
        realm.presentations.primary().flush_count(),
        0,
        "still never dirtied, still zero flushes"
    );
    assert_eq!(
        realm
            .presentations
            .get(second_id)
            .expect("still installed")
            .flush_count(),
        1,
        "a settled presentation must not flush again with nothing \
         new to do"
    );
}
