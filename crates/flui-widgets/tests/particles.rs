//! `ParticleEmitter`, `ParticleConfig` and `ParticleController`: configurations checked when
//! built, bursts and their capacity, the clock that stops and restarts, fixed-step catch-up
//! after a paused ticker, the numeric guard before the atlas, flash safety of the presets,
//! one emitter per controller, reduced motion, and semantics, read from the recorded atlas.

use std::f64::consts::FRAC_PI_2;
use std::time::Duration;

use crate::common::{LaidOut, lay_out_animated, tight};
use flui_animation::{MotionOverride, Vsync};
use flui_foundation::geometry::{Matrix4, Offset, Point};
use flui_painting::display_list::DrawOp;
use flui_painting::styling::Color;
use flui_widgets::motion::particles::{
    Coverage, EmitterDuration, EmitterSource, MAX_CAPACITY, ParticleConfig, ParticleConfigError,
    ParticleController, ParticleEmitter, ParticleField, Sprite,
};
use flui_widgets::{TickerMode, VsyncScope};

/// The emitter's box.
const SIDE: f64 = 200.0;
/// A built-in sprite's centre in the atlas.
const SPRITE_CENTRE: f64 = 16.0;
/// One frame at 60 Hz.
const FRAME: Duration = Duration::from_nanos(16_666_667);

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// Every particle drawn in one atlas call: its centre, and its color.
#[derive(Debug, Clone, PartialEq)]
struct Frame {
    atlases: usize,
    particles: Vec<(Point<f64>, Color, Matrix4)>,
}

fn frame(laid: &LaidOut) -> Frame {
    let mut atlases = 0;
    let mut particles = Vec::new();
    for command in laid.draw_ops() {
        if let DrawOp::Atlas {
            transforms, colors, ..
        } = command.op
        {
            atlases += 1;
            let colors = colors.expect("particles are tinted");
            for (transform, color) in transforms.into_iter().zip(colors) {
                let (x, y) = transform.transform_point(SPRITE_CENTRE, SPRITE_CENTRE);
                particles.push((Point::new(x, y), color, transform));
            }
        }
    }
    Frame { atlases, particles }
}

fn count(laid: &LaidOut) -> usize {
    frame(laid).particles.len()
}

/// A burst-only configuration: particles thrown up from the box's centre, falling back.
fn fountain(seed: u64) -> ParticleConfig {
    ParticleConfig::builder()
        .source(EmitterSource::point(Point::new(0.5, 0.5)))
        .rate(0.0)
        .lifetime(1.0, 1.5)
        .speed(40.0, 120.0)
        .direction(-FRAC_PI_2 - 0.5, -FRAC_PI_2 + 0.5)
        .size(4.0, 8.0)
        .spin(-3.0, 3.0)
        .gravity(Offset::new(0.0, 200.0))
        .drag(0.5)
        .seed(seed)
        .build()
        .expect("a valid fountain")
}

fn emitter(config: ParticleConfig, controller: &ParticleController) -> ParticleEmitter {
    ParticleEmitter::new(config).controller(controller.clone())
}

/// `view` mounted in a 200 × 200 box under a ticking scope, its first frame at time zero.
fn mount(view: impl flui_view::View) -> (LaidOut, Vsync) {
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), view),
        tight(SIDE, SIDE),
        vsync.clone(),
    );
    laid.pump_for(Duration::ZERO);
    (laid, vsync)
}

fn frames(laid: &mut LaidOut, n: usize) {
    for _ in 0..n {
        laid.pump_for(FRAME);
    }
}

