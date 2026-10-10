//! The text pipeline end to end: a styled span measured by `TextPainter`
//! records a paragraph on the canvas.

use flui_foundation::geometry::Offset;
use flui_painting::typography::{FontWeight, TextDirection, TextSpan, TextStyle};
use flui_painting::{Canvas, TextPainter};

fn invalid_font_size_does_not_unwind(font_size: f64, scale: f64) {
    let fonts = flui_painting::FontCollection::new();
    fonts
        .register_font(include_bytes!("../assets/fonts/Roboto-Regular.ttf"))
        .expect("the fixture font loads");
    let mut context = flui_painting::TextContext::new(&fonts);
    let valid = TextSpan::styled("AAA", TextStyle::new().with_font_size(16.0));
    let mut painter = TextPainter::new()
        .with_text(valid.clone())
        .with_text_direction(TextDirection::Ltr)
        .with_text_scale_factor(scale);
    painter
        .layout(&mut context, 0.0, 400.0)
        .expect("valid initial layout");
    let expected = painter.size();
    painter.set_text(Some(
        TextSpan::styled("AAA", TextStyle::new().with_font_size(font_size)).into(),
    ));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        painter.layout(&mut context, 0.0, 400.0)
    }));
    let result = result.expect("an invalid authored/resolved size must not panic");
    assert!(matches!(
        result,
        Err(flui_painting::TextMeasurementError::Layout(
            flui_painting::TextLayoutError::InvalidFontSize { .. }
        ))
    ));
    assert!(
        !painter.has_layout(),
        "failed text cannot expose current geometry"
    );
    painter.set_text(Some(valid.into()));
    painter
        .layout(&mut context, 0.0, 400.0)
        .expect("layout recovers after invalid input");
    assert_eq!(painter.size(), expected);
    assert!(matches!(
        painter.layout(&mut context, 0.0, f64::MAX),
        Err(flui_painting::TextMeasurementError::Layout(
            flui_painting::TextLayoutError::InvalidWidth { .. }
        ))
    ));
    assert!(
        !painter.has_layout(),
        "failed constraints cannot retain the previous cache"
    );
    for (min_width, max_width) in [(-1e-50, 400.0), (0.0, -1e-50)] {
        painter
            .layout(&mut context, 0.0, 400.0)
            .expect("valid layout before a negative subnormal width");
        assert!(
            matches!(
                painter.layout(&mut context, min_width, max_width),
                Err(flui_painting::TextMeasurementError::Layout(
                    flui_painting::TextLayoutError::InvalidWidth { .. }
                ))
            ),
            "a negative width must not become accepted negative zero after narrowing"
        );
        assert!(!painter.has_layout());
    }
}

pub(crate) fn authored_font_size_overflow_is_an_ordinary_error() {
    invalid_font_size_does_not_unwind(f64::MAX, 1.0);
}

pub(crate) fn scaled_font_size_overflow_is_an_ordinary_error() {
    invalid_font_size_does_not_unwind(f64::MAX, 64.0);
}

pub(crate) fn font_size_narrowing_to_zero_is_an_ordinary_error() {
    invalid_font_size_does_not_unwind(1e-50, 1.0);
}

pub(crate) fn changing_to_an_invalid_scale_cannot_reuse_successful_geometry() {
    let fonts = flui_painting::FontCollection::new();
    let mut context = flui_painting::TextContext::new(&fonts);
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("AAA"))
        .with_text_direction(TextDirection::Ltr);
    for factor in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        painter.set_text_scale_factor(1.0);
        painter
            .layout(&mut context, 0.0, 400.0)
            .expect("valid baseline layout");
        painter.set_text_scale_factor(factor);
        assert!(
            matches!(
                painter.layout(&mut context, 0.0, 400.0),
                Err(flui_painting::TextMeasurementError::Layout(
                    flui_painting::TextLayoutError::InvalidScale { .. }
                ))
            ),
            "an invalid authored scale must be rejected: {factor}"
        );
        assert!(!painter.has_layout());
    }
}

