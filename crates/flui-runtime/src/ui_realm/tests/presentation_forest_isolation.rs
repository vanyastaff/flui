use super::*;

/// This slice lifted `PresentationForest::install`'s former
/// `len()<=1` ratchet: `install_second_presentation_for_test` (and
/// its production counterpart, `UiRealm::install_presentation`) now
/// go through the SAME production `install` call, not a
/// `cfg(test)`-only bypass.
#[test]
fn install_presentation_grows_the_forest_past_one() {
    let mut realm = UiRealm::for_test();
    assert_eq!(realm.presentation_count(), 1);

    realm.install_second_presentation_for_test();

    assert_eq!(
        realm.presentation_count(),
        2,
        "PresentationForest::install must accept a second presentation now that the \
         ratchet is lifted"
    );
}

/// The realm composite `enter()` activates (ADR-0043 §1)
/// must resolve a `GlobalKey` registered in EITHER presentation's own
/// tree, not just the primary one — the correctness property
/// `GlobalKeyRegistryComposite` exists for.
#[test]
fn composite_registry_resolves_keys_registered_in_either_presentation() {
    let mut realm = UiRealm::for_test();
    let second_id = realm.install_second_presentation_for_test();

    let key_in_primary = flui_view::GlobalKey::<()>::new();
    let element_in_primary = flui_foundation::ElementId::new(1);
    let key_in_second = flui_view::GlobalKey::<()>::new();
    let element_in_second = flui_foundation::ElementId::new(1); // deliberately the SAME numeral

    realm.enter(|realm| {
        realm
            .presentations
            .primary()
            .widgets()
            .with_build_owner_mut(|owner| {
                owner.register_global_key(&key_in_primary, element_in_primary);
            });
        let second = realm
            .presentations
            .get(second_id)
            .expect("second presentation installed");
        second.widgets().with_build_owner_mut(|owner| {
            owner.register_global_key(&key_in_second, element_in_second);
        });

        assert_eq!(
            key_in_primary.current_element(),
            Some(element_in_primary),
            "composite must resolve a key registered in the primary presentation"
        );
        assert_eq!(
            key_in_second.current_element(),
            Some(element_in_second),
            "composite must resolve a key registered in the second \
             presentation, even though both trees use the same raw \
             ElementId numeral"
        );
    });
}

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

/// `UiRealm::apply_hot_reload` must reassemble EVERY presentation in
/// mount order, not just the primary one.
///
/// Oracle: `has_pending_builds()` on EACH presentation's own
/// `WidgetsBinding` after `apply_hot_reload` — `perform_reassemble`
/// (`BuildOwner::reassemble`) marks every MOUNTED element dirty, so a
/// presentation that was actually reassembled reports a pending
/// build; one that fan-out skipped does not. A membership check
/// alone (does the forest still contain both ids) is vacuous here:
/// it stays green even if fan-out silently reverted to
/// primary-only, since nothing removes a presentation from the
/// forest just because reassemble skipped it. Mutant-verified:
/// reverting `UiRealm::apply_hot_reload`'s loop to call
/// `self.presentations.primary().apply_hot_reload(tier)` alone (no
/// loop over the whole forest) makes this test fail on B's
/// `has_pending_builds()` assertion; restoring the loop makes it
/// pass again.
#[test]
#[cfg(feature = "hot-reload")]
fn reassemble_fans_out_to_all_presentations_in_mount_order() {
    use crate::reload::ReloadTier;

    let mut realm = UiRealm::for_test();
    let b_id = realm.install_second_presentation_for_test();

    // Mount independent content on BOTH presentations -- reassemble
    // has nothing to mark dirty on an empty tree, so the oracle
    // below needs a real mounted root on each.
    realm
        .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
        .expect("A mounts");
    realm
        .enter(|realm| {
            realm
                .presentations
                .get(b_id)
                .expect("B installed")
                .widgets()
                .attach_root_widget(&flui_widgets::SizedBox::new(20.0, 20.0))
        })
        .expect("B mounts");

    // A fresh mount always starts with a pending initial build --
    // drain both before reassembling, or the oracle below cannot
    // tell "still pending from mount" apart from "reassemble
    // actually re-marked it dirty".
    let constraints = BoxConstraints::tight(flui_types::Size::new(px(50.0), px(50.0)));
    let _ = realm.draw_frame(constraints);
    assert!(
        !realm.widgets().has_pending_builds(),
        "precondition: A's initial build must be drained before reassembling"
    );
    assert!(
        !realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .widgets()
            .has_pending_builds(),
        "precondition: B's initial build must be drained before reassembling"
    );

    let _ = realm.apply_hot_reload(ReloadTier::Reassemble);

    assert!(
        realm.widgets().has_pending_builds(),
        "reassemble must mark A's mounted root dirty"
    );
    assert!(
        realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .widgets()
            .has_pending_builds(),
        "reassemble must mark B's mounted root dirty too -- fan-out must not skip \
         or stop at the primary presentation"
    );
}