/// Each refused configuration names its reason.
pub(crate) fn configurations_are_checked_when_built() {
    use ParticleConfigError as E;
    use ParticleField as F;
    let base = ParticleConfig::builder;
    let rows: Vec<(
        &str,
        Result<ParticleConfig, ParticleConfigError>,
        ParticleConfigError,
    )> = vec![
        (
            "NaN rate",
            base().rate(f64::NAN).build(),
            E::NonFinite { field: F::Rate },
        ),
        (
            "infinite speed",
            base().speed(0.0, f64::INFINITY).build(),
            E::NonFinite { field: F::Speed },
        ),
        (
            "a spread overflowing",
            base().spin(-f64::MAX, f64::MAX).build(),
            E::NonFinite { field: F::Spin },
        ),
        (
            "negative lifetime",
            base().lifetime(-1.0, 1.0).build(),
            E::NonPositiveLifetime,
        ),
        (
            "zero lifetime",
            base().lifetime(0.0, 1.0).build(),
            E::NonPositiveLifetime,
        ),
        (
            "inverted size",
            base().size(8.0, 2.0).build(),
            E::InvertedRange { field: F::Size },
        ),
        (
            "negative rate",
            base().rate(-1.0).build(),
            E::Negative { field: F::Rate },
        ),
        (
            "negative drag",
            base().drag(-0.1).build(),
            E::Negative { field: F::Drag },
        ),
        (
            "opacity above one",
            base().opacity(1.5, 0.0).build(),
            E::OutOfUnitRange { field: F::Opacity },
        ),
        (
            "capacity above the limit",
            base().capacity(MAX_CAPACITY + 1).build(),
            E::CapacityTooLarge {
                capacity: MAX_CAPACITY + 1,
                max: MAX_CAPACITY,
            },
        ),
        (
            "NaN source",
            base()
                .source(EmitterSource::point(Point::new(f64::NAN, 0.0)))
                .build(),
            E::NonFinite { field: F::Source },
        ),
        (
            "inverted ring",
            base()
                .source(EmitterSource::ring(Point::new(0.5, 0.5), 0.4, 0.2))
                .build(),
            E::InvertedRange { field: F::Source },
        ),
        (
            "infinite gravity",
            base().gravity(Offset::new(0.0, f64::INFINITY)).build(),
            E::NonFinite { field: F::Gravity },
        ),
        (
            "empty palette",
            base().palette(Vec::new()).build(),
            E::EmptyPalette,
        ),
    ];
    for (what, result, expected) in rows {
        assert_eq!(result.err(), Some(expected), "{what}");
    }
    assert!(
        base()
            .capacity(MAX_CAPACITY)
            .source(EmitterSource::circle(
                Point::new(0.5, 0.5),
                0.3,
                Coverage::Outline
            ))
            .duration(EmitterDuration::For(ms(500)))
            .sprite(Sprite::Star)
            .build()
            .is_ok(),
        "the limits themselves are accepted"
    );
    for preset in [
        ParticleConfig::confetti(),
        ParticleConfig::sparkle(),
        ParticleConfig::snow(),
    ] {
        assert!(preset.capacity() <= MAX_CAPACITY);
    }
}

/// A burst of 120 draws 120 sprites in one atlas call, and the same seed gives the same
/// frames.
pub(crate) fn a_seeded_burst_draws_one_atlas_the_same_each_run() {
    let run = || {
        let controller = ParticleController::new();
        let (mut laid, _) = mount(emitter(fountain(7), &controller));
        assert_eq!(controller.emit_burst(120), 120);
        let mut recorded = Vec::new();
        for _ in 0..10 {
            laid.pump_for(FRAME);
            recorded.push(frame(&laid));
        }
        recorded
    };
    let first = run();
    assert_eq!(first[0].atlases, 1, "one atlas call a frame");
    assert_eq!(first[0].particles.len(), 120);
    assert_ne!(first[0], first[9], "the particles move");
    assert_eq!(first, run(), "the same seed records the same frames");
}

/// The clock stops once the last particle dies, and a burst after that is drawn in the next
/// frame with the clock running again.
pub(crate) fn the_clock_stops_when_empty_and_a_burst_restarts_it() {
    let controller = ParticleController::new();
    let (mut laid, vsync) = mount(emitter(fountain(1), &controller));
    assert!(!vsync.has_running(), "a burst-only emitter waits");
    assert_eq!(controller.emit_burst(10), 10);
    frames(&mut laid, 2);
    assert!(vsync.has_running(), "a burst runs the clock");
    laid.pump_for(ms(1600));
    frames(&mut laid, 3);
    assert_eq!(count(&laid), 0, "every particle has died");
    assert!(!vsync.has_running(), "the clock stops");
    assert_eq!(controller.emit_burst(5), 5);
    laid.pump_for(FRAME);
    assert_eq!(count(&laid), 5, "the burst is drawn in the next frame");
    assert!(vsync.has_running());
}