pub(crate) fn exact_profiles_reach_every_measurement_and_painted_run() {
    use flui_foundation::{
        TextScaleProfile as Profile, TextSize, TextSizeRequest, TextSizingIntent,
    };
    use flui_painting::styling::Color;
    use flui_painting::{DrawOp, TextBaseline, TextMeasurement, TextSizing};
    let request = |size, profile| TextSizeRequest {
        size: TextSize::new(size).expect("authored size"),
        profile,
    };
    let sizing = TextSizing::exact([
        (
            request(14.0, Profile::Body),
            TextSize::new(24.25).expect("body"),
        ),
        (
            request(14.0, Profile::Headline),
            TextSize::new(21.5).expect("headline"),
        ),
        (
            request(14.0, Profile::Caption1),
            TextSize::new(17.75).expect("caption"),
        ),
        (
            request(18.25, Profile::Body),
            TextSize::new(28.125).expect("fractional"),
        ),
    ])
    .expect("consistent capture");
    let style = |size, profile| {
        TextStyle::new()
            .with_font_size(size)
            .with_sizing(TextSizingIntent::Profile(profile))
    };
    let authored = TextSpan::styled("AAA", style(14.0, Profile::Body))
        .with_child(
            TextSpan::styled("BBB", style(14.0, Profile::Headline)).with_child(TextSpan::styled(
                "CCC",
                TextStyle::new().with_color(Color::RED),
            )),
        )
        .with_child(TextSpan::styled("DDD", style(14.0, Profile::Caption1)))
        .with_child(TextSpan::styled("EEE", style(18.25, Profile::Body)));
    let expected = TextSpan::styled("AAA", TextStyle::new().with_font_size(24.25))
        .with_child(
            TextSpan::styled("BBB", TextStyle::new().with_font_size(21.5)).with_child(
                TextSpan::styled("CCC", TextStyle::new().with_color(Color::RED)),
            ),
        )
        .with_child(TextSpan::styled(
            "DDD",
            TextStyle::new().with_font_size(17.75),
        ))
        .with_child(TextSpan::styled(
            "EEE",
            TextStyle::new().with_font_size(28.125),
        ));
    let mut context = text_cx();
    let mut painter = TextPainter::new()
        .with_text(authored)
        .with_text_direction(TextDirection::Ltr);
    let mut reference = TextPainter::new()
        .with_text(expected)
        .with_text_direction(TextDirection::Ltr);
    reference
        .layout(&mut context, 0.0, 400.0)
        .expect("independent resolved reference");
    let min = reference
        .min_intrinsic_width(&mut context)
        .expect("reference min");
    let max = reference
        .max_intrinsic_width(&mut context)
        .expect("reference max");
    let mut measurement = TextMeasurement::new(&mut context, &sizing);
    assert_eq!(
        measurement.min_intrinsic_width(&painter).expect("cold min"),
        min
    );
    assert_eq!(
        measurement.max_intrinsic_width(&painter).expect("cold max"),
        max
    );
    assert_eq!(
        measurement.dry_size(&painter, 0.0, 400.0).expect("dry"),
        reference.size()
    );
    assert_eq!(
        measurement
            .intrinsic_height(&painter, 400.0)
            .expect("height"),
        reference.height()
    );
    assert_eq!(
        measurement
            .dry_baseline(&painter, 0.0, 400.0, TextBaseline::Alphabetic)
            .expect("baseline"),
        Some(reference.compute_distance_to_actual_baseline(TextBaseline::Alphabetic))
    );
    measurement
        .layout(&mut painter, 0.0, 400.0)
        .expect("exact layout");
    assert_eq!(painter.size(), reference.size());
    assert_eq!(
        measurement
            .max_intrinsic_width(&painter)
            .expect("cached max"),
        max
    );
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);
    let display = canvas.finish();
    let paragraph = display
        .iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(paragraph),
            _ => None,
        })
        .expect("actual painted paragraph");
    let mut registry = flui_painting::glyphs::FontRegistry::new();
    let observed: Vec<_> = paragraph
        .runs()
        .map(|run| {
            let key = registry.prepare_run(&run).expect("painted face");
            let glyph = run
                .placed_glyphs(key, (0.0, 0.0), 1.0)
                .next()
                .expect("painted glyph");
            (glyph.key.size(), glyph.color)
        })
        .collect();
    for size in [24.25, 21.5, 17.75, 28.125] {
        assert!(
            observed.iter().any(|(actual, _)| *actual == size),
            "paint missing resolved size {size}: {observed:?}"
        );
    }
    assert!(
        observed.contains(&(21.5, Some(Color::RED))),
        "color-only descendant inherits Headline independently of size"
    );
    let fixed = TextSizing::fixed();
    TextMeasurement::new(&mut context, &fixed)
        .layout(&mut painter, 0.0, 400.0)
        .expect("replace capture without changing fonts or authored text");
    assert!(painter.width() < reference.width());
    painter.set_text_sizing(Some(sizing.clone()));
    painter
        .layout(&mut context, 0.0, 400.0)
        .expect("direct painter uses its accepted exact authority");
    assert_eq!(painter.size(), reference.size());
    painter.set_text_sizing(None);
    painter.set_text_scale_factor(1.0);
    TextMeasurement::new(&mut context, &sizing)
        .layout(&mut painter, 0.0, 400.0)
        .expect("explicit one overrides inheritance");
    let explicit = painter.size();
    painter
        .layout(&mut context, 0.0, 400.0)
        .expect("legacy entrypoint agrees with explicit override");
    assert_eq!(painter.size(), explicit);
}