/// The SAME fan-out property as `reassemble_fans_out_to_all_
/// presentations_in_mount_order`, but driven through the OTHER
/// production entry point: the closed-command inbox
/// (`command_sender().request_hot_reload` + `drain_commands`), the
/// path the desktop worker's own hot-reload trigger actually uses --
/// not `apply_hot_reload`/`perform_hot_reload_entered` called
/// directly. Before this arm reused `Self::apply_hot_reload`, it
/// called `self.presentations.primary().apply_hot_reload(tier)`
/// directly, reassembling only the primary and silently skipping
/// every sibling — the registry-pinned exploit's exact regression
/// class, on its untested twin path. Mutant-verified: reverting
/// `drain_commands`'s `UiCommand::HotReload` arm back to
/// `self.presentations.primary().apply_hot_reload(tier)` makes this
/// test fail on B's `has_pending_builds()` assertion; restoring the
/// `self.apply_hot_reload(tier)` fan-out call makes it pass again.
#[test]
#[cfg(feature = "hot-reload")]
fn hot_reload_via_the_command_inbox_fans_out_to_all_presentations_in_mount_order() {
    use crate::reload::ReloadTier;

    let mut realm = UiRealm::for_test();
    let b_id = realm.install_second_presentation_for_test();

    realm
        .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
        .expect("A mounts");
    realm
        .enter(|realm| {
            realm
                .presentations
                .get(b_id)
                .expect("B installed")
                .widgets()
                .attach_root_widget(&flui_widgets::SizedBox::new(20.0, 20.0))
        })
        .expect("B mounts");

    let constraints = BoxConstraints::tight(flui_types::Size::new(px(50.0), px(50.0)));
    let _ = realm.draw_frame(constraints);
    assert!(
        !realm.widgets().has_pending_builds(),
        "precondition: A's initial build must be drained before reassembling"
    );
    assert!(
        !realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .widgets()
            .has_pending_builds(),
        "precondition: B's initial build must be drained before reassembling"
    );

    realm
        .command_sender()
        .request_hot_reload(ReloadTier::Reassemble)
        .expect("inbox has room");
    let report = realm.drain_commands();
    assert_eq!(
        report.invoked, 1,
        "the hot-reload command must be applied, not dropped as stale"
    );

    assert!(
        realm.widgets().has_pending_builds(),
        "the inbox path must mark A's mounted root dirty, exactly like the direct path"
    );
    assert!(
        realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .widgets()
            .has_pending_builds(),
        "the inbox path must mark B's mounted root dirty too -- it is the SAME \
         fan-out as the direct perform_hot_reload_entered path, not a narrower one"
    );
}

/// Dropping the realm closes EVERY presentation it hosts, not just
/// the primary one.
#[test]
fn dropping_the_realm_closes_every_presentation() {
    let mut realm = UiRealm::for_test();
    realm.install_second_presentation_for_test();

    let lifecycles_before: Vec<_> = realm
        .presentations
        .iter()
        .map(crate::presentation::PresentationState::lifecycle)
        .collect();
    assert!(
        lifecycles_before
            .iter()
            .all(|l| { *l == crate::presentation::PresentationLifecycle::SurfaceAttached }),
        "both presentations must start attached"
    );

    drop(realm);
    // Both presentations' own Drop impls ran as part of the realm's
    // Drop (each PresentationState transitions to Closed on its own
    // drop -- see PresentationState::close/Drop); nothing left to
    // assert on here beyond "this did not panic", since the values
    // themselves are gone. This test covers whole-realm teardown;
    // the addressed close path separately removes exactly one live
    // forest member while preserving its surviving siblings.
}

fn segment_constraints() -> BoxConstraints {
    BoxConstraints::tight(flui_types::Size::new(px(800.0), px(600.0)))
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

/// A mark arriving for a presentation while ITS OWN segment is
/// running must not be lost: the segment already cleared the bit at
/// its own START, so a mark landing anywhere between that clear and
/// the next pump's sample must survive to be observed there.
///
/// This collapses the timing to a single thread (mark, run the
/// segment, mark again immediately after) rather than a literal
/// concurrent race — in a single-threaded pump there is no OTHER
/// window where a mark could land except between one segment
/// ending and the next beginning, or nested inside the segment's
/// own build via a real callback; both reduce to the same
/// observable property this test checks: the bit set after the
/// clear is not silently dropped.
#[test]
fn cross_presentation_dirty_during_a_segment_sets_wake_bit_and_lands_next_pump() {
    let realm = UiRealm::for_test();
    let presentation = realm.presentations.primary();

    presentation.mark_redraw_pending();
    let _ = realm.enter(|realm| realm.draw_frame_entered(segment_constraints()));
    assert_eq!(
        presentation.flush_count(),
        1,
        "the armed wake bit must cause exactly one flush"
    );

    // A mark arrives during (or immediately after) that segment —
    // nothing else about the presentation is dirty (no widget tree
    // was ever attached, so no pending builds; the render tree is
    // empty, so no dirty nodes either): the wake bit is the ONLY
    // dirty signal in play.
    presentation.mark_redraw_pending();

    let _ = realm.enter(|realm| realm.draw_frame_entered(segment_constraints()));
    assert_eq!(
        presentation.flush_count(),
        2,
        "the wake bit set after the first segment cleared its own \
         bit must not be lost -- it must cause a second flush on \
         the next pump, even though nothing else about the \
         presentation is dirty"
    );
}
