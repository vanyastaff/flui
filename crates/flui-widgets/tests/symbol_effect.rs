//! `SymbolEffect` and `PathEffect`: each motion at its peak, read from the composited scale,
//! turn and opacity; one-shot plays per change and per repeat; a repeating effect finishing
//! its cycle; the replace cross-fade and its semantics; transitions on first application;
//! reduced motion; speed; and paths drawn on and off.

use std::num::NonZeroU32;
use std::time::Duration;

use flui_animation::{ConstantAnimation, MotionOverride, Vsync};
use flui_foundation::geometry::Matrix4;
use flui_painting::display_list::DrawOp;
use flui_view::View;
use flui_widgets::{
    AnimatedIcon, AnimatedIconData, Icon, IconCommand, IconData, IconPath, PathEffect, Speed,
    SymbolEffect, TickerMode, VsyncScope,
};

use crate::common::{LaidOut, lay_out_animated, loose};

/// One straight stroke 20 units long, still across its two frames.
const LINE: AnimatedIconData = AnimatedIconData::from_static(
    24.0,
    &[0.0, 1.0],
    &[],
    &[IconPath::new(
        &[1.0, 1.0],
        &[
            IconCommand::move_to(&[[2.0, 12.0], [2.0, 12.0]]),
            IconCommand::line_to(&[[22.0, 12.0], [22.0, 12.0]]),
        ],
    )],
    false,
);

/// A tree under a frame clock that can be shown again with a new configuration.
struct Stage {
    laid: LaidOut,
    vsync: Vsync,
}

impl Stage {
    fn mount(view: impl View) -> Self {
        Self::mount_with(view, None)
    }

    fn mount_with(view: impl View, motion: Option<MotionOverride>) -> Self {
        let vsync = Vsync::new();
        let mut laid = lay_out_animated(
            VsyncScope::new(vsync.clone(), view),
            loose(200.0),
            vsync.clone(),
        );
        if let Some(motion) = motion {
            laid.set_motion_override(motion);
        }
        laid.pump_for(Duration::ZERO);
        Self { laid, vsync }
    }

    fn show(&mut self, view: impl View) {
        self.laid
            .pump_widget(VsyncScope::new(self.vsync.clone(), view));
        self.laid.pump_for(Duration::ZERO);
    }

    fn advance(&mut self, millis: u64) {
        self.laid.pump_for(Duration::from_millis(millis));
    }

    /// The composited transform of the icon, or the identity without one.
    fn matrix(&self) -> Matrix4 {
        self.laid
            .transform_layer_matrices()
            .into_iter()
            .fold(Matrix4::IDENTITY, |outer, inner| outer * inner)
    }

    /// How much the icon is scaled.
    fn scale(&self) -> f64 {
        let (x0, y0) = self.matrix().transform_point(0.0, 0.0);
        let (x1, y1) = self.matrix().transform_point(1.0, 0.0);
        (x1 - x0).hypot(y1 - y0)
    }

    /// How far the icon is turned, in turns.
    fn turns(&self) -> f64 {
        let (x0, y0) = self.matrix().transform_point(0.0, 0.0);
        let (x1, y1) = self.matrix().transform_point(1.0, 0.0);
        (y1 - y0).atan2(x1 - x0) / std::f64::consts::TAU
    }

    /// Every opacity in the tree, outermost first.
    fn opacities(&self) -> Vec<f64> {
        self.laid
            .find_all_by_render_type("RenderAnimatedOpacity")
            .into_iter()
            .map(|id| self.laid.opacity(id))
            .collect()
    }

    /// The icon's opacity: the product of every opacity above it.
    fn opacity(&self) -> f64 {
        self.opacities().into_iter().product()
    }

    /// The drawn length of the vector icon's one path, or 0 when nothing is drawn.
    fn drawn_length(&self) -> f64 {
        self.laid
            .draw_ops()
            .into_iter()
            .filter_map(|command| match command.op {
                DrawOp::Path { path, .. } => Some(path.length()),
                _ => None,
            })
            .sum()
    }
}

fn glyph(code: u32) -> Icon {
    Icon::new(IconData::new(code)).size(24.0)
}

fn close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 0.02,
        "{what}: expected {expected}, got {actual}"
    );
}

