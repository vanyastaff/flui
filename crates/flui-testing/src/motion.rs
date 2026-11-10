//! A motion gate: drive a mounted tree on the virtual clock, record what the window shows each
//! frame, and list the motion a person would notice (ADR-0321).
//!
//! A [`MotionRun`] samples [`PipelineOwner::presented_nodes`] after every frame it drives: each
//! render node's box in window-logical coordinates, the part of it its ancestors' clips leave and
//! the opacity it is drawn at. [`check`] reads the [`Recording`] against the [`Rule`]s and charges
//! each finding to the outermost node that shows it ([`charge_to_outermost`]). [`gate`] compares
//! the findings with an allowlist whose every row names its reason; a row that never matches is a
//! failure too, so the list only describes motion that still happens.
//!
//! ```
//! use std::time::Duration;
//!
//! use flui_animation::Vsync;
//! use flui_testing::motion::{Allow, MotionRun, assert_smooth};
//! use flui_testing::widgets::{lay_out_animated, tight};
//! use flui_view::environment::VsyncScope;
//! use flui_widgets::{AnimatedOpacity, MotionBuilder, SizedBox};
//!
//! let vsync = Vsync::new();
//! let tree = |opacity| {
//!     VsyncScope::new(
//!         vsync.clone(),
//!         AnimatedOpacity::new(opacity, SizedBox::new(40.0, 40.0))
//!             .duration(Duration::from_millis(200)),
//!     )
//! };
//! let mut laid = lay_out_animated(tree(1.0), tight(100.0, 100.0), vsync.clone());
//! let mut run = MotionRun::new(&mut laid);
//! run.act(|laid| laid.pump_widget(tree(0.0)));
//! run.play(Duration::from_secs(1), Duration::from_millis(16));
//! assert_smooth(&run.finish(), &[] as &[Allow]);
//! ```
//!
//! [`PipelineOwner::presented_nodes`]: flui_rendering::pipeline::PipelineOwner::presented_nodes

mod rules;

use std::fmt;
use std::time::Duration;

use flui_foundation::RenderId;
use flui_foundation::geometry::Rect;
use flui_rendering::pipeline::PresentedNode;
use rustc_hash::FxHashMap;

use crate::widgets::LaidOut;

pub use rules::{charge_to_outermost, check, check_all};

/// The thresholds every rule reads, in one place (ADR-0321 §3). Distances are window-logical
/// points, opacities `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct MotionConfig {
    /// `jump`: an edge that moves at least this far in one frame between frames where it holds
    /// still.
    pub jump: f64,
    /// How little an edge may move in a frame and still count as holding still.
    pub still: f64,
    /// `pop-in` and `pop-out`: a node that mounts or unmounts drawn at least this opaque.
    pub pop_opacity: f64,
    /// `fade-snap`: a drawn node whose opacity steps by at least this in one frame.
    pub fade_snap: f64,
    /// `flicker`: an opacity dip at least this deep that comes back within
    /// [`Self::flicker_window`].
    pub flicker_dip: f64,
    /// `flicker`: how soon a dip must come back to count.
    pub flicker_window: Duration,
    /// `overshoot`: how far past its resting place an edge may travel.
    pub overshoot: f64,
    /// `never-settles`: the stretch at the end of the run, after the last input, in which
    /// nothing may still move.
    pub settle_window: Duration,
    /// `never-settles`: an opacity change in that stretch at least this large.
    pub settle_opacity: f64,
    /// `wakes-after-end`: how long after the last visible change a frame may still be asked for.
    pub wake_after_end: Duration,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            jump: 2.0,
            still: 0.5,
            pop_opacity: 0.5,
            fade_snap: 0.5,
            flicker_dip: 0.3,
            flicker_window: Duration::from_millis(300),
            overshoot: 1.0,
            settle_window: Duration::from_millis(500),
            settle_opacity: 0.05,
            wake_after_end: Duration::from_secs(1),
        }
    }
}