/// A burst larger than the free capacity is cut to it; a full emitter emits nothing and keeps
/// its particles.
pub(crate) fn a_burst_is_cut_to_the_free_capacity() {
    let controller = ParticleController::new();
    let (mut laid, _) = mount(emitter(fountain(2), &controller));
    assert_eq!(controller.emit_burst(1000), 512, "the default capacity");
    assert_eq!(controller.emit_burst(10), 0, "full");
    laid.pump_for(FRAME);
    assert_eq!(count(&laid), 512, "live particles are not pushed out");

    let small = ParticleController::new();
    let config = fountain(2).to_builder().capacity(8).build().expect("valid");
    let (_laid, _) = mount(emitter(config, &small));
    assert_eq!(small.emit_burst(5), 5);
    assert_eq!(small.emit_burst(5), 3);
}

/// A burst under a disabled `TickerMode` is refused and not kept: turning the ticker back on
/// emits nothing.
pub(crate) fn a_muted_ticker_refuses_bursts() {
    let controller = ParticleController::new();
    let vsync = Vsync::new();
    let view = |enabled: bool| {
        VsyncScope::new(
            vsync.clone(),
            TickerMode::new(emitter(fountain(3), &controller)).enabled(enabled),
        )
    };
    let mut laid = lay_out_animated(view(false), tight(SIDE, SIDE), vsync.clone());
    laid.pump_for(Duration::ZERO);
    assert_eq!(controller.emit_burst(20), 0, "refused while muted");
    laid.pump_widget(view(true));
    frames(&mut laid, 3);
    assert_eq!(count(&laid), 0, "nothing was kept for later");
    assert_eq!(
        controller.emit_burst(20),
        20,
        "accepted once the ticker runs"
    );
}

/// A particle that would publish a non-finite or undrawable sprite is removed before the
/// atlas; ordinary particles beside it are drawn.
pub(crate) fn non_finite_particles_never_reach_the_atlas() {
    let controller = ParticleController::new();
    let (mut laid, _) = mount(emitter(fountain(4), &controller));
    assert_eq!(
        controller.emit_burst_at(Point::new(f64::MAX, 0.0), 5),
        0,
        "a sprite at f64::MAX is outside the atlas's range"
    );
    assert_eq!(controller.emit_burst_at(Point::new(f64::NAN, 0.0), 5), 0);
    assert_eq!(
        controller.emit_burst_at(Point::new(f64::INFINITY, 0.0), 5),
        0
    );
    assert_eq!(controller.emit_burst(3), 3, "ordinary particles");
    let wild = ParticleController::new();
    let config = ParticleConfig::builder()
        .rate(0.0)
        .speed(f64::MAX / 2.0, f64::MAX / 2.0)
        .rotation(f64::MAX / 2.0, f64::MAX / 2.0)
        .spin(f64::MAX / 2.0, f64::MAX / 2.0)
        .build()
        .expect("finite, if extreme");
    let (mut wild_laid, _) = mount(ParticleEmitter::new(config).controller(wild.clone()));
    let emitted = wild.emit_burst(5);
    frames(&mut wild_laid, 2);
    assert_eq!(count(&wild_laid), 0, "{emitted} wild particles all removed");
    frames(&mut laid, 2);
    let drawn = frame(&laid);
    assert_eq!(drawn.particles.len(), 3);
    for (centre, _, transform) in drawn.particles {
        assert!(centre.x.is_finite() && centre.y.is_finite());
        assert!(transform.m.iter().all(|value| value.is_finite()));
    }
}