/// Each motion at its peak: a bounce's scale a third of the way, a wiggle's first turn, a
/// steady rotation a quarter turn in, a pulse's lowest opacity and a breath's largest scale
/// half way, and an appearing icon half grown; each at rest at the end of its cycle.
pub(crate) fn each_effect_peaks_where_its_timing_says() {
    let mut bounce = Stage::mount(glyph(1).symbol_effect(SymbolEffect::bounce().on_change(0)));
    bounce.show(glyph(1).symbol_effect(SymbolEffect::bounce().on_change(1)));
    bounce.advance(150);
    close(bounce.scale(), 1.2, "a bounce at its peak");
    bounce.advance(400);
    close(bounce.scale(), 1.0, "a bounce at rest");

    let mut wiggle = Stage::mount(glyph(1).symbol_effect(SymbolEffect::wiggle().on_change(0)));
    wiggle.show(glyph(1).symbol_effect(SymbolEffect::wiggle().on_change(1)));
    wiggle.advance(125);
    close(wiggle.turns(), 0.03, "a wiggle's first turn");
    wiggle.advance(400);
    close(wiggle.turns(), 0.0, "a wiggle at rest");

    let rotate = |active| glyph(1).symbol_effect(SymbolEffect::rotate().active(active));
    let mut spin = Stage::mount(rotate(true));
    spin.advance(250);
    close(
        spin.turns(),
        0.25,
        "a steady rotation a quarter of a second in",
    );

    let mut pulse = Stage::mount(glyph(1).symbol_effect(SymbolEffect::pulse().active(true)));
    pulse.advance(500);
    close(pulse.opacity(), 0.35, "a pulse at its lowest");
    pulse.advance(500);
    close(pulse.opacity(), 1.0, "a pulse back up");

    let mut breathe = Stage::mount(glyph(1).symbol_effect(SymbolEffect::breathe().active(true)));
    breathe.advance(1000);
    close(breathe.scale(), 1.1, "a breath at its largest");

    let mut appear = Stage::mount(glyph(1).symbol_effect(SymbolEffect::appear()));
    close(appear.opacity(), 0.0, "an appearing icon starts unseen");
    appear.advance(300);
    close(appear.opacity(), 1.0, "and ends seen");
    close(appear.scale(), 1.0, "at its own size");
}

/// A one-shot effect does not play on the first build or on a rebuild with the same value;
/// a new value plays it once, and `repeat(3)` three times.
pub(crate) fn a_one_shot_effect_plays_per_change_and_repeat() {
    let bounce = |value| glyph(1).symbol_effect(SymbolEffect::bounce().on_change(value));
    let mut stage = Stage::mount(bounce(7));
    stage.advance(150);
    close(stage.scale(), 1.0, "the first build is still");
    stage.show(bounce(7));
    stage.advance(150);
    close(stage.scale(), 1.0, "the same value is still");

    let thrice = |value| {
        glyph(1).symbol_effect(
            SymbolEffect::bounce()
                .on_change(value)
                .repeat(NonZeroU32::new(3).expect("three")),
        )
    };
    let mut stage = Stage::mount(thrice(0));
    stage.show(thrice(1));
    stage.advance(150);
    close(stage.scale(), 1.2, "the first bounce");
    stage.advance(500);
    close(stage.scale(), 1.2, "the second bounce at its peak");
    stage.advance(500);
    close(stage.scale(), 1.2, "the third bounce at its peak");
    stage.advance(500);
    close(stage.scale(), 1.0, "at rest after three");
    stage.advance(150);
    close(stage.scale(), 1.0, "and no fourth");
}

/// A repeating effect turned off finishes the cycle in progress and then stays at rest.
pub(crate) fn a_repeating_effect_finishes_its_cycle_when_turned_off() {
    let pulse = |active| glyph(1).symbol_effect(SymbolEffect::pulse().active(active));
    let mut stage = Stage::mount(pulse(true));
    stage.advance(1250);
    let dimmed = stage.opacity();
    assert!(dimmed < 0.9, "a quarter into the second cycle: {dimmed}");
    stage.show(pulse(false));
    stage.advance(250);
    close(
        stage.opacity(),
        0.35,
        "the cycle runs on to its lowest point",
    );
    stage.advance(600);
    close(stage.opacity(), 1.0, "and ends at rest");
    stage.advance(500);
    close(stage.opacity(), 1.0, "where it stays");
}

/// A replace fades the old glyph out under the new one; only the new label is in the
/// semantics tree, from the first frame of the fade on, and the old glyph is gone at its end.
pub(crate) fn a_replace_cross_fades_and_only_the_new_label_is_heard() {
    let icon = |code, label: &str| {
        glyph(code)
            .semantic_label(label)
            .symbol_effect(SymbolEffect::replace())
    };
    let mut stage = Stage::mount(icon(1, "Play"));
    stage.laid.enable_semantics();
    assert_eq!(stage.opacities(), [1.0], "one glyph at rest");
    stage.show(icon(2, "Pause"));
    stage.advance(125);
    let fading = stage.opacities();
    assert_eq!(fading.len(), 2, "both glyphs mid-fade: {fading:?}");
    close(fading[0], 0.5, "the new glyph half in");
    close(fading[1], 0.5, "the old glyph half out");
    let tree = stage.laid.a11y_tree().expect("semantics enabled");
    assert!(tree.find_by_label("Pause").is_ok(), "the new label");
    assert!(
        tree.find_by_label("Play").is_err(),
        "the old label is not heard"
    );

    stage.advance(200);
    assert_eq!(stage.opacities(), [1.0], "the old glyph is gone");

    stage.show(icon(2, "Pause"));
    stage.advance(100);
    assert_eq!(stage.opacities(), [1.0], "the same glyph does not fade");
}