pub(crate) fn exact_frontier_is_complete_and_does_not_publish_stale_geometry() {
    use flui_foundation::{
        TextScaleProfile as Profile, TextSize, TextSizeRequest, TextSizingIntent,
    };
    use flui_painting::{TextMeasurement, TextMeasurementError, TextSizing};
    let (empty, mut admission) = TextSizing::captured();
    let source = admission.source();
    let mut context = text_cx();
    let authored = TextSpan::new("unstyled")
        .with_child(TextSpan::styled(
            "caption",
            TextStyle::new()
                .with_font_size(12.25)
                .with_sizing(TextSizingIntent::Profile(Profile::Caption1)),
        ))
        .with_child(TextSpan::styled(
            "fixed",
            TextStyle::new()
                .with_font_size(18.0)
                .with_sizing(TextSizingIntent::Fixed),
        ));
    let mut painter = TextPainter::new()
        .with_text(authored)
        .with_text_direction(TextDirection::Ltr);
    painter
        .layout(&mut context, 0.0, 400.0)
        .expect("committed baseline");
    let TextMeasurementError::Pending(frontier) = TextMeasurement::new(&mut context, &empty)
        .max_intrinsic_width(&painter)
        .expect_err("probe must not return old cache")
    else {
        panic!("ordinary layout error is not preparation debt");
    };
    assert!(
        painter.has_layout(),
        "transient failure retains committed geometry"
    );
    assert_eq!(frontier.policy_source, source);
    assert_eq!(
        frontier.requests,
        [
            TextSizeRequest {
                size: TextSize::new(14.0).expect("default"),
                profile: Profile::Body
            },
            TextSizeRequest {
                size: TextSize::new(12.25).expect("caption"),
                profile: Profile::Caption1
            },
        ]
    );
    assert!(matches!(
        TextMeasurement::new(&mut context, &empty).layout(&mut painter, 0.0, 400.0),
        Err(TextMeasurementError::Pending(_))
    ));
    assert!(
        !painter.has_layout(),
        "failed current layout withdraws previous cache"
    );
    admission
        .admit(frontier.requests.iter().copied().map(|request| {
            (
                request,
                TextSize::new(request.size.value() * 1.5).expect("answer"),
            )
        }))
        .expect("consistent answers");
    TextMeasurement::new(&mut context, &empty)
        .layout(&mut painter, 0.0, 400.0)
        .expect("captured answers resume shaping");
    let mut empty_text = TextPainter::new()
        .with_text(TextSpan::new(""))
        .with_text_direction(TextDirection::Ltr);
    let unanswered = TextSizing::exact([]).expect("separate closed capture");
    let TextMeasurementError::Pending(frontier) = TextMeasurement::new(&mut context, &unanswered)
        .layout(&mut empty_text, 0.0, 400.0)
        .expect_err("empty paragraphs still request their default line size")
    else {
        panic!("expected preparation debt");
    };
    assert_eq!(frontier.requests.len(), 1);
    assert_eq!(frontier.requests[0].profile, Profile::Body);
}