/// After a pause of the ticker of at most a quarter second, live particles stand where they
/// would have without it; after a longer pause none are left, and a rate carries on without a
/// burst for the missed time.
pub(crate) fn a_paused_ticker_catches_up_in_fixed_steps() {
    let positions = |pause: Option<Duration>| {
        let controller = ParticleController::new();
        let vsync = Vsync::new();
        let view = |enabled: bool| {
            VsyncScope::new(
                vsync.clone(),
                TickerMode::new(emitter(fountain(5), &controller)).enabled(enabled),
            )
        };
        let mut laid = lay_out_animated(view(true), tight(SIDE, SIDE), vsync.clone());
        laid.pump_for(Duration::ZERO);
        controller.emit_burst(30);
        frames(&mut laid, 6);
        match pause {
            Some(pause) => {
                laid.pump_widget(view(false));
                laid.pump_for(pause);
                laid.pump_widget(view(true));
                laid.pump_for(Duration::ZERO);
            }
            None => laid.pump_for(Duration::from_millis(200)),
        }
        frames(&mut laid, 6);
        frame(&laid)
            .particles
            .into_iter()
            .map(|(centre, _, _)| centre)
            .collect::<Vec<_>>()
    };
    let straight = positions(None);
    let paused = positions(Some(ms(200)));
    assert_eq!(straight.len(), 30);
    assert_eq!(paused.len(), straight.len());
    for (a, b) in straight.iter().zip(&paused) {
        assert!(
            (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6,
            "a 0.2 s pause lands where no pause would: {a:?} and {b:?}"
        );
    }
    assert_eq!(positions(Some(ms(1000))), Vec::new(), "a long pause clears");

    let vsync = Vsync::new();
    let view = |enabled: bool| {
        VsyncScope::new(
            vsync.clone(),
            TickerMode::new(ParticleEmitter::new(ParticleConfig::snow())).enabled(enabled),
        )
    };
    let mut laid = lay_out_animated(view(true), tight(SIDE, SIDE), vsync.clone());
    laid.pump_for(Duration::ZERO);
    frames(&mut laid, 120);
    laid.pump_widget(view(false));
    laid.pump_for(ms(1000));
    laid.pump_widget(view(true));
    laid.pump_for(Duration::ZERO);
    frames(&mut laid, 1);
    let after = count(&laid);
    assert!(
        after <= 1,
        "snow does not burst for the paused second: {after}"
    );
    frames(&mut laid, 60);
    let resumed = count(&laid);
    assert!(
        (12..=24).contains(&resumed),
        "snow carries on at its rate of 18 a second: {resumed}"
    );
}

/// The mean brightness of a preset's box never changes by more than 10 % in more than three
/// opposing pairs within any second (WCAG 2.3.1).
pub(crate) fn presets_never_flash() {
    for (name, config, burst) in [
        ("confetti", ParticleConfig::confetti(), true),
        ("sparkle", ParticleConfig::sparkle(), false),
        ("snow", ParticleConfig::snow(), false),
    ] {
        let controller = ParticleController::new();
        let (mut laid, _) = mount(emitter(config, &controller));
        let mut brightness = Vec::new();
        for i in 0..240 {
            if burst && i % 60 == 0 {
                controller.emit_burst_at(Point::new(SIDE / 2.0, SIDE / 2.0), 120);
            }
            laid.pump_for(FRAME);
            let lit: f64 = frame(&laid)
                .particles
                .iter()
                .map(|(_, color, transform)| {
                    let area = (transform.m[0] * transform.m[5] - transform.m[1] * transform.m[4])
                        .abs()
                        * 32.0
                        * 32.0;
                    f64::from(color.luminance()) * f64::from(color.opacity()) * area
                })
                .sum();
            brightness.push((lit / (SIDE * SIDE)).min(1.0));
        }
        // Each change of more than 0.1 from the last extreme in the opposite direction.
        let mut flashes = Vec::new();
        let (mut extreme, mut rising) = (brightness[0], None::<bool>);
        for (i, &value) in brightness.iter().enumerate() {
            let delta = value - extreme;
            if delta.abs() > 0.1 {
                let up = delta > 0.0;
                if rising.is_some_and(|was| was != up) {
                    flashes.push(i);
                }
                rising = Some(up);
                extreme = value;
            } else if rising.is_some_and(|up| (up && value > extreme) || (!up && value < extreme)) {
                extreme = value;
            }
        }
        for window in flashes.windows(7) {
            assert!(
                window[6] - window[0] >= 60,
                "{name} flashes more than three times a second: {flashes:?}"
            );
        }
    }
}

/// A controller drives one mounted emitter: a second one with it runs without it, and once
/// the first unmounts the next emitter takes it. Without an emitter a burst emits 0.
pub(crate) fn a_controller_drives_one_emitter() {
    let controller = ParticleController::new();
    assert_eq!(controller.emit_burst(3), 0, "no emitter yet");
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            flui_widgets::Row::new((
                flui_widgets::SizedBox::new(100.0, 100.0).child(emitter(fountain(6), &controller)),
                flui_widgets::SizedBox::new(100.0, 100.0).child(emitter(fountain(6), &controller)),
            )),
        ),
        tight(SIDE, 100.0),
        vsync.clone(),
    );
    laid.pump_for(Duration::ZERO);
    assert!(controller.is_attached());
    assert_eq!(controller.emit_burst(4), 4);
    laid.pump_for(FRAME);
    assert_eq!(count(&laid), 4, "one emitter took the burst");
    let centres = frame(&laid).particles;
    assert!(
        centres.iter().all(|(centre, _, _)| centre.x < 100.0),
        "the first emitter keeps the controller: {centres:?}"
    );

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        flui_widgets::SizedBox::shrink(),
    ));
    assert!(!controller.is_attached(), "free after unmount");
    assert_eq!(controller.emit_burst(4), 0);
    laid.pump_widget(VsyncScope::new(
        vsync,
        flui_widgets::SizedBox::new(100.0, 100.0).child(emitter(fountain(6), &controller)),
    ));
    laid.pump_for(Duration::ZERO);
    assert_eq!(controller.emit_burst(4), 4, "the next emitter takes it");
}

