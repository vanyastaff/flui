//! The rules a [`Recording`] is read against (ADR-0321 §2) and the charge of each finding to the
//! outermost node that shows it.

use std::time::Duration;

use flui_foundation::RenderId;
use rustc_hash::{FxHashMap, FxHashSet};

use super::{Finding, MotionConfig, Recording, Rule, Sample};

/// The opacity difference below which two frames count as drawing a node the same.
const SAME_OPACITY: f64 = 0.01;

/// How close a flicker's return must come to the opacity it dipped from.
const RETURN_OPACITY: f64 = 0.05;

/// Every finding of every rule, each on every node that shows it: a node moving with its
/// parent repeats the parent's finding. [`check`] is this charged to the outermost node.
#[must_use]
pub fn check_all(recording: &Recording, config: &MotionConfig) -> Vec<Finding> {
    let frames = recording.frames();
    let timelines = timelines(recording);
    let mut findings = Vec::new();
    let last_frame = frames.len().saturating_sub(1);
    let last_input = frames.iter().rposition(|frame| frame.input).unwrap_or(0);
    for (id, line) in &timelines {
        presence(recording, config, *id, line, last_frame, &mut findings);
        steps(recording, config, *id, line, &mut findings);
        overshoot(recording, config, *id, line, last_input, &mut findings);
    }
    never_settles(recording, config, &timelines, last_input, &mut findings);
    wakes_after_end(recording, config, last_input, &mut findings);
    findings.sort_by(|a, b| (a.frame, a.rule, &a.locator).cmp(&(b.frame, b.rule, &b.locator)));
    findings
}

/// [`check_all`] charged to the outermost node that shows each finding
/// ([`charge_to_outermost`]).
#[must_use]
pub fn check(recording: &Recording, config: &MotionConfig) -> Vec<Finding> {
    charge_to_outermost(check_all(recording, config), recording)
}

/// Drops each finding whose node's parent, in the finding's frame, shows a finding of the same
/// rule in the same frame (ADR-0321 §2). A node that moves, fades or mounts with its parent says
/// nothing its parent did not, so a jump of a row is reported once, on the row, and not again on
/// each of the ten wrappers inside it.
#[must_use]
pub fn charge_to_outermost(findings: Vec<Finding>, recording: &Recording) -> Vec<Finding> {
    let shown: FxHashSet<(Rule, usize, RenderId)> = findings
        .iter()
        .map(|finding| (finding.rule, finding.frame, finding.id))
        .collect();
    findings
        .into_iter()
        .filter(|finding| {
            let parent = parent_in(recording, finding.frame, finding.id);
            !parent.is_some_and(|parent| shown.contains(&(finding.rule, finding.frame, parent)))
        })
        .collect()
}

/// The parent of `id` in frame `frame`, or in the frame before when `id` is gone from it (a
/// `pop-out` is reported on the frame that no longer draws the node).
fn parent_in(recording: &Recording, frame: usize, id: RenderId) -> Option<RenderId> {
    let frames = recording.frames();
    [Some(frame), frame.checked_sub(1)]
        .into_iter()
        .flatten()
        .filter_map(|index| frames.get(index))
        .find_map(|frame| frame.samples.iter().find(|sample| sample.id == id))
        .and_then(|sample| sample.parent)
}

/// Each node's samples by frame index. Ids are generational, so a node that leaves never comes
/// back under the same id and its frames are contiguous.
fn timelines(recording: &Recording) -> FxHashMap<RenderId, Vec<(usize, &Sample)>> {
    let mut lines: FxHashMap<RenderId, Vec<(usize, &Sample)>> = FxHashMap::default();
    for (index, frame) in recording.frames().iter().enumerate() {
        for sample in &frame.samples {
            lines.entry(sample.id).or_default().push((index, sample));
        }
    }
    lines
}

fn finding(
    recording: &Recording,
    rule: Rule,
    id: RenderId,
    frame: usize,
    locator: &str,
    detail: String,
) -> Finding {
    Finding {
        rule,
        id,
        locator: locator.to_owned(),
        frame,
        time: recording
            .frames()
            .get(frame)
            .map_or(Duration::ZERO, |frame| frame.time),
        detail,
    }
}

fn same_place(a: &Sample, b: &Sample, still: f64) -> bool {
    a.edges()
        .iter()
        .zip(b.edges())
        .all(|(x, y)| (x - y).abs() < still)
        && (a.opacity - b.opacity).abs() < SAME_OPACITY
        && a.visible.is_some() == b.visible.is_some()
}