pub(crate) fn captured_answers_reach_retained_clones_without_reshaping_old_geometry() {
    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_painting::{DrawOp, TextMeasurement, TextMeasurementError, TextSizing};
    let request = |size| TextSizeRequest {
        size: TextSize::new(size).expect("authored size"),
        profile: TextScaleProfile::Body,
    };
    let first = (request(14.0), TextSize::new(21.0).expect("first answer"));
    let second = (request(24.0), TextSize::new(30.5).expect("second answer"));
    let (sizing, mut admission) = TextSizing::captured();
    admission.admit([first]).expect("initial answers");
    let fonts = flui_painting::FontCollection::new();
    fonts
        .register_font(include_bytes!("../assets/fonts/Roboto-Regular.ttf"))
        .expect("fixture font");
    let mut context = flui_painting::TextContext::new(&fonts);
    let painter = |size| {
        TextPainter::new()
            .with_text(TextSpan::styled(
                "retained answer",
                TextStyle::new()
                    .with_font_family("Roboto")
                    .with_font_size(size),
            ))
            .with_text_direction(TextDirection::Ltr)
            .with_text_sizing(Some(sizing.clone()))
    };
    let mut old = painter(14.0);
    let mut pending = painter(24.0);
    old.layout(&mut context, 0.0, 400.0)
        .expect("known initial answer");
    let mut before = Canvas::new();
    old.paint(&mut before, Offset::ZERO);
    let before = before.finish();
    let old_paragraph = before
        .iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(paragraph.clone()),
            _ => None,
        })
        .expect("actual initial paragraph");
    assert!(matches!(
        pending.layout(&mut context, 0.0, 400.0),
        Err(TextMeasurementError::Pending(_))
    ));
    admission.admit([first, second]).expect("extended answers");
    TextMeasurement::new(&mut context, &sizing)
        .layout(&mut pending, 0.0, 400.0)
        .expect("a retained policy clone must receive native answer extension");
    TextMeasurement::new(&mut context, &sizing)
        .layout(&mut old, 0.0, 400.0)
        .expect("unrelated answer extension preserves successful layout");
    let mut after = Canvas::new();
    old.paint(&mut after, Offset::ZERO);
    pending.paint(&mut after, Offset::new(0.0, 100.0));
    let after = after.finish();
    let paragraphs: Vec<_> = after
        .iter()
        .filter_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(paragraph),
            _ => None,
        })
        .collect();
    assert_eq!(paragraphs.len(), 2);
    assert!(std::sync::Arc::ptr_eq(&old_paragraph, paragraphs[0]));
    let mut registry = flui_painting::glyphs::FontRegistry::new();
    let sizes: Vec<_> = paragraphs[1]
        .runs()
        .map(|run| {
            let key = registry.prepare_run(&run).expect("actual glyph run");
            run.placed_glyphs(key, (0.0, 0.0), 1.0)
                .next()
                .expect("actual painted glyph")
                .key
                .size()
        })
        .collect();
    assert_ne!(sizes, [] as [f32; 0]);
    assert!(sizes.iter().all(|size| *size == 30.5));
}

pub(crate) fn captured_live_geometry_rejects_conflicts_after_warm_eviction() {
    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_painting::{TextMeasurementError, TextSizing, TextSizingAdmissionError};
    let request = |size| TextSizeRequest {
        size: TextSize::new(size).expect("authored size"),
        profile: TextScaleProfile::Body,
    };
    let first = request(14.0);
    let fresh = request(24.0);
    let (sizing, mut admission) = TextSizing::captured();
    admission.set_warm_capacity(0);
    let cohort = sizing.begin_cohort();
    let attempt = cohort.policy();
    assert!(matches!(
        attempt.resolve(first),
        Err(TextMeasurementError::Pending(_))
    ));
    admission
        .admit([(first, TextSize::new(21.0).expect("answer"))])
        .expect("initial answer");
    let mut context = text_cx();
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new("live geometry"))
        .with_text_direction(TextDirection::Ltr)
        .with_text_sizing(Some(sizing.clone()));
    painter
        .layout(&mut context, 0.0, 400.0)
        .expect("live paragraph");
    let previous = painter.size();
    drop(cohort);
    assert!(matches!(
        admission.admit([
            (fresh, TextSize::new(30.5).expect("fresh answer")),
            (first, TextSize::new(28.0).expect("contradiction")),
        ]),
        Err(TextSizingAdmissionError::Conflict(_))
    ));
    assert_eq!(
        sizing
            .resolve(first)
            .expect("live answer survives eviction")
            .value(),
        21.0
    );
    assert!(matches!(
        sizing.resolve(fresh),
        Err(TextMeasurementError::Pending(_))
    ));
    painter
        .layout(&mut context, 0.0, 400.0)
        .expect("old paragraph remains authoritative");
    assert_eq!(painter.size(), previous);
    admission
        .admit([(first, TextSize::new(21.0).expect("same answer"))])
        .expect("identical repeat");
    let sealed = TextSizing::exact([(first, TextSize::new(28.0).expect("other capture"))])
        .expect("independent sealed capture");
    assert_ne!(sealed, sizing);
    painter.set_text_sizing(Some(sealed));
    painter
        .layout(&mut context, 0.0, 400.0)
        .expect("capture replacement");
    assert!(painter.height() > previous.height);
}