/// What a person would notice (ADR-0321 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Rule {
    /// A node mounts drawn opaque, at its final place and size, with nothing changing on its next
    /// frame: it appeared instead of entering.
    PopIn,
    /// A node unmounts while drawn opaque: it vanished instead of leaving.
    PopOut,
    /// An edge moves [`MotionConfig::jump`] or more in one frame between frames where it holds
    /// still.
    Jump,
    /// A drawn node's opacity steps by [`MotionConfig::fade_snap`] or more in one frame.
    FadeSnap,
    /// Opacity falls from a peak and climbs back within [`MotionConfig::flicker_window`], in one
    /// step or many; a frame the node is left out of or drawn wholly clipped counts as opacity 0.
    Flicker,
    /// An edge travels past where it comes to rest by more than [`MotionConfig::overshoot`].
    Overshoot,
    /// Something still moves in the last [`MotionConfig::settle_window`] of the run.
    NeverSettles,
    /// The window still asks for frames [`MotionConfig::wake_after_end`] after the last visible
    /// change.
    WakesAfterEnd,
}

impl Rule {
    /// Every rule, in report order.
    pub const ALL: [Self; 8] = [
        Self::PopIn,
        Self::PopOut,
        Self::Jump,
        Self::FadeSnap,
        Self::Flicker,
        Self::Overshoot,
        Self::NeverSettles,
        Self::WakesAfterEnd,
    ];

    /// The rule's name in a report and an allowlist.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PopIn => "pop-in",
            Self::PopOut => "pop-out",
            Self::Jump => "jump",
            Self::FadeSnap => "fade-snap",
            Self::Flicker => "flicker",
            Self::Overshoot => "overshoot",
            Self::NeverSettles => "never-settles",
            Self::WakesAfterEnd => "wakes-after-end",
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One node in one recorded frame.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Sample {
    /// The node. A remounted node is a new id.
    pub id: RenderId,
    /// The node's parent in this frame.
    pub parent: Option<RenderId>,
    /// The node's place in the tree: render type names from the root, each with its child slot
    /// when that is not `0` (`RenderView/RenderFlex/RenderPadding[1]`). It is the same in every
    /// run of one tree, unlike the id, so an [`Allow`] row matches it.
    pub locator: String,
    /// The node's box in the window, window-logical points.
    pub rect: Rect,
    /// The part of [`Self::rect`] the clips leave, `None` when nothing is left.
    pub visible: Option<Rect>,
    /// The opacity the node is drawn at.
    pub opacity: f64,
}

impl Sample {
    /// Whether a person can see any of it.
    #[must_use]
    pub fn is_shown(&self) -> bool {
        self.visible.is_some() && self.opacity > 1e-3
    }

    fn edges(&self) -> [f64; 4] {
        [
            self.rect.left(),
            self.rect.top(),
            self.rect.right(),
            self.rect.bottom(),
        ]
    }

    fn visible_edges(&self) -> Option<[f64; 4]> {
        self.visible
            .map(|kept| [kept.left(), kept.top(), kept.right(), kept.bottom()])
    }
}

/// One frame a [`MotionRun`] drove.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Frame {
    /// Virtual time since the run started, from the harness's clock.
    pub time: Duration,
    /// Whether [`MotionRun::act`] or [`MotionRun::direct`] produced this frame.
    pub input: bool,
    /// Whether the frame follows a hand directly ([`MotionRun::direct`]): `jump` does not apply.
    pub direct: bool,
    /// Whether the host would run another frame at once after this one.
    pub wants_frame: bool,
    /// Every node the frame drew, in paint order.
    pub samples: Vec<Sample>,
}

/// The frames of one run, in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recording {
    frames: Vec<Frame>,
}

impl Recording {
    /// The frames, the settled start first.
    #[must_use]
    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }
}