/// `pop-in` and `pop-out`. Frame 0 is the settled start: what it draws loaded with the view.
fn presence(
    recording: &Recording,
    config: &MotionConfig,
    id: RenderId,
    line: &[(usize, &Sample)],
    last_frame: usize,
    out: &mut Vec<Finding>,
) {
    if let Some(&(first, sample)) = line.first()
        && first > 0
        && sample.is_shown()
        && sample.opacity >= config.pop_opacity
    {
        let unchanged_next = line
            .get(1)
            .is_none_or(|&(_, next)| same_place(sample, next, config.still));
        if unchanged_next {
            out.push(finding(
                recording,
                Rule::PopIn,
                id,
                first,
                &sample.locator,
                format!(
                    "mounted at opacity {:.2} with nothing changing",
                    sample.opacity
                ),
            ));
        }
    }
    if let Some(&(last, sample)) = line.last()
        && last < last_frame
        && sample.is_shown()
        && sample.opacity >= config.pop_opacity
    {
        out.push(finding(
            recording,
            Rule::PopOut,
            id,
            last + 1,
            &sample.locator,
            format!("unmounted at opacity {:.2}", sample.opacity),
        ));
    }
}

/// `jump`, `fade-snap` and `flicker`: what one frame does against its neighbours.
fn steps(
    recording: &Recording,
    config: &MotionConfig,
    id: RenderId,
    line: &[(usize, &Sample)],
    out: &mut Vec<Finding>,
) {
    let frames = recording.frames();
    for at in 1..line.len() {
        let (frame, sample) = line[at];
        let (_, before) = line[at - 1];
        let direct = frames.get(frame).is_some_and(|frame| frame.direct);
        if !direct && before.is_shown() && sample.is_shown() {
            let largest = largest_jump(line, at, config);
            if let Some(distance) = largest {
                out.push(finding(
                    recording,
                    Rule::Jump,
                    id,
                    frame,
                    &sample.locator,
                    format!("an edge moved {distance:.1} pt in one frame between still frames"),
                ));
            }
        }
        if before.visible.is_some() && sample.visible.is_some() {
            let step = sample.opacity - before.opacity;
            if step.abs() >= config.fade_snap {
                out.push(finding(
                    recording,
                    Rule::FadeSnap,
                    id,
                    frame,
                    &sample.locator,
                    format!(
                        "opacity stepped {:.2} to {:.2} in one frame",
                        before.opacity, sample.opacity
                    ),
                ));
            }
            if before.opacity - sample.opacity >= config.flicker_dip {
                let dip_time = frames.get(frame).map_or(Duration::ZERO, |f| f.time);
                let returns = line[at + 1..].iter().take_while(|(later, _)| {
                    frames
                        .get(*later)
                        .is_some_and(|f| f.time.saturating_sub(dip_time) <= config.flicker_window)
                });
                if returns
                    .clone()
                    .any(|(_, later)| later.opacity >= before.opacity - RETURN_OPACITY)
                {
                    out.push(finding(
                        recording,
                        Rule::Flicker,
                        id,
                        frame,
                        &sample.locator,
                        format!(
                            "opacity dipped {:.2} to {:.2} and came back",
                            before.opacity, sample.opacity
                        ),
                    ));
                }
            }
        }
    }
}

/// The largest edge step into `line[at]` when the node held still on both sides of it.
fn largest_jump(line: &[(usize, &Sample)], at: usize, config: &MotionConfig) -> Option<f64> {
    let edges = |index: usize| line.get(index).map(|(_, sample)| sample.edges());
    let current = edges(at)?;
    let before = edges(at - 1)?;
    let before_before = at.checked_sub(2).and_then(edges);
    let after = edges(at + 1);
    let mut largest: Option<f64> = None;
    for edge in 0..4 {
        let step = (current[edge] - before[edge]).abs();
        if step < config.jump {
            continue;
        }
        let still_before =
            before_before.is_none_or(|b| (before[edge] - b[edge]).abs() < config.still);
        let still_after = after.is_none_or(|a| (a[edge] - current[edge]).abs() < config.still);
        if still_before && still_after {
            largest = Some(largest.map_or(step, |l: f64| l.max(step)));
        }
    }
    largest
}