pub(crate) fn captured_cohorts_converge_beyond_warm_capacity_and_retire_history() {
    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_painting::{TextMeasurement, TextMeasurementError, TextSizing};
    let request = |size| TextSizeRequest {
        size: TextSize::new(size).expect("authored size"),
        profile: TextScaleProfile::Body,
    };
    let (sizing, mut admission) = TextSizing::captured();
    admission.set_warm_capacity(2);
    let mut context = text_cx();
    let make_painter = |size| {
        TextPainter::new()
            .with_text(TextSpan::styled(
                "finite cohort",
                TextStyle::new().with_font_size(size),
            ))
            .with_text_direction(TextDirection::Ltr)
            .with_text_sizing(Some(sizing.clone()))
    };
    let inputs = [11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0, 20.0];
    let outputs = [
        21.0, 22.5, 23.25, 24.0, 25.5, 26.25, 27.0, 28.5, 29.25, 30.0,
    ];
    let painters: Vec<_> = inputs.into_iter().map(make_painter).collect();
    let cohort = sizing.begin_cohort();
    let attempt = cohort.policy();
    for (painter, (input, output)) in painters.iter().zip(inputs.into_iter().zip(outputs)) {
        assert!(matches!(
            TextMeasurement::new(&mut context, &attempt).dry_size(painter, 0.0, 140.0),
            Err(TextMeasurementError::Pending(_))
        ));
        admission
            .admit([(request(input), TextSize::new(output).expect("table answer"))])
            .expect("admit frontier");
        assert!(
            TextMeasurement::new(&mut context, &attempt)
                .max_intrinsic_width(painter)
                .expect("intrinsic frontier")
                .is_finite()
        );
    }
    for painter in &painters {
        let measurement = TextMeasurement::new(&mut context, &attempt)
            .dry_size(painter, 0.0, 140.0)
            .expect("whole finite cohort converges");
        assert!(measurement.width.is_finite() && measurement.height > 0.0);
    }
    drop(cohort);
    assert!(matches!(
        sizing.resolve(request(inputs[0])),
        Err(TextMeasurementError::Pending(_))
    ));
    for step in 0..300 {
        let authored = 30.0 + f64::from(step) / 16.0;
        let cohort = sizing.begin_cohort();
        let attempt = cohort.policy();
        assert!(matches!(
            attempt.resolve(request(authored)),
            Err(TextMeasurementError::Pending(_))
        ));
        admission
            .admit([(
                request(authored),
                TextSize::new(authored + 5.0).expect("fractional answer"),
            )])
            .expect("ongoing animation has no cumulative admission cap");
        let painter = make_painter(authored);
        TextMeasurement::new(&mut context, &attempt)
            .dry_size(&painter, 0.0, 140.0)
            .expect("fractional measurement");
        drop(cohort);
    }
    assert!(matches!(
        sizing.resolve(request(30.0)),
        Err(TextMeasurementError::Pending(_))
    ));
    assert_eq!(
        sizing
            .resolve(request(30.0 + 299.0 / 16.0))
            .expect("recent warm answer")
            .value(),
        35.0 + 299.0 / 16.0
    );
}

