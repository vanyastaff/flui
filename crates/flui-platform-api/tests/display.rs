use std::collections::HashSet;
use std::time::Duration;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{
    DisplayId, DisplayInfo, DisplayKey, DisplayRegistry, DisplaySnapshot, RefreshPeriod,
    WindowDisplay,
};

fn same_connector_after_replug_keeps_the_id() {
    let registry = DisplayRegistry::new();
    let connector = DisplayKey::Backend("DP-1".into());
    let first = registry.mint(&connector).expect("id");
    let other = registry
        .mint(&DisplayKey::Backend("HDMI-1".into()))
        .expect("id");
    // The connector goes away and comes back; the registry is session state.
    let again = registry.mint(&connector).expect("id");
    assert_eq!(first, again, "replugging a connector changed its identity");
    assert_ne!(first, other);
}

fn two_identical_monitors_get_distinct_ids() {
    let registry = DisplayRegistry::new();
    let left = registry.mint(&DisplayKey::Injected(70)).expect("id");
    let right = registry.mint(&DisplayKey::Injected(71)).expect("id");
    assert_ne!(
        left, right,
        "two monitors of one model on two connectors are two displays"
    );
}

fn swapped_cables_swap_identities() {
    let registry = DisplayRegistry::new();
    let a = DisplayKey::Injected(1);
    let b = DisplayKey::Injected(2);
    let id_a = registry.mint(&a).expect("id");
    let id_b = registry.mint(&b).expect("id");
    // The monitors trade connectors: the identity follows the connector, so the
    // model that sat on `a` now answers `b`'s id.
    assert_eq!(registry.mint(&b), Ok(id_b));
    assert_eq!(registry.mint(&a), Ok(id_a));
    assert_ne!(id_a, id_b);
}

fn unkeyed_output_mints_a_fresh_id_each_time() {
    let registry = DisplayRegistry::new();
    let first = registry.mint(&DisplayKey::Unkeyed).expect("id");
    let second = registry.mint(&DisplayKey::Unkeyed).expect("id");
    assert_ne!(first, second, "an output with no native key was aliased");
}

fn known_key_still_resolves_after_exhaustion() {
    // The id space is 64 bits, so exhaustion itself is exercised in the crate
    // (`exhausted_counter_refuses_without_wrapping`). From outside, what is
    // observable is that ids are issued in order and never repeat.
    let registry = DisplayRegistry::new();
    let ids: Vec<DisplayId> = (0..64)
        .map(|n| registry.mint(&DisplayKey::Injected(n)).expect("id"))
        .collect();
    let distinct: HashSet<DisplayId> = ids.iter().copied().collect();
    assert_eq!(distinct.len(), ids.len(), "an id was issued twice");
    for (n, id) in ids.iter().enumerate() {
        assert_eq!(
            registry.mint(&DisplayKey::Injected(n as u64)),
            Ok(*id),
            "key {n} lost its id"
        );
    }
}

fn snapshot_derives_the_window_display_from_its_id() {
    let registry = DisplayRegistry::new();
    let id = registry.mint(&DisplayKey::Injected(1)).expect("id");
    let unheld = registry.mint(&DisplayKey::Injected(2)).expect("id");
    let bounds = Bounds::new(Point::new(0, 0), Size::new(1920, 1080));
    let display = DisplayInfo::new(id, "Panel", bounds, 1.0)
        .with_primary(true)
        .with_refresh_period(Duration::from_nanos(16_683_333));
    let snapshot = DisplaySnapshot::empty()
        .successor(vec![display.clone()])
        .expect("revision");
    assert_eq!(snapshot.primary(), Some(&display));
    assert_eq!(
        snapshot.window_display(Some(id)),
        Some(WindowDisplay::new(id, display.refresh_period))
    );
    assert_eq!(
        snapshot.window_display(None),
        None,
        "a window naming no display is on an unknown display, not the primary"
    );
    assert_eq!(
        snapshot.window_display(Some(unheld)),
        None,
        "an id the snapshot does not hold is unknown"
    );
    let same = snapshot.successor(vec![display]).expect("revision");
    assert_eq!(
        same.revision(),
        snapshot.revision(),
        "an unchanged sample advanced the revision"
    );
}

fn an_id_from_another_registry_does_not_resolve() {
    let ours = DisplayRegistry::new();
    let theirs = DisplayRegistry::new();
    let key = DisplayKey::Injected(1);
    let our_id = ours.mint(&key).expect("id");
    let their_id = theirs.mint(&key).expect("id");
    assert_eq!(
        our_id.get(),
        their_id.get(),
        "both registries issue the same number first"
    );
    assert_ne!(our_id, their_id, "the issuing registry takes part in Eq");
    assert_eq!(
        HashSet::from([our_id, their_id]).len(),
        2,
        "the issuing registry takes part in Hash"
    );

    let bounds = Bounds::new(Point::new(0, 0), Size::new(1920, 1080));
    let snapshot = DisplaySnapshot::empty()
        .successor(vec![DisplayInfo::new(our_id, "Panel", bounds, 1.0)])
        .expect("revision");
    assert!(snapshot.get(our_id).is_some());
    assert!(
        snapshot.get(their_id).is_none(),
        "another registry's id resolved against this snapshot"
    );
    assert_eq!(
        snapshot.window_display(Some(their_id)),
        None,
        "a window on another registry's display is on an unknown display"
    );
}