/// Under reduced motion nothing is emitted, bursts are refused, and live particles are gone
/// in the next frame.
pub(crate) fn reduced_motion_emits_nothing() {
    let controller = ParticleController::new();
    let (mut laid, vsync) = mount(emitter(fountain(8), &controller));
    assert_eq!(controller.emit_burst(10), 10);
    laid.pump_for(FRAME);
    assert_eq!(count(&laid), 10);
    laid.set_motion_override(MotionOverride::Reduce);
    laid.pump_for(FRAME);
    assert_eq!(count(&laid), 0, "live particles are removed");
    assert_eq!(controller.emit_burst(10), 0, "bursts are refused");
    frames(&mut laid, 2);
    assert!(!vsync.has_running(), "nothing ticks");

    let (mut snow, _) = mount(ParticleEmitter::new(ParticleConfig::snow()));
    snow.set_motion_override(MotionOverride::Reduce);
    snow.pump_for(Duration::ZERO);
    frames(&mut snow, 60);
    assert_eq!(count(&snow), 0, "a rate emits nothing");
}

/// Particles are left out of semantics: the child keeps its own node, and a burst adds none.
pub(crate) fn particles_are_left_out_of_semantics() {
    let controller = ParticleController::new();
    let (mut laid, _) =
        mount(emitter(fountain(9), &controller).child(flui_widgets::Text::new("Done")));
    laid.enable_semantics();
    laid.tick();
    let before = laid.a11y_tree().expect("semantics enabled").len();
    assert_eq!(controller.emit_burst(10), 10);
    laid.pump_for(FRAME);
    let tree = laid.a11y_tree().expect("semantics enabled");
    assert!(
        tree.find_by_label("Done").is_ok(),
        "the child keeps its semantics"
    );
    assert_eq!(tree.len(), before, "the particles add no nodes");
}
