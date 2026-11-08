//! The tracing identities are a public wire contract, not producer-local strings.

use flui_foundation::diagnostics::{FrameSpan, PERIOD_US, PRESENTATION, RUNTIME};

const WIRE: [(FrameSpan, &str, &str); 14] = [
    (FrameSpan::Frame, "flui.frame", "frame"),
    (FrameSpan::Presentation, "flui.frame", "presentation"),
    (FrameSpan::Build, "flui.frame", "build"),
    (FrameSpan::Layout, "flui.frame", "layout"),
    (FrameSpan::LayoutRoot, "flui.frame", "layout_root"),
    (FrameSpan::Compositing, "flui.frame", "compositing"),
    (FrameSpan::Paint, "flui.frame", "paint"),
    (FrameSpan::Semantics, "flui.frame", "semantics"),
    (FrameSpan::Commit, "flui.frame", "commit"),
    (FrameSpan::Submit, "flui.frame", "submit"),
    (FrameSpan::AnimationTick, "flui.animation", "animation_tick"),
    (
        FrameSpan::AnimationSampling,
        "flui.animation",
        "animation_sampling",
    ),
    (
        FrameSpan::AnimationListeners,
        "flui.animation",
        "animation_listeners",
    ),
    (
        FrameSpan::AnimationController,
        "flui.animation.diagnostics",
        "animation_controller",
    ),
];

const _: () = {
    let Some(FrameSpan::Frame) = FrameSpan::of("flui.frame", "frame") else {
        panic!("the frame identity must classify in a const context");
    };
    let None = FrameSpan::of("example.application", "paint") else {
        panic!("an application span must not classify as framework telemetry");
    };
};

#[test]
fn frame_span_identities_are_the_exact_unique_wire_contract() {
    let all: Vec<_> = FrameSpan::ALL
        .into_iter()
        .map(|span| (span, span.target(), span.name()))
        .collect();
    assert_eq!(all, WIRE, "the complete typed wire table changed");

    for (index, &(span, target, name)) in WIRE.iter().enumerate() {
        assert_eq!(span.target(), target);
        assert_eq!(span.name(), name);
        assert_eq!(FrameSpan::of(target, name), Some(span));
        assert!(
            WIRE[..index]
                .iter()
                .all(|&(_, other_target, other_name)| (other_target, other_name) != (target, name)),
            "duplicate tracing identity ({target}, {name})"
        );
    }
}

#[test]
fn frame_span_classification_requires_the_exact_target_and_name() {
    for (target, name) in [
        ("example.application", "paint"),
        ("flui.frame", "application_paint"),
        ("flui.animation", "paint"),
        ("flui.frame", "animation_tick"),
        ("flui.animation.diagnostics", "animation_tick"),
        ("flui.animation", "animation_controller"),
    ] {
        assert_eq!(
            FrameSpan::of(target, name),
            None,
            "accepted ({target}, {name})"
        );
    }
}

#[test]
fn shared_frame_fields_keep_their_wire_names() {
    assert_eq!(RUNTIME, "runtime");
    assert_eq!(PRESENTATION, "presentation");
    assert_eq!(PERIOD_US, "period_us");
}
