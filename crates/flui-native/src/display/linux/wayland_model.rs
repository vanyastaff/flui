//! The Wayland output model: what `wl_output` (version 2 to 4) and
//! `zxdg_output_v1` say, kept until the compositor says it is consistent.
//!
//! Wayland sends an output's properties as a burst of events closed by
//! `wl_output.done`. A reader that applied each event on arrival would publish
//! an output with its new mode and its old scale, so [`WaylandOutputs`]
//! accumulates the events of each output and takes them over only at `done`.
//!
//! The model is pure: it is fed [`OutputEvent`]s and answers [`OutputView`]s,
//! and knows nothing of the connection that produced them.
//!
//! # What an output is
//!
//! - **Bounds** are in the compositor's logical space, the only global space
//!   Wayland has. The position and size come from `zxdg_output_v1` when the
//!   compositor offers it, otherwise from `wl_output.geometry` and the current
//!   mode divided by the integer scale (swapped for a quarter-turn transform).
//! - **Scale** is the current mode's width over the logical width when a
//!   logical size is known, so a fractionally scaled output reports 1.5 and not
//!   the 2 `wl_output.scale` rounds it up to. Without a logical size it is the
//!   integer `wl_output.scale`.
//! - **Refresh** is `wl_output.mode`'s `refresh` in millihertz; zero or less
//!   means the compositor does not know.
//! - **Identity** is the version 4 `name` (the connector, such as `DP-1`). An
//!   output without one gets a fresh id for as long as its `wl_output` global
//!   exists. Two live outputs that report one name are two displays: the later
//!   one is not keyed by it.
//! - There is no primary output and no work area; the usable area is the
//!   bounds.

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{DisplayKey, RefreshPeriod};

use super::view::{Identity, OutputView};

