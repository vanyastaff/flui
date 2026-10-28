use std::collections::HashSet;
use std::time::Duration;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{
    CapabilityAbsence, ColorGamut, DisplayColor, DisplayId, DisplayInfo, DisplayKey,
    DisplayRegistry, DisplaySnapshot, DynamicRange, Headroom, InvalidHeadroom, RefreshPeriod,
    SurfaceColorSpace, UnsupportedReason, WindowDisplay,
};

fn same_connector_after_replug_keeps_the_id() {
    let registry = DisplayRegistry::new();
    let connector = DisplayKey::WaylandOutput("DP-1".into());
    let first = registry.mint(&connector).expect("id");
    let other = registry
        .mint(&DisplayKey::WaylandOutput("HDMI-1".into()))
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

/// A measured interval is a period as it stands, and one that describes no
/// cadence (not finite, not positive, outside 1 ms to 1 s) is unknown.
fn measured_interval_is_the_period() {
    for (seconds, nanos) in [
        (1.0 / 60.0, Some(16_666_666)),
        (0.001, Some(1_000_000)),
        (1.0, Some(1_000_000_000)),
        (0.0, None),
        (-1.0 / 60.0, None),
        (0.0002, None),
        (2.0, None),
        (f64::NAN, None),
        (f64::INFINITY, None),
        (f64::NEG_INFINITY, None),
    ] {
        let period = RefreshPeriod::from_secs_f64(seconds).ok();
        match nanos {
            Some(nanos) => assert!(
                period.is_some_and(|period| period.get().abs_diff(Duration::from_nanos(nanos))
                    <= Duration::from_nanos(1)),
                "{seconds} s is {period:?}"
            ),
            None => assert_eq!(period, None, "{seconds} s"),
        }
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
            (
                "measured_interval_is_the_period",
                measured_interval_is_the_period as fn(),
            ),
        ],
    );
}

fn panel(id: DisplayId) -> DisplayInfo {
    DisplayInfo::new(
        id,
        "Panel",
        Bounds::new(Point::new(0, 0), Size::new(1920, 1080)),
        1.0,
    )
}

fn headroom_refuses_what_is_not_a_finite_multiple_of_white() {
    for refused in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, 0.99, -2.0] {
        assert_eq!(
            Headroom::try_new(refused),
            Err(InvalidHeadroom),
            "{refused} is no headroom"
        );
    }
    assert_eq!(Headroom::try_new(1.0), Ok(Headroom::SDR));
    assert_eq!(Headroom::try_new(4.5).map(Headroom::get), Ok(4.5));
}

fn headroom_from_luminance_is_the_peak_over_white() {
    assert_eq!(
        Headroom::from_luminance(1000.0, 250.0).map(Headroom::get),
        Ok(4.0)
    );
    assert_eq!(
        Headroom::from_luminance(400.0, 480.0),
        Ok(Headroom::SDR),
        "white set above the reported peak is no headroom, not a fraction"
    );
    for (peak, white) in [
        (0.0, 80.0),
        (1000.0, 0.0),
        (f32::NAN, 80.0),
        (1000.0, f32::INFINITY),
        (-5.0, 80.0),
        // The ratio overflows to infinity.
        (f32::MAX, f32::MIN_POSITIVE),
    ] {
        assert_eq!(
            Headroom::from_luminance(peak, white),
            Err(InvalidHeadroom),
            "{peak} nits over {white} nits must not publish a headroom"
        );
    }
}

fn display_colour_queries_answer_unsupported_with_the_reason() {
    let registry = DisplayRegistry::new();
    let unread = panel(registry.mint(&DisplayKey::Injected(1)).expect("id"));
    assert_eq!(
        unread.color_space().map_err(|refused| refused.reason),
        Err(UnsupportedReason::NotYetOnThisHost),
        "a display whose host did not read its colour says so"
    );

    let browser = unread.clone().with_color(Ok(DisplayColor::high(
        ColorGamut::DisplayP3,
        Err(CapabilityAbsence::NotOnThisPlatform),
    )));
    assert_eq!(
        browser.color_space(),
        Ok(SurfaceColorSpace::new(
            ColorGamut::DisplayP3,
            DynamicRange::High
        ))
    );
    assert_eq!(
        browser.headroom().map_err(|refused| refused.reason),
        Err(UnsupportedReason::NotOnThisPlatform),
        "a high-range display without a reported peak answers why"
    );

    let sdr = unread
        .clone()
        .with_color(Ok(DisplayColor::standard(ColorGamut::Srgb)));
    assert_eq!(
        sdr.color_space(),
        Ok(SurfaceColorSpace::new(
            ColorGamut::Srgb,
            DynamicRange::Standard
        ))
    );
    assert_eq!(sdr.headroom(), Ok(Headroom::SDR));

    let absent = unread.with_color(Err(CapabilityAbsence::NotOnThisDevice));
    assert_eq!(
        absent.headroom().map_err(|refused| refused.reason),
        Err(UnsupportedReason::NotOnThisDevice)
    );
}

fn a_profile_change_is_a_new_revision() {
    let registry = DisplayRegistry::new();
    let id = registry.mint(&DisplayKey::Injected(1)).expect("id");
    let sdr = panel(id).with_color(Ok(DisplayColor::standard(ColorGamut::Srgb)));
    let first = DisplaySnapshot::empty()
        .successor(vec![sdr.clone()])
        .expect("revision");
    let three = Headroom::try_new(3.0).expect("headroom");
    let hdr = sdr.with_color(Ok(DisplayColor::high(ColorGamut::Rec2020, Ok(three))));
    let second = first.successor(vec![hdr.clone()]).expect("revision");
    assert!(
        second.revision() > first.revision(),
        "HDR switching on is a display change"
    );
    assert_eq!(second.get(id).map(DisplayInfo::headroom), Some(Ok(three)));
    let again = second.successor(vec![hdr]).expect("revision");
    assert_eq!(
        again.revision(),
        second.revision(),
        "the same profile sampled again is no change"
    );
}

#[test]
fn display_colour_table() {
    crate::run_table(
        "display_colour_table",
        &[
            (
                "headroom_refuses_what_is_not_a_finite_multiple_of_white",
                headroom_refuses_what_is_not_a_finite_multiple_of_white as fn(),
            ),
            (
                "headroom_from_luminance_is_the_peak_over_white",
                headroom_from_luminance_is_the_peak_over_white as fn(),
            ),
            (
                "display_colour_queries_answer_unsupported_with_the_reason",
                display_colour_queries_answer_unsupported_with_the_reason as fn(),
            ),
            (
                "a_profile_change_is_a_new_revision",
                a_profile_change_is_a_new_revision as fn(),
            ),
        ],
    );
}