/// `overshoot`: from the last input to the end of the run, an edge that travelled passes where it
/// came to rest.
fn overshoot(
    recording: &Recording,
    config: &MotionConfig,
    id: RenderId,
    line: &[(usize, &Sample)],
    last_input: usize,
    out: &mut Vec<Finding>,
) {
    let Some(start) = line.iter().position(|&(frame, _)| frame >= last_input) else {
        return;
    };
    let travel = &line[start..];
    let (Some(&(_, first)), Some(&(_, rest))) = (travel.first(), travel.last()) else {
        return;
    };
    let mut worst: Option<(usize, f64)> = None;
    for edge in 0..4 {
        let from = first.edges()[edge];
        let to = rest.edges()[edge];
        let distance = to - from;
        if distance.abs() < config.jump {
            continue;
        }
        for &(frame, sample) in travel {
            let past = (sample.edges()[edge] - to) * distance.signum();
            if past > config.overshoot && worst.is_none_or(|(_, w)| past > w) {
                worst = Some((frame, past));
            }
        }
    }
    if let Some((frame, past)) = worst {
        out.push(finding(
            recording,
            Rule::Overshoot,
            id,
            frame,
            &rest.locator,
            format!("an edge passed its resting place by {past:.1} pt"),
        ));
    }
}

/// `never-settles`: a node still moving or fading in the run's last stretch, when that stretch
/// comes after the last input. A run that ends sooner says nothing about settling.
fn never_settles(
    recording: &Recording,
    config: &MotionConfig,
    timelines: &FxHashMap<RenderId, Vec<(usize, &Sample)>>,
    last_input: usize,
    out: &mut Vec<Finding>,
) {
    let frames = recording.frames();
    let (Some(last), Some(input)) = (frames.last(), frames.get(last_input)) else {
        return;
    };
    if last.time.saturating_sub(input.time) < config.settle_window {
        return;
    }
    let from = last.time.saturating_sub(config.settle_window);
    let Some(window_start) = frames.iter().position(|frame| frame.time >= from) else {
        return;
    };
    for (id, line) in timelines {
        let tail: Vec<&Sample> = line
            .iter()
            .filter(|(frame, _)| *frame >= window_start)
            .map(|(_, sample)| *sample)
            .collect();
        let (Some(first), Some(end)) = (tail.first(), tail.last()) else {
            continue;
        };
        if !end.is_shown() {
            continue;
        }
        let moving = tail.iter().any(|sample| {
            sample
                .edges()
                .iter()
                .zip(first.edges())
                .any(|(x, y)| (x - y).abs() >= config.still)
                || (sample.opacity - first.opacity).abs() >= config.settle_opacity
        });
        if moving {
            out.push(finding(
                recording,
                Rule::NeverSettles,
                *id,
                window_start,
                &end.locator,
                format!(
                    "still changing in the last {:.2} s",
                    config.settle_window.as_secs_f64()
                ),
            ));
        }
    }
}

/// `wakes-after-end`: frames still asked for long after the last frame that changed anything.
fn wakes_after_end(
    recording: &Recording,
    config: &MotionConfig,
    last_input: usize,
    out: &mut Vec<Finding>,
) {
    let frames = recording.frames();
    let last_change = (1..frames.len())
        .rev()
        .find(|&index| !same_frame(&frames[index - 1].samples, &frames[index].samples, config))
        .unwrap_or(0)
        .max(last_input);
    let Some(end) = frames.get(last_change) else {
        return;
    };
    let late = frames
        .iter()
        .enumerate()
        .skip(last_change + 1)
        .find(|(_, frame)| {
            frame.wants_frame && frame.time.saturating_sub(end.time) >= config.wake_after_end
        });
    if let (Some((index, frame)), Some(root)) = (late, frames[last_change].samples.first()) {
        out.push(Finding {
            rule: Rule::WakesAfterEnd,
            id: root.id,
            locator: root.locator.clone(),
            frame: index,
            time: frame.time,
            detail: format!(
                "asked for a frame {:.2} s after the last visible change",
                frame.time.saturating_sub(end.time).as_secs_f64()
            ),
        });
    }
}

fn same_frame(before: &[Sample], after: &[Sample], config: &MotionConfig) -> bool {
    before.len() == after.len()
        && before
            .iter()
            .zip(after)
            .all(|(a, b)| a.id == b.id && same_place(a, b, config.still))
}