pub(crate) fn captured_cohorts_do_not_pin_other_attempts_history() {
    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_painting::{TextMeasurementError, TextSizing};
    let request = |size| TextSizeRequest {
        size: TextSize::new(size).expect("authored size"),
        profile: TextScaleProfile::Body,
    };
    let (sizing, mut admission) = TextSizing::captured();
    admission.set_warm_capacity(2);
    let suspended = sizing.begin_cohort();
    let suspended_policy = suspended.policy();
    assert!(matches!(
        suspended_policy.resolve(request(14.0)),
        Err(TextMeasurementError::Pending(_))
    ));
    admission
        .admit([(
            request(14.0),
            TextSize::new(21.0).expect("suspended answer"),
        )])
        .expect("suspended attempt admission");
    for step in 0..300 {
        let authored = 30.0 + f64::from(step) / 16.0;
        let active = sizing.begin_cohort();
        let active_policy = active.policy();
        assert!(matches!(
            active_policy.resolve(request(authored)),
            Err(TextMeasurementError::Pending(_))
        ));
        admission
            .admit([(
                request(authored),
                TextSize::new(authored + 5.0).expect("active answer"),
            )])
            .expect("independent attempt admission");
        assert_eq!(
            active_policy
                .resolve(request(authored))
                .expect("active answer delivered")
                .value(),
            authored + 5.0
        );
        drop(active);
    }
    assert!(matches!(
        sizing.resolve(request(30.0)),
        Err(TextMeasurementError::Pending(_))
    ));
    assert_eq!(
        suspended_policy
            .resolve(request(14.0))
            .expect("own suspended answer remains pinned")
            .value(),
        21.0
    );
    drop(suspended);
    admission.set_warm_capacity(0);
    assert!(matches!(
        suspended_policy.resolve(request(14.0)),
        Err(TextMeasurementError::Pending(_))
    ));
    admission.set_warm_capacity(2);
    for _ in 0..5000 {
        drop(sizing.begin_cohort());
    }
    let next = sizing.begin_cohort();
    let next_policy = next.policy();
    assert!(matches!(
        next_policy.resolve(request(20.25)),
        Err(TextMeasurementError::Pending(_))
    ));
    admission
        .admit([(
            request(20.25),
            TextSize::new(28.75).expect("recovery answer"),
        )])
        .expect("next attempt after repeated cancellation");
    assert_eq!(
        next_policy
            .resolve(request(20.25))
            .expect("next delivery")
            .value(),
        28.75
    );
}

pub(crate) fn captured_attempts_retain_nested_sources_across_frontiers() {
    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_painting::{TextMeasurement, TextMeasurementError, TextSizing};
    let request = |size| TextSizeRequest {
        size: TextSize::new(size).expect("authored size"),
        profile: TextScaleProfile::Body,
    };
    let make_painter = |size, policy| {
        TextPainter::new()
            .with_text(TextSpan::styled(
                "nested captured typography",
                TextStyle::new().with_font_size(size),
            ))
            .with_text_direction(TextDirection::Ltr)
            .with_text_sizing(policy)
    };
    let (root_policy, mut root_writer) = TextSizing::captured();
    let (nested_policy, mut nested_writer) = TextSizing::captured();
    root_writer.set_warm_capacity(0);
    nested_writer.set_warm_capacity(0);
    let root_attempt = root_policy.begin_cohort();
    let inherited = root_attempt.policy();
    let mut context = text_cx();
    let root = make_painter(10.0, None);
    assert!(matches!(
        TextMeasurement::new(&mut context, &inherited).dry_size(&root, 0.0, 400.0),
        Err(TextMeasurementError::Pending(_))
    ));
    root_writer
        .admit([(request(10.0), TextSize::new(15.0).expect("root answer"))])
        .expect("root frontier admission");
    TextMeasurement::new(&mut context, &inherited)
        .dry_size(&root, 0.0, 400.0)
        .expect("root source resolves in the retained attempt");

    let mut measured = Vec::new();
    for (authored, resolved) in [(12.25, 24.5), (17.75, 28.25)] {
        // A short nested attempt seeds an answer even on the old source-bound
        // implementation. The inherited attempt must take custody when this
        // override is actually measured; neither this seed nor live geometry
        // remains to mask missing cross-source retention at the final revisit.
        let seed = nested_policy.begin_cohort();
        let painter = make_painter(authored, Some(seed.policy()));
        assert!(matches!(
            TextMeasurement::new(&mut context, &inherited).dry_size(&painter, 0.0, 400.0),
            Err(TextMeasurementError::Pending(_))
        ));
        nested_writer
            .admit([(
                request(authored),
                TextSize::new(resolved).expect("nested answer"),
            )])
            .expect("nested frontier admission");
        let geometry = TextMeasurement::new(&mut context, &inherited)
            .dry_size(&painter, 0.0, 400.0)
            .expect("nested answer was actually measured");
        assert!(geometry.width > 0.0 && geometry.height > 0.0);
        assert!(
            !painter.has_layout(),
            "dry measurement owns no live layout lease"
        );
        drop(seed);
        measured.push((painter, geometry));
    }
    for (painter, expected) in &measured {
        let revisited = TextMeasurement::new(&mut context, &inherited)
            .dry_size(painter, 0.0, 400.0)
            .expect("root attempt retains every nested source across later frontiers");
        assert_eq!(revisited, *expected);
    }
    drop(root_attempt);
    assert!(matches!(
        nested_policy.resolve(request(12.25)),
        Err(TextMeasurementError::Pending(_))
    ));
}