/// One event of an output, in the terms the model needs. The adapter maps the
/// protocol's events onto these.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OutputEvent {
    /// `wl_output.geometry`: the position in the compositor space, and whether
    /// the transform turns the output by a quarter (so width and height swap).
    Geometry {
        x: i32,
        y: i32,
        swaps_axes: bool,
        make: String,
        model: String,
    },
    /// `wl_output.mode`. Only the mode flagged `current` is kept.
    Mode {
        current: bool,
        width: i32,
        height: i32,
        refresh_millihertz: i32,
    },
    /// `wl_output.scale`.
    Scale(i32),
    /// `wl_output.name` (version 4).
    Name(String),
    /// `wl_output.description` (version 4).
    Description(String),
    /// `zxdg_output_v1.logical_position`.
    LogicalPosition { x: i32, y: i32 },
    /// `zxdg_output_v1.logical_size`.
    LogicalSize { width: i32, height: i32 },
    /// `wl_output.done`, or `zxdg_output_v1.done` for a compositor whose
    /// xdg-output is older than version 3.
    Done,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct OutputState {
    position: Option<(i32, i32)>,
    swaps_axes: bool,
    make: String,
    model: String,
    mode: Option<Mode>,
    scale: i32,
    name: Option<String>,
    description: Option<String>,
    logical_position: Option<(i32, i32)>,
    logical_size: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Mode {
    width: i32,
    height: i32,
    refresh_millihertz: i32,
}

#[derive(Debug)]
struct Output {
    global: u32,
    /// Events since the last `done`.
    pending: OutputState,
    /// The state at the last `done`; `None` until the first one.
    applied: Option<OutputState>,
    /// An xdg-output was asked for before the output was first listed, and its
    /// logical size has not arrived: the first listing waits for it, so a
    /// hotplugged output is not published with a guessed size and scale.
    awaiting_logical_size: bool,
}

/// Every `wl_output` the compositor advertises.
#[derive(Debug, Default)]
pub(crate) struct WaylandOutputs {
    outputs: Vec<Output>,
}

impl WaylandOutputs {
    /// An output appeared, under the registry name `global`. It is listed once
    /// its first `done` arrives.
    pub(crate) fn add(&mut self, global: u32) {
        if self.outputs.iter().any(|output| output.global == global) {
            return;
        }
        self.outputs.push(Output {
            global,
            pending: OutputState::default(),
            applied: None,
            awaiting_logical_size: false,
        });
    }

    /// An xdg-output was requested for `global`, so its logical size will
    /// follow. An output not yet listed is held until it does.
    pub(crate) fn expect_logical_size(&mut self, global: u32) {
        if let Some(output) = self
            .outputs
            .iter_mut()
            .find(|output| output.global == global && output.applied.is_none())
        {
            output.awaiting_logical_size = true;
        }
    }

    /// The output under `global` went away (`wl_registry.global_remove`).
    /// Returns whether the listed outputs changed.
    pub(crate) fn remove(&mut self, global: u32) -> bool {
        let Some(index) = self
            .outputs
            .iter()
            .position(|output| output.global == global)
        else {
            return false;
        };
        self.outputs.remove(index).applied.is_some()
    }

    /// Feed one event of the output `global`. Returns whether the listed
    /// outputs changed, which only a `done` can cause; an event for an output
    /// the model does not hold is ignored.
    pub(crate) fn apply(&mut self, global: u32, event: OutputEvent) -> bool {
        let Some(output) = self
            .outputs
            .iter_mut()
            .find(|output| output.global == global)
        else {
            return false;
        };
        let pending = &mut output.pending;
        match event {
            OutputEvent::Geometry {
                x,
                y,
                swaps_axes,
                make,
                model,
            } => {
                pending.position = Some((x, y));
                pending.swaps_axes = swaps_axes;
                pending.make = make;
                pending.model = model;
            }
            OutputEvent::Mode {
                current,
                width,
                height,
                refresh_millihertz,
            } => {
                if current {
                    pending.mode = Some(Mode {
                        width,
                        height,
                        refresh_millihertz,
                    });
                }
            }
            OutputEvent::Scale(scale) => pending.scale = scale,
            OutputEvent::Name(name) => pending.name = Some(name),
            OutputEvent::Description(description) => pending.description = Some(description),
            OutputEvent::LogicalPosition { x, y } => pending.logical_position = Some((x, y)),
            OutputEvent::LogicalSize { width, height } => {
                pending.logical_size = Some((width, height));
                // The `done` of the burst came before the logical size and was
                // held back; the size completes the first state, so it is
                // taken over now rather than waiting for a `done` that a
                // compositor need not repeat.
                if std::mem::take(&mut output.awaiting_logical_size) {
                    output.applied = Some(output.pending.clone());
                    return true;
                }
            }
            OutputEvent::Done => {
                if output.awaiting_logical_size {
                    return false;
                }
                let changed = output.applied.as_ref() != Some(&output.pending);
                output.applied = Some(output.pending.clone());
                return changed;
            }
        }
        false
    }

    /// The outputs as of their last `done`, in the order they appeared. An
    /// output that has not completed one, or has no current mode, is not
    /// listed.
    pub(crate) fn views(&self) -> Vec<OutputView> {
        let mut taken_names: Vec<&str> = Vec::new();
        let mut views = Vec::with_capacity(self.outputs.len());
        for output in &self.outputs {
            let Some(state) = &output.applied else {
                continue;
            };
            // The name is the key only the first time it is seen in this list.
            let key_name = state
                .name
                .as_deref()
                .filter(|name| !name.is_empty() && !taken_names.contains(name));
            if let Some(name) = key_name {
                taken_names.push(name);
            }
            if let Some(view) = view_of(output.global, state, key_name) {
                views.push(view);
            }
        }
        views
    }
}

fn view_of(global: u32, state: &OutputState, key_name: Option<&str>) -> Option<OutputView> {
    let mode = state.mode?;
    if mode.width <= 0 || mode.height <= 0 {
        return None;
    }
    let (pixel_width, pixel_height) = if state.swaps_axes {
        (mode.height, mode.width)
    } else {
        (mode.width, mode.height)
    };
    let integer_scale = state.scale.max(1);
    let logical = state
        .logical_size
        .filter(|&(width, height)| width > 0 && height > 0);
    let (width, height, scale_factor) = match logical {
        Some((width, height)) => (width, height, f64::from(pixel_width) / f64::from(width)),
        None => (
            (pixel_width / integer_scale).max(1),
            (pixel_height / integer_scale).max(1),
            f64::from(integer_scale),
        ),
    };
    let (x, y) = state
        .logical_position
        .or(state.position)
        .unwrap_or_default();
    let bounds = Bounds::new(Point::new(x, y), Size::new(width, height));
    let identity = match key_name {
        Some(name) => Identity::Keyed(DisplayKey::WaylandOutput(name.to_owned())),
        None => Identity::Fresh { slot: global },
    };
    Some(OutputView {
        identity,
        natives: vec![global],
        name: label_of(state),
        bounds,
        usable_bounds: bounds,
        scale_factor,
        // `wl_output.mode`'s `refresh` is millihertz in an `i32`; zero or
        // less means the compositor does not know.
        refresh_period: RefreshPeriod::from_millihertz(i64::from(mode.refresh_millihertz)).ok(),
        is_primary: false,
        persistent_key: key_name.map(str::to_owned),
    })
}

/// A label for people: the description, then the connector, then the make and
/// model.
fn label_of(state: &OutputState) -> String {
    let nonempty = |text: &str| (!text.trim().is_empty()).then(|| text.trim().to_owned());
    state
        .description
        .as_deref()
        .and_then(nonempty)
        .or_else(|| state.name.as_deref().and_then(nonempty))
        .or_else(|| nonempty(&format!("{} {}", state.make, state.model)))
        .unwrap_or_else(|| "Display".to_owned())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use flui_platform::DisplayRegistry;

    use super::super::view::FreshIds;
    use super::*;

    const GLOBAL: u32 = 41;

    fn geometry(x: i32, y: i32) -> OutputEvent {
        OutputEvent::Geometry {
            x,
            y,
            swaps_axes: false,
            make: "Acme".into(),
            model: "Panel".into(),
        }
    }

    fn mode(width: i32, height: i32, refresh_millihertz: i32) -> OutputEvent {
        OutputEvent::Mode {
            current: true,
            width,
            height,
            refresh_millihertz,
        }
    }

    /// Feed an output its whole burst and `done`.
    fn announce(outputs: &mut WaylandOutputs, global: u32, events: Vec<OutputEvent>) {
        outputs.add(global);
        for event in events {
            outputs.apply(global, event);
        }
        outputs.apply(global, OutputEvent::Done);
    }

    fn only(outputs: &WaylandOutputs) -> OutputView {
        let mut views = outputs.views();
        assert_eq!(views.len(), 1, "{views:?}");
        views.remove(0)
    }

    fn wayland_output_applies_only_after_done() {
        let mut outputs = WaylandOutputs::default();
        outputs.add(GLOBAL);
        for event in [
            geometry(0, 0),
            mode(1920, 1080, 60_000),
            OutputEvent::Scale(1),
        ] {
            assert!(!outputs.apply(GLOBAL, event));
            assert!(outputs.views().is_empty(), "listed before its first done");
        }
        assert!(outputs.apply(GLOBAL, OutputEvent::Done));
        let first = only(&outputs);
        assert_eq!(first.bounds.size.width, 1920);
        assert_eq!(first.scale_factor, 1.0);

        // A mode and a scale change arrive as a burst: neither shows until done.
        assert!(!outputs.apply(GLOBAL, mode(3840, 2160, 120_000)));
        assert!(!outputs.apply(GLOBAL, OutputEvent::Scale(2)));
        assert_eq!(only(&outputs), first, "half a burst was published");
        assert!(outputs.apply(GLOBAL, OutputEvent::Done));
        let second = only(&outputs);
        assert_eq!(second.bounds.size.width, 1920, "3840 pixels at scale 2");
        assert_eq!(second.scale_factor, 2.0);
        assert_eq!(
            second.refresh_period.map(RefreshPeriod::get),
            Some(Duration::from_nanos(8_333_333))
        );

        // A done that changes nothing reports nothing.
        assert!(!outputs.apply(GLOBAL, OutputEvent::Done));
    }

    fn wayland_name_v4_keys_the_output() {
        let mut outputs = WaylandOutputs::default();
        announce(
            &mut outputs,
            GLOBAL,
            vec![
                geometry(0, 0),
                mode(1920, 1080, 60_000),
                OutputEvent::Name("DP-1".into()),
                OutputEvent::Description("Acme Panel (DP-1)".into()),
            ],
        );
        let view = only(&outputs);
        assert_eq!(
            view.identity,
            Identity::Keyed(DisplayKey::WaylandOutput("DP-1".into()))
        );
        assert_eq!(view.name, "Acme Panel (DP-1)");
        assert_eq!(
            view.persistent_key.as_deref(),
            Some("DP-1"),
            "the connector"
        );

        // The connector is unplugged and plugged in again: a new global, the
        // same key, so the same display.
        let registry = DisplayRegistry::new();
        let mut fresh = FreshIds::default();
        let before = fresh.resolve(&outputs.views(), |key| registry.mint(key).ok());
        assert!(outputs.remove(GLOBAL));
        announce(
            &mut outputs,
            GLOBAL + 1,
            vec![
                geometry(0, 0),
                mode(1920, 1080, 60_000),
                OutputEvent::Name("DP-1".into()),
            ],
        );
        let after = fresh.resolve(&outputs.views(), |key| registry.mint(key).ok());
        assert_eq!(before.displays[0].id, after.displays[0].id);
    }

    fn wayland_below_v4_mints_fresh() {
        let registry = DisplayRegistry::new();
        let mut fresh = FreshIds::default();
        let mut outputs = WaylandOutputs::default();
        // No name event: a wl_output older than version 4.
        announce(
            &mut outputs,
            GLOBAL,
            vec![geometry(0, 0), mode(1920, 1080, 60_000)],
        );
        let view = only(&outputs);
        assert_eq!(view.identity, Identity::Fresh { slot: GLOBAL });
        assert_eq!(view.name, "Acme Panel", "the make and model label it");
        assert_eq!(view.persistent_key, None, "no connector name to key it by");

        let first = fresh.resolve(&outputs.views(), |key| registry.mint(key).ok());
        // A later change keeps the id of the same global.
        outputs.apply(GLOBAL, OutputEvent::Scale(2));
        outputs.apply(GLOBAL, OutputEvent::Done);
        let changed = fresh.resolve(&outputs.views(), |key| registry.mint(key).ok());
        assert_eq!(first.displays[0].id, changed.displays[0].id);

        // The global goes away and a new one stands for the same screen: it is
        // a new display, since nothing says it is the same.
        outputs.remove(GLOBAL);
        fresh.resolve(&outputs.views(), |key| registry.mint(key).ok());
        announce(
            &mut outputs,
            GLOBAL + 1,
            vec![geometry(0, 0), mode(1920, 1080, 60_000)],
        );
        let replaced = fresh.resolve(&outputs.views(), |key| registry.mint(key).ok());
        assert_ne!(first.displays[0].id, replaced.displays[0].id);
    }

    fn wayland_removed_output_leaves_the_snapshot() {
        let mut outputs = WaylandOutputs::default();
        announce(
            &mut outputs,
            1,
            vec![geometry(0, 0), mode(1920, 1080, 60_000)],
        );
        announce(
            &mut outputs,
            2,
            vec![geometry(1920, 0), mode(1280, 720, 50_000)],
        );
        assert_eq!(outputs.views().len(), 2);
        assert!(outputs.remove(1));
        let remaining = only(&outputs);
        assert_eq!(remaining.natives, vec![2]);
        // An output removed before its first done was never listed.
        outputs.add(3);
        assert!(!outputs.remove(3));
        assert!(!outputs.remove(99), "unknown global");
        // Events for a removed output are ignored.
        assert!(!outputs.apply(1, OutputEvent::Done));
        assert_eq!(outputs.views().len(), 1);
    }

    fn mirrored_outputs_keep_distinct_ids() {
        // Two outputs showing one picture: the same position and size.
        let registry = DisplayRegistry::new();
        let mut fresh = FreshIds::default();
        let mut outputs = WaylandOutputs::default();
        for (global, name) in [(5, "DP-1"), (6, "HDMI-A-1")] {
            announce(
                &mut outputs,
                global,
                vec![
                    geometry(0, 0),
                    mode(1920, 1080, 60_000),
                    OutputEvent::Name(name.into()),
                ],
            );
        }
        let views = outputs.views();
        assert_eq!(views[0].bounds, views[1].bounds);
        let resolved = fresh.resolve(&views, |key| registry.mint(key).ok());
        assert_eq!(resolved.displays.len(), 2);
        assert_ne!(resolved.displays[0].id, resolved.displays[1].id);

        // Mirrored outputs that carry no name are told apart by their globals.
        let mut unnamed = WaylandOutputs::default();
        for global in [7, 8] {
            announce(
                &mut unnamed,
                global,
                vec![geometry(0, 0), mode(1920, 1080, 60_000)],
            );
        }
        let resolved = fresh.resolve(&unnamed.views(), |key| registry.mint(key).ok());
        assert_ne!(resolved.displays[0].id, resolved.displays[1].id);
    }

    fn two_outputs_reporting_one_name_are_two_displays() {
        let registry = DisplayRegistry::new();
        let mut fresh = FreshIds::default();
        let mut outputs = WaylandOutputs::default();
        for global in [5, 6] {
            announce(
                &mut outputs,
                global,
                vec![
                    geometry(0, 0),
                    mode(1920, 1080, 60_000),
                    OutputEvent::Name("DP-1".into()),
                ],
            );
        }
        let resolved = fresh.resolve(&outputs.views(), |key| registry.mint(key).ok());
        assert_eq!(resolved.displays.len(), 2);
        assert_ne!(resolved.displays[0].id, resolved.displays[1].id);
    }

    fn wayland_refresh_at_or_below_zero_is_unknown() {
        for refresh in [0, -1, i32::MIN] {
            let mut outputs = WaylandOutputs::default();
            announce(
                &mut outputs,
                GLOBAL,
                vec![geometry(0, 0), mode(1920, 1080, refresh)],
            );
            assert_eq!(only(&outputs).refresh_period, None, "refresh {refresh}");
        }
        // 59.94 Hz, the usual television rate, is kept exactly enough.
        let mut outputs = WaylandOutputs::default();
        announce(
            &mut outputs,
            GLOBAL,
            vec![geometry(0, 0), mode(1920, 1080, 59_940)],
        );
        let period = only(&outputs).refresh_period.expect("a known rate");
        assert_eq!(period.get(), Duration::from_nanos(16_683_350));
        // A rate whose period is outside 1 ms to 1 s does not describe a cadence.
        let mut outputs = WaylandOutputs::default();
        announce(
            &mut outputs,
            GLOBAL,
            vec![geometry(0, 0), mode(1920, 1080, 500)],
        );
        assert_eq!(only(&outputs).refresh_period, None);
    }

    fn logical_size_gives_a_fractional_scale_and_the_bounds() {
        let mut outputs = WaylandOutputs::default();
        announce(
            &mut outputs,
            GLOBAL,
            vec![
                geometry(0, 0),
                mode(3840, 2160, 60_000),
                // wl_output.scale rounds 1.5 up.
                OutputEvent::Scale(2),
                OutputEvent::LogicalPosition { x: 1280, y: 0 },
                OutputEvent::LogicalSize {
                    width: 2560,
                    height: 1440,
                },
            ],
        );
        let view = only(&outputs);
        assert_eq!(view.scale_factor, 1.5);
        assert_eq!(view.bounds.origin.x, 1280);
        assert_eq!(view.bounds.size.width, 2560);
        assert_eq!(view.bounds.size.height, 1440);
        assert_eq!(view.usable_bounds, view.bounds);
        assert!(!view.is_primary, "Wayland has no primary output");
    }

    fn a_quarter_turn_swaps_the_mode_without_a_logical_size() {
        let mut outputs = WaylandOutputs::default();
        announce(
            &mut outputs,
            GLOBAL,
            vec![
                OutputEvent::Geometry {
                    x: -1080,
                    y: 0,
                    swaps_axes: true,
                    make: String::new(),
                    model: String::new(),
                },
                mode(1920, 1080, 60_000),
                OutputEvent::Scale(1),
            ],
        );
        let view = only(&outputs);
        assert_eq!(view.bounds.size.width, 1080);
        assert_eq!(view.bounds.size.height, 1920);
        assert_eq!(view.bounds.origin.x, -1080);
        assert_eq!(view.name, "Display", "nothing to label it with");
    }

    fn a_hotplugged_output_waits_for_its_logical_size() {
        let mut outputs = WaylandOutputs::default();
        outputs.add(GLOBAL);
        outputs.expect_logical_size(GLOBAL);
        for event in [
            geometry(0, 0),
            mode(3840, 2160, 60_000),
            OutputEvent::Scale(2),
        ] {
            outputs.apply(GLOBAL, event);
        }
        // `done` arrives before the xdg-output has said anything.
        assert!(!outputs.apply(GLOBAL, OutputEvent::Done));
        assert_eq!(outputs.views(), [], "published with a guessed size");
        outputs.apply(GLOBAL, OutputEvent::LogicalPosition { x: 0, y: 0 });
        // The logical size completes the first state, with no second `done`.
        assert!(outputs.apply(
            GLOBAL,
            OutputEvent::LogicalSize {
                width: 2560,
                height: 1440
            }
        ));
        let view = only(&outputs);
        assert_eq!(view.scale_factor, 1.5);
        assert_eq!(view.bounds.size.width, 2560);
        // Once listed, a later burst applies at its own `done` as usual.
        outputs.apply(GLOBAL, OutputEvent::Scale(1));
        assert_eq!(only(&outputs), view);
        assert!(outputs.apply(GLOBAL, OutputEvent::Done));
        assert_eq!(only(&outputs).scale_factor, 1.5, "the logical size rules");
    }

    fn an_output_without_a_usable_mode_is_not_listed() {
        let mut outputs = WaylandOutputs::default();
        announce(&mut outputs, 1, vec![geometry(0, 0)]);
        announce(&mut outputs, 2, vec![geometry(0, 0), mode(0, 1080, 60_000)]);
        // A mode that is not the current one does not count.
        announce(
            &mut outputs,
            3,
            vec![
                geometry(0, 0),
                OutputEvent::Mode {
                    current: false,
                    width: 1920,
                    height: 1080,
                    refresh_millihertz: 60_000,
                },
            ],
        );
        assert_eq!(outputs.views(), []);
    }

    #[test]
    fn wayland_output_model() {
        crate::table_test::run_table(
            "wayland_output_model",
            &[
                (
                    "wayland_output_applies_only_after_done",
                    wayland_output_applies_only_after_done as fn(),
                ),
                (
                    "wayland_name_v4_keys_the_output",
                    wayland_name_v4_keys_the_output,
                ),
                ("wayland_below_v4_mints_fresh", wayland_below_v4_mints_fresh),
                (
                    "wayland_removed_output_leaves_the_snapshot",
                    wayland_removed_output_leaves_the_snapshot,
                ),
                (
                    "mirrored_outputs_keep_distinct_ids",
                    mirrored_outputs_keep_distinct_ids,
                ),
                (
                    "two_outputs_reporting_one_name_are_two_displays",
                    two_outputs_reporting_one_name_are_two_displays,
                ),
                (
                    "wayland_refresh_at_or_below_zero_is_unknown",
                    wayland_refresh_at_or_below_zero_is_unknown,
                ),
                (
                    "logical_size_gives_a_fractional_scale_and_the_bounds",
                    logical_size_gives_a_fractional_scale_and_the_bounds,
                ),
                (
                    "a_quarter_turn_swaps_the_mode_without_a_logical_size",
                    a_quarter_turn_swaps_the_mode_without_a_logical_size,
                ),
                (
                    "a_hotplugged_output_waits_for_its_logical_size",
                    a_hotplugged_output_waits_for_its_logical_size,
                ),
                (
                    "an_output_without_a_usable_mode_is_not_listed",
                    an_output_without_a_usable_mode_is_not_listed,
                ),
            ],
        );
    }
}