/// Drives a [`LaidOut`] frame by frame and records each frame (ADR-0321 §1).
pub struct MotionRun<'a> {
    laid: &'a mut LaidOut,
    start: Duration,
    frames: Vec<Frame>,
}

impl fmt::Debug for MotionRun<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MotionRun")
            .field("frames", &self.frames.len())
            .finish_non_exhaustive()
    }
}

impl<'a> MotionRun<'a> {
    /// Starts a run on `laid` and records the frame it shows now as frame 0, the settled start.
    pub fn new(laid: &'a mut LaidOut) -> Self {
        let start = laid.elapsed();
        let mut run = Self {
            laid,
            start,
            frames: Vec::new(),
        };
        run.record(false, false);
        run
    }

    /// Advances `dt` of virtual time, drives a frame and records it.
    pub fn frame(&mut self, dt: Duration) {
        self.laid.pump_for(dt);
        self.record(false, false);
    }

    /// Drives frames `step` apart until `duration` has passed. A zero `step` drives none.
    pub fn play(&mut self, duration: Duration, step: Duration) {
        if step.is_zero() {
            return;
        }
        let mut played = Duration::ZERO;
        while played < duration {
            self.frame(step);
            played = played.saturating_add(step);
        }
    }

    /// Runs `input` (a tap, a `pump_widget`, a key), drives the frame it asks for and records
    /// that frame, marked as input. Dispatched input only schedules a rebuild; the frame is
    /// what shows its effect.
    pub fn act(&mut self, input: impl FnOnce(&mut LaidOut)) {
        input(self.laid);
        self.laid.pump_for(Duration::ZERO);
        self.record(true, false);
    }

    /// As [`Self::act`], for a frame that follows a hand directly (a drag step): content that
    /// tracks the pointer moves as far as the pointer did, so `jump` does not apply to it.
    pub fn direct(&mut self, input: impl FnOnce(&mut LaidOut)) {
        input(self.laid);
        self.laid.pump_for(Duration::ZERO);
        self.record(true, true);
    }

    /// The harness, for a read between frames.
    #[must_use]
    pub fn laid_out(&self) -> &LaidOut {
        self.laid
    }

    /// Ends the run.
    #[must_use]
    pub fn finish(self) -> Recording {
        Recording {
            frames: self.frames,
        }
    }

    fn record(&mut self, input: bool, direct: bool) {
        let samples = samples_of(self.laid.presented_nodes());
        self.frames.push(Frame {
            time: self.laid.elapsed().saturating_sub(self.start),
            input,
            direct,
            wants_frame: self.laid.wants_frame(),
            samples,
        });
    }
}

fn samples_of(nodes: Vec<PresentedNode>) -> Vec<Sample> {
    let mut locators: FxHashMap<RenderId, String> = FxHashMap::default();
    let mut samples = Vec::with_capacity(nodes.len());
    for node in nodes {
        let segment = if node.slot == 0 {
            node.name.to_owned()
        } else {
            format!("{}[{}]", node.name, node.slot)
        };
        // Paint order puts a parent before its children, so its locator is known.
        let locator = match node.parent.and_then(|parent| locators.get(&parent)) {
            Some(parent) => format!("{parent}/{segment}"),
            None => segment,
        };
        locators.insert(node.id, locator.clone());
        samples.push(Sample {
            id: node.id,
            parent: node.parent,
            locator,
            rect: node.rect,
            visible: node.visible,
            opacity: node.opacity,
        });
    }
    samples
}

/// One motion a person would notice.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Finding {
    /// The rule it breaks.
    pub rule: Rule,
    /// The node that shows it.
    pub id: RenderId,
    /// That node's [`Sample::locator`].
    pub locator: String,
    /// The frame it shows in.
    pub frame: usize,
    /// That frame's time.
    pub time: Duration,
    /// What was measured.
    pub detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at frame {} ({:.3} s): {}: {}",
            self.rule,
            self.frame,
            self.time.as_secs_f64(),
            self.locator,
            self.detail
        )
    }
}