pub(crate) fn fixed_and_linear_attempts_retain_multiple_nested_sources() {
    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_painting::{TextMeasurement, TextMeasurementError, TextSizing};
    let request = |size| TextSizeRequest {
        size: TextSize::new(size).expect("authored size"),
        profile: TextScaleProfile::Body,
    };
    for root in [
        TextSizing::fixed(),
        TextSizing::linear(1.25).expect("linear root"),
    ] {
        let attempt = root.begin_cohort();
        let inherited = attempt.policy();
        assert_eq!(
            root, inherited,
            "attempt membership is not numeric identity"
        );
        let (first, mut first_writer) = TextSizing::captured();
        let (second, mut second_writer) = TextSizing::captured();
        first_writer.set_warm_capacity(0);
        second_writer.set_warm_capacity(0);
        let mut context = text_cx();
        let mut measured = Vec::new();
        for (policy, writer, authored, resolved) in [
            (&first, &mut first_writer, 12.25, 24.5),
            (&second, &mut second_writer, 17.75, 28.25),
        ] {
            let painter = TextPainter::new()
                .with_text(TextSpan::styled(
                    "independent nested source",
                    TextStyle::new().with_font_size(authored),
                ))
                .with_text_direction(TextDirection::Ltr)
                .with_text_sizing(Some(policy.clone()));
            assert!(matches!(
                TextMeasurement::new(&mut context, &inherited).dry_size(&painter, 0.0, 400.0),
                Err(TextMeasurementError::Pending(_))
            ));
            writer
                .admit([(request(authored), TextSize::new(resolved).expect("answer"))])
                .expect("source-specific admission");
            let geometry = TextMeasurement::new(&mut context, &inherited)
                .dry_size(&painter, 0.0, 400.0)
                .expect("root attempt retains a nested frontier");
            assert!(!painter.has_layout(), "only copied geometry is retained");
            measured.push((painter, geometry));
        }
        for (painter, expected) in &measured {
            assert_eq!(
                TextMeasurement::new(&mut context, &inherited)
                    .dry_size(painter, 0.0, 400.0)
                    .expect("all nested sources survive later frontiers"),
                *expected
            );
        }
        // Root retention cannot replace either source's numeric authority.
        assert!(matches!(
            first.resolve(request(17.75)),
            Err(TextMeasurementError::Pending(_))
        ));
        drop(attempt);
        for (policy, authored) in [(&first, 12.25), (&second, 17.75)] {
            assert!(matches!(
                policy.resolve(request(authored)),
                Err(TextMeasurementError::Pending(_))
            ));
        }
    }
}

pub(crate) fn captured_debug_releases_infrastructure_before_formatter_reentry() {
    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_painting::{TextSizing, TextSizingAdmission};
    use std::fmt::Write as _;
    struct ReentrantWriter<'a> {
        admission: &'a mut TextSizingAdmission,
        request: TextSizeRequest,
        answer: TextSize,
        calls: usize,
    }
    impl std::fmt::Write for ReentrantWriter<'_> {
        fn write_str(&mut self, _: &str) -> std::fmt::Result {
            self.admission
                .admit([(self.request, self.answer)])
                .expect("formatter may reenter admission");
            self.calls += 1;
            Ok(())
        }
    }
    let request = TextSizeRequest {
        size: TextSize::new(14.0).expect("authored size"),
        profile: TextScaleProfile::Body,
    };
    let (policy, mut admission) = TextSizing::captured();
    let source = admission.source();
    let cohort = policy.begin_cohort();
    let mut output = ReentrantWriter {
        admission: &mut admission,
        request,
        answer: TextSize::new(21.0).expect("answer"),
        calls: 0,
    };
    write!(&mut output, "{source:?} {cohort:?} {policy:?}").expect("opaque metadata formatting");
    assert!(output.calls > 0);
    assert_eq!(
        policy
            .resolve(request)
            .expect("formatter's admitted answer")
            .value(),
        21.0
    );
}