#[test]
fn display_identity_contract() {
    crate::run_table(
        "display_identity_contract",
        &[
            (
                "same_connector_after_replug_keeps_the_id",
                same_connector_after_replug_keeps_the_id as fn(),
            ),
            (
                "two_identical_monitors_get_distinct_ids",
                two_identical_monitors_get_distinct_ids as fn(),
            ),
            (
                "swapped_cables_swap_identities",
                swapped_cables_swap_identities as fn(),
            ),
            (
                "unkeyed_output_mints_a_fresh_id_each_time",
                unkeyed_output_mints_a_fresh_id_each_time as fn(),
            ),
            (
                "known_key_still_resolves_after_exhaustion",
                known_key_still_resolves_after_exhaustion as fn(),
            ),
            (
                "snapshot_derives_the_window_display_from_its_id",
                snapshot_derives_the_window_display_from_its_id as fn(),
            ),
            (
                "an_id_from_another_registry_does_not_resolve",
                an_id_from_another_registry_does_not_resolve as fn(),
            ),
        ],
    );
}

fn ratio(numerator: u32, denominator: u32) -> Option<Duration> {
    RefreshPeriod::from_ratio(numerator, denominator)
        .ok()
        .map(RefreshPeriod::get)
}

fn mhz(millihertz: i64) -> Option<Duration> {
    RefreshPeriod::from_millihertz(millihertz)
        .ok()
        .map(RefreshPeriod::get)
}

fn period_of(info: DisplayInfo) -> Option<Duration> {
    info.refresh_period.map(RefreshPeriod::get)
}

fn blank_display() -> DisplayInfo {
    let id = DisplayRegistry::new()
        .mint(&DisplayKey::Injected(1))
        .expect("id");
    DisplayInfo::new(id, "d", Bounds::default(), 1.0)
}

fn zero_rate_is_unknown() {
    assert_eq!(mhz(0), None);
    assert_eq!(ratio(0, 1), None);
    assert_eq!(RefreshPeriod::from_hz(0.0).ok(), None);
}

fn rational_refresh_converts_exactly() {
    assert_eq!(ratio(120, 1), Some(Duration::from_nanos(8_333_333)));
    assert_eq!(
        ratio(240_000, 2000),
        Some(Duration::from_nanos(8_333_333)),
        "an unreduced ratio must give the same period"
    );
}

fn zero_denominator_is_unknown() {
    assert_eq!(ratio(60, 0), None);
}

fn period_below_one_ms_is_unknown() {
    assert_eq!(
        period_of(blank_display().with_refresh_period(Duration::from_micros(999))),
        None
    );
    assert_eq!(mhz(1_000_001), None, "just over 1000 Hz");
    assert_eq!(
        period_of(blank_display().with_refresh_period(Duration::from_millis(1))),
        Some(Duration::from_millis(1)),
        "1000 Hz is the inclusive bound"
    );
}

fn period_above_one_second_is_unknown() {
    assert_eq!(
        period_of(blank_display().with_refresh_period(Duration::from_millis(1001))),
        None
    );
    assert_eq!(mhz(999), None, "just under 1 Hz");
    assert_eq!(
        period_of(blank_display().with_refresh_period(Duration::from_secs(1))),
        Some(Duration::from_secs(1)),
        "1 Hz is the inclusive bound"
    );
    assert_eq!(
        RefreshPeriod::from_ratio(1, u32::MAX).ok(),
        None,
        "a huge denominator must not overflow into a valid period"
    );
}

fn ntsc_59_94_is_kept_exact() {
    let ntsc = ratio(60_000, 1001).expect("period");
    assert_eq!(ntsc, Duration::from_nanos(16_683_333));
    assert_ne!(
        ntsc,
        Duration::from_nanos(16_666_667),
        "59.94 Hz was rounded to 60 Hz"
    );
}

fn wl_output_refresh_zero_or_negative_is_unknown() {
    assert_eq!(mhz(-1), None);
    assert_eq!(mhz(i64::MIN), None);
    assert_eq!(mhz(60_000), Some(Duration::from_nanos(16_666_667)));
}

fn non_finite_rate_is_unknown() {
    for hz in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -60.0] {
        assert_eq!(RefreshPeriod::from_hz(hz).ok(), None, "{hz}");
    }
}

#[test]
fn refresh_value_classification_table() {
    crate::run_table(
        "refresh_value_classification_table",
        &[
            ("zero_rate_is_unknown", zero_rate_is_unknown as fn()),
            (
                "rational_refresh_converts_exactly",
                rational_refresh_converts_exactly as fn(),
            ),
            (
                "zero_denominator_is_unknown",
                zero_denominator_is_unknown as fn(),
            ),
            (
                "period_below_one_ms_is_unknown",
                period_below_one_ms_is_unknown as fn(),
            ),
            (
                "period_above_one_second_is_unknown",
                period_above_one_second_is_unknown as fn(),
            ),
            ("ntsc_59_94_is_kept_exact", ntsc_59_94_is_kept_exact as fn()),
            (
                "wl_output_refresh_zero_or_negative_is_unknown",
                wl_output_refresh_zero_or_negative_is_unknown as fn(),
            ),
            (
                "non_finite_rate_is_unknown",
                non_finite_rate_is_unknown as fn(),
            ),
        ],
    );
}