/// An allowlist row: motion of `rule` on nodes whose locator contains `locator` is deliberate,
/// for `reason`, at most `max` times (ADR-0321 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Allow {
    /// The rule the row allows.
    pub rule: Rule,
    /// A part of the [`Sample::locator`] of the nodes it covers.
    pub locator: &'static str,
    /// Why the motion is deliberate.
    pub reason: &'static str,
    /// How many findings it covers.
    pub max: usize,
}

impl Allow {
    /// A row covering one finding.
    #[must_use]
    pub const fn new(rule: Rule, locator: &'static str, reason: &'static str) -> Self {
        Self {
            rule,
            locator,
            reason,
            max: 1,
        }
    }

    /// The row covers up to `max` findings.
    #[must_use]
    pub const fn max(self, max: usize) -> Self {
        Self { max, ..self }
    }

    fn covers(&self, finding: &Finding) -> bool {
        self.rule == finding.rule
            && !self.locator.is_empty()
            && finding.locator.contains(self.locator)
    }
}

/// Why a run failed its gate.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct MotionReport {
    /// Findings no row covers.
    pub unallowed: Vec<Finding>,
    /// Rows that matched more findings than their `max`, with the count.
    pub over_budget: Vec<(Allow, usize)>,
    /// Rows that matched nothing: the motion they described is gone, so the row goes too.
    pub unused: Vec<Allow>,
    /// Rows without a reason or a locator.
    pub unexplained: Vec<Allow>,
}

impl MotionReport {
    /// Whether the run passes.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.unallowed.is_empty()
            && self.over_budget.is_empty()
            && self.unused.is_empty()
            && self.unexplained.is_empty()
    }
}

impl fmt::Display for MotionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for finding in &self.unallowed {
            writeln!(f, "{finding}")?;
        }
        for (row, count) in &self.over_budget {
            writeln!(
                f,
                "allow row {} on {:?} matched {count} findings, max {}",
                row.rule, row.locator, row.max
            )?;
        }
        for row in &self.unused {
            writeln!(
                f,
                "allow row {} on {:?} matched nothing: delete it",
                row.rule, row.locator
            )?;
        }
        for row in &self.unexplained {
            writeln!(
                f,
                "allow row {} on {:?} needs a locator and a reason",
                row.rule, row.locator
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for MotionReport {}

/// Checks `recording` with `config` and compares the findings with `allow` (ADR-0321 §3).
///
/// # Errors
/// [`MotionReport`] when a finding is not covered, a row covers more than its `max`, a row
/// matches nothing or a row has no locator or reason.
pub fn gate(
    recording: &Recording,
    config: &MotionConfig,
    allow: &[Allow],
) -> Result<(), MotionReport> {
    let mut report = MotionReport::default();
    for row in allow {
        if row.locator.is_empty() || row.reason.trim().is_empty() {
            report.unexplained.push(*row);
        }
    }
    let mut used = vec![0_usize; allow.len()];
    for finding in check(recording, config) {
        match allow.iter().position(|row| row.covers(&finding)) {
            Some(index) => used[index] += 1,
            None => report.unallowed.push(finding),
        }
    }
    for (row, &count) in allow.iter().zip(&used) {
        if count == 0 {
            report.unused.push(*row);
        } else if count > row.max {
            report.over_budget.push((*row, count));
        }
    }
    if report.is_clean() {
        Ok(())
    } else {
        Err(report)
    }
}

/// [`gate`] with [`MotionConfig::default`], panicking with the report.
///
/// # Panics
/// When the gate fails.
#[track_caller]
pub fn assert_smooth(recording: &Recording, allow: &[Allow]) {
    if let Err(report) = gate(recording, &MotionConfig::default(), allow) {
        panic!("motion a person would notice:\n{report}");
    }
}