/// A transition plays when first applied: on mount, unless `animate_on_appear` is off, and
/// when it takes another effect's place.
pub(crate) fn transitions_play_when_first_applied() {
    let mut stage =
        Stage::mount(glyph(1).symbol_effect(SymbolEffect::appear().animate_on_appear(false)));
    close(stage.opacity(), 1.0, "mounted at its end state");

    stage.show(glyph(1).symbol_effect(SymbolEffect::disappear()));
    stage.advance(125);
    let leaving = stage.opacity();
    assert!(
        leaving > 0.05 && leaving < 0.95,
        "a disappear mid-way: {leaving}"
    );
    stage.advance(200);
    close(stage.opacity(), 0.0, "and hidden");
    stage.advance(500);
    close(stage.opacity(), 0.0, "and it stays hidden");

    stage.show(glyph(1).symbol_effect(SymbolEffect::appear()));
    stage.advance(125);
    let coming = stage.opacity();
    assert!(
        coming > 0.05 && coming < 0.95,
        "an appear mid-way: {coming}"
    );
    stage.advance(200);
    close(stage.opacity(), 1.0, "and shown");
}

/// Under reduced motion one-shot and repeating effects leave the icon still, and
/// transitions land on their end state on the next frame.
pub(crate) fn reduced_motion_keeps_the_icon_still_or_at_its_end() {
    let reduced = |view| Stage::mount_with(view, Some(MotionOverride::Reduce));

    let bounce = |value| glyph(1).symbol_effect(SymbolEffect::bounce().on_change(value));
    let mut stage = reduced(bounce(0));
    stage.show(bounce(1));
    stage.advance(150);
    close(stage.scale(), 1.0, "a bounce is still");

    let mut stage = reduced(glyph(1).symbol_effect(SymbolEffect::pulse().active(true)));
    stage.advance(500);
    close(stage.opacity(), 1.0, "a pulse is still");

    let mut stage = reduced(glyph(1).symbol_effect(SymbolEffect::disappear()));
    stage.advance(16);
    close(stage.opacity(), 0.0, "a disappear lands hidden");

    let icon = |code| glyph(code).symbol_effect(SymbolEffect::replace());
    let mut stage = reduced(icon(1));
    stage.show(icon(2));
    stage.advance(16);
    assert_eq!(stage.opacities(), [1.0], "a replace lands on the new glyph");

    let mut stage = Stage::mount_with(
        AnimatedIcon::new(LINE, ConstantAnimation::new(0.0)).path_effect(PathEffect::draw_on()),
        Some(MotionOverride::Reduce),
    );
    stage.advance(16);
    close(stage.drawn_length(), 20.0, "a draw on lands whole");
}

/// A speed is a finite positive multiplier, and `speed(2)` plays an effect in half the time.
pub(crate) fn a_speed_is_positive_and_finite_and_scales_the_timing() {
    for refused in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(Speed::try_new(refused), None, "{refused} is refused");
    }
    let double = Speed::try_new(2.0).expect("a positive speed");
    assert_eq!(double.get(), 2.0);
    assert_eq!(Speed::default(), Speed::NORMAL);

    let bounce =
        |value| glyph(1).symbol_effect(SymbolEffect::bounce().on_change(value).speed(double));
    let mut stage = Stage::mount(bounce(0));
    stage.show(bounce(1));
    stage.advance(75);
    close(stage.scale(), 1.2, "the peak in half the time");
    stage.advance(200);
    close(stage.scale(), 1.0, "at rest in half the time");
}

/// A draw on traces the paths from nothing to whole and a draw off back to nothing; under
/// a muted ticker each lands at its end at once.
pub(crate) fn a_path_effect_draws_the_paths_on_and_off() {
    let line = |effect| {
        AnimatedIcon::new(LINE, ConstantAnimation::new(0.0))
            .size(24.0)
            .path_effect(effect)
    };
    let mut stage = Stage::mount(line(PathEffect::draw_on()));
    close(stage.drawn_length(), 0.0, "nothing drawn at the start");
    stage.advance(250);
    let half = stage.drawn_length();
    assert!(half > 2.0 && half < 18.0, "part of the line: {half}");
    stage.advance(300);
    close(stage.drawn_length(), 20.0, "the whole line");

    stage.show(line(PathEffect::draw_off()));
    stage.advance(250);
    let part = stage.drawn_length();
    assert!(part > 2.0 && part < 18.0, "part undrawn: {part}");
    stage.advance(300);
    close(stage.drawn_length(), 0.0, "nothing left");

    let muted = Stage::mount(TickerMode::new(line(PathEffect::draw_on())).enabled(false));
    close(
        muted.drawn_length(),
        20.0,
        "drawn whole under a muted ticker",
    );
}