pub(crate) fn captured_cache_hits_pin_the_current_attempt_before_font_invalidation() {
    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_painting::{TextMeasurement, TextMeasurementError, TextSizing};
    for (name, authored_scope) in [
        ("inherited attempt", false),
        ("same-capture authored setter", true),
    ] {
        let (policy, mut admission) = TextSizing::captured();
        admission.set_warm_capacity(0);
        let request = TextSizeRequest {
            size: TextSize::new(14.0).expect("authored size"),
            profile: TextScaleProfile::Body,
        };
        let first = policy.begin_cohort();
        let first_policy = first.policy();
        assert!(matches!(
            first_policy.resolve(request),
            Err(TextMeasurementError::Pending(_))
        ));
        admission
            .admit([(request, TextSize::new(21.0).expect("answer"))])
            .expect("first answer");
        let fonts = flui_painting::FontCollection::new();
        fonts
            .register_font(include_bytes!("../assets/fonts/Roboto-Regular.ttf"))
            .expect("fixture font");
        let mut context = flui_painting::TextContext::new(&fonts);
        let mut painter = TextPainter::new()
            .with_text(TextSpan::styled(
                "cached typography",
                TextStyle::new().with_font_family("Roboto"),
            ))
            .with_text_direction(TextDirection::Ltr)
            .with_text_sizing(Some(first_policy));
        painter
            .layout(&mut context, 0.0, 400.0)
            .expect("first committed geometry");
        let previous = painter.size();
        drop(first);
        let next = policy.begin_cohort();
        let next_policy = next.policy();
        let width = if authored_scope {
            painter.set_text_sizing(Some(next_policy.clone()));
            painter
                .max_intrinsic_width(&mut context)
                .expect("same-capture cached intrinsic")
        } else {
            TextMeasurement::new(&mut context, &next_policy)
                .max_intrinsic_width(&painter)
                .expect("inherited cached intrinsic")
        };
        assert!(width > 0.0);
        fonts
            .register_font(include_bytes!("../assets/fonts/probe-mono-600.ttf"))
            .expect("real font publication invalidates the geometry cache");
        let result = if authored_scope {
            painter.layout(&mut context, 0.0, 400.0)
        } else {
            TextMeasurement::new(&mut context, &next_policy).layout(&mut painter, 0.0, 400.0)
        };
        result.unwrap_or_else(|error| {
            panic!("{name}: a cache-hit answer remains pinned through font invalidation: {error}")
        });
        assert_eq!(painter.size(), previous);
        assert_eq!(
            policy
                .resolve(request)
                .expect("current attempt still retains answer")
                .value(),
            21.0
        );
    }
}

/// A text context over a fresh collection, lent to each measurement.
fn text_cx() -> flui_painting::TextContext {
    flui_painting::TextContext::new(&flui_painting::FontCollection::new())
}

// ============================================================================
// measure_text standalone function
// ============================================================================

// ============================================================================
// Full pipeline: measure -> layout -> paint -> display list
// ============================================================================

pub(crate) fn full_pipeline_with_styled_text() {
    let style = TextStyle::new()
        .with_font_size(20.0)
        .with_font_weight(FontWeight::BOLD);

    let span = TextSpan::new("Styled text").with_style(style);

    let mut painter = TextPainter::new()
        .with_text(span)
        .with_text_direction(TextDirection::Ltr);

    painter
        .layout(&mut text_cx(), 0.0, 400.0)
        .expect("valid fixture lays out");
    assert!(painter.width() > 0.0);

    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, Offset::ZERO);

    let dl = canvas.finish();
    assert!(!dl.is_empty());
}
