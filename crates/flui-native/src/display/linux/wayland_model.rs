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
//! - **Colour** comes from `wp_color_management_v1`, where the compositor has
//!   it: the output's image description, described again whenever the
//!   compositor says it changed. Each description is numbered and only the
//!   newest one's `done` is taken over. Without the protocol, or when the
//!   compositor cannot describe an output, the colour is absent on this
//!   device.

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{
    CapabilityAbsence, ColorGamut, DisplayColor, DisplayKey, Headroom, RefreshPeriod,
};

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

/// What an image description's transfer function says of the range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferRange {
    /// A curve that ends at SDR white (sRGB, gamma, BT.1886, a power curve).
    Standard,
    /// SMPTE ST 2084 (PQ): absolute luminance up to the curve's own ceiling.
    Pq,
    /// Another curve that carries values above white (HLG, extended linear,
    /// extended sRGB).
    Extended,
}

/// One event of an output's image description (`wp_image_description_info_v1`
/// of `wp_color_management_output_v1`), in the terms the model needs. The
/// adapter maps the protocol's events onto these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColorEvent {
    /// `primaries_named`, as a gamut; `None` for a named set the model does
    /// not class, which leaves the gamut to the chromaticities.
    NamedPrimaries(Option<ColorGamut>),
    /// `primaries`: the red primary's x chromaticity, times 1 000 000.
    Primaries { red_x: i32 },
    /// `tf_named` or `tf_power`.
    Transfer(TransferRange),
    /// `luminances`: the primary volume's maximum and the reference white, in
    /// cd/m².
    Luminances { max: u32, reference: u32 },
    /// `target_luminance`: the maximum the output is driven to, in cd/m².
    TargetLuminance { max: u32 },
    /// `done`: the description is complete.
    Done,
}

/// What the events of one image description said, until its `done`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ColorDescription {
    named: Option<ColorGamut>,
    red_x: Option<i32>,
    transfer: Option<TransferRange>,
    max: Option<u32>,
    reference: Option<u32>,
    target_max: Option<u32>,
}

/// Red primaries from this x on are the BT.2020 red (0.708).
const REC2020_RED_X: i32 = 700_000;
/// Red primaries from this x on are the Display P3 red (0.680); below, the
/// sRGB red (0.640).
const P3_RED_X: i32 = 670_000;

/// The colour an output's complete image description gives it.
///
/// The gamut is the named primaries' where the model classes them, otherwise
/// the one the red chromaticity reaches. A description without primaries or a
/// transfer function is one this host does not read. A high-range output's
/// headroom is its target peak over the reference white. Without a target
/// peak, an extended curve's primary-volume maximum stands in; a PQ output's
/// does not, because for PQ that maximum is the curve's ceiling (10 000 cd/m²),
/// not what the output shows, and the headroom is then absent. It is absent
/// too when the reference white is missing or the two give no finite multiple.
fn color_of(description: &ColorDescription) -> Result<DisplayColor, CapabilityAbsence> {
    let gamut = description
        .named
        .or_else(|| {
            description.red_x.map(|red_x| match red_x {
                x if x >= REC2020_RED_X => ColorGamut::Rec2020,
                x if x >= P3_RED_X => ColorGamut::DisplayP3,
                _ => ColorGamut::Srgb,
            })
        })
        .ok_or(CapabilityAbsence::NotYetOnThisHost)?;
    let peak = match description
        .transfer
        .ok_or(CapabilityAbsence::NotYetOnThisHost)?
    {
        TransferRange::Standard => return Ok(DisplayColor::standard(gamut)),
        TransferRange::Pq => description.target_max,
        TransferRange::Extended => description.target_max.or(description.max),
    };
    let headroom = peak
        .zip(description.reference)
        .ok_or(CapabilityAbsence::NotOnThisDevice)
        .and_then(|(peak, reference)| {
            // Both are whole cd/m²: exact in `f32` up to 2^24, finite beyond.
            Headroom::from_luminance(peak as f32, reference as f32)
                .map_err(|_| CapabilityAbsence::NotOnThisDevice)
        });
    Ok(DisplayColor::high(gamut, headroom))
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
    /// The colour of the last complete image description; absent on a
    /// compositor without color management until one arrives.
    color: Result<DisplayColor, CapabilityAbsence>,
    /// The image description being described; replaced by a newer one, so an
    /// older description never overwrites a newer colour.
    describing: Option<(u64, ColorDescription)>,
}

impl Output {
    /// Take `color` over; whether a listed output's colour changed.
    fn set_color(&mut self, color: Result<DisplayColor, CapabilityAbsence>) -> bool {
        let changed = self.color != color;
        self.color = color;
        changed && self.applied.is_some()
    }
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
            color: Err(CapabilityAbsence::NotOnThisDevice),
            describing: None,
        });
    }

    /// The output `global` is being described again, by the image description
    /// numbered `description` (a number the adapter gives each description it
    /// asks for, rising). Events of any earlier description are dropped from
    /// now on.
    pub(crate) fn describe(&mut self, global: u32, description: u64) {
        if let Some(output) = self.output_mut(global) {
            output.describing = Some((description, ColorDescription::default()));
        }
    }

    /// Feed one event of the image description `description` of the output
    /// `global`. Returns whether a listed output's colour changed, which only
    /// the `done` of the newest description can cause.
    pub(crate) fn apply_color(&mut self, global: u32, description: u64, event: ColorEvent) -> bool {
        let Some(output) = self.output_mut(global) else {
            return false;
        };
        let Some((current, pending)) = output.describing.as_mut() else {
            return false;
        };
        if *current != description {
            return false;
        }
        match event {
            ColorEvent::NamedPrimaries(gamut) => pending.named = gamut,
            ColorEvent::Primaries { red_x } => pending.red_x = Some(red_x),
            ColorEvent::Transfer(range) => pending.transfer = Some(range),
            ColorEvent::Luminances { max, reference } => {
                pending.max = Some(max);
                pending.reference = Some(reference);
            }
            ColorEvent::TargetLuminance { max } => pending.target_max = Some(max),
            ColorEvent::Done => {
                let color = color_of(pending);
                output.describing = None;
                return output.set_color(color);
            }
        }
        false
    }

    /// The compositor could not describe the output `global` with the image
    /// description `description` (`wp_image_description_v1.failed`). Returns
    /// whether a listed output's colour changed.
    pub(crate) fn color_failed(&mut self, global: u32, description: u64) -> bool {
        let Some(output) = self.output_mut(global) else {
            return false;
        };
        if output.describing.map(|(current, _)| current) != Some(description) {
            return false;
        }
        output.describing = None;
        output.set_color(Err(CapabilityAbsence::NotOnThisDevice))
    }

    fn output_mut(&mut self, global: u32) -> Option<&mut Output> {
        self.outputs
            .iter_mut()
            .find(|output| output.global == global)
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
            if let Some(view) = view_of(output.global, state, key_name, output.color) {
                views.push(view);
            }
        }
        views
    }
}

fn view_of(
    global: u32,
    state: &OutputState,
    key_name: Option<&str>,
    color: Result<DisplayColor, CapabilityAbsence>,
) -> Option<OutputView> {
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
        color,
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

    /// Feed `description` of `GLOBAL` its events and `done`; whether a listed
    /// colour changed.
    fn describe(outputs: &mut WaylandOutputs, description: u64, events: &[ColorEvent]) -> bool {
        outputs.describe(GLOBAL, description);
        for event in events {
            outputs.apply_color(GLOBAL, description, *event);
        }
        outputs.apply_color(GLOBAL, description, ColorEvent::Done)
    }

    const SDR_SRGB: [ColorEvent; 3] = [
        ColorEvent::NamedPrimaries(Some(ColorGamut::Srgb)),
        ColorEvent::Primaries { red_x: 640_000 },
        ColorEvent::Transfer(TransferRange::Standard),
    ];

    /// An HDR10 output: PQ in BT.2020, 203 cd/m² white, the curve's 10 000
    /// cd/m² ceiling as the primary volume's maximum, and a 1015 cd/m² target.
    const HDR10: [ColorEvent; 5] = [
        ColorEvent::NamedPrimaries(Some(ColorGamut::Rec2020)),
        ColorEvent::Primaries { red_x: 708_000 },
        ColorEvent::Transfer(TransferRange::Pq),
        ColorEvent::Luminances {
            max: 10_000,
            reference: 203,
        },
        ColorEvent::TargetLuminance { max: 1015 },
    ];

    /// Image descriptions to colours: named primaries, the red chromaticity
    /// alone, the transfer function and the headroom from the luminances.
    fn wayland_image_descriptions_map_to_the_display_colour() {
        let colour_of = |events: &[ColorEvent]| {
            let mut outputs = WaylandOutputs::default();
            announce(&mut outputs, GLOBAL, vec![mode(1920, 1080, 60_000)]);
            describe(&mut outputs, 0, events);
            only(&outputs).color
        };
        assert_eq!(
            colour_of(&SDR_SRGB),
            Ok(DisplayColor::standard(ColorGamut::Srgb))
        );
        assert_eq!(
            colour_of(&HDR10),
            Ok(DisplayColor::high(
                ColorGamut::Rec2020,
                Ok(Headroom::try_new(5.0).expect("headroom"))
            )),
            "1015 over 203 cd/m²"
        );
        assert_eq!(
            colour_of(&[
                ColorEvent::NamedPrimaries(None),
                ColorEvent::Primaries { red_x: 680_000 },
                ColorEvent::Transfer(TransferRange::Standard),
            ]),
            Ok(DisplayColor::standard(ColorGamut::DisplayP3)),
            "an unclassed named set falls to the chromaticities"
        );
        assert_eq!(
            colour_of(&HDR10[..4]),
            Ok(DisplayColor::high(
                ColorGamut::Rec2020,
                Err(CapabilityAbsence::NotOnThisDevice)
            )),
            "PQ without a target peak has no headroom: the 10 000 cd/m² maximum is the \
             curve's ceiling, not the output's"
        );
        assert_eq!(
            colour_of(&[
                ColorEvent::Primaries { red_x: 708_000 },
                ColorEvent::Transfer(TransferRange::Extended),
                ColorEvent::Luminances {
                    max: 812,
                    reference: 203,
                },
            ]),
            Ok(DisplayColor::high(
                ColorGamut::Rec2020,
                Ok(Headroom::try_new(4.0).expect("headroom"))
            )),
            "an extended curve without a target peak takes its maximum"
        );
        assert_eq!(
            colour_of(&[
                ColorEvent::Primaries { red_x: 708_000 },
                ColorEvent::Transfer(TransferRange::Pq),
                ColorEvent::Luminances {
                    max: 10_000,
                    reference: 203,
                },
                ColorEvent::TargetLuminance { max: 150 },
            ]),
            Ok(DisplayColor::high(ColorGamut::Rec2020, Ok(Headroom::SDR))),
            "a target peak below the reference white is no headroom"
        );
        assert_eq!(
            colour_of(&[
                ColorEvent::Primaries { red_x: 708_000 },
                ColorEvent::Transfer(TransferRange::Extended),
                ColorEvent::Luminances {
                    max: 1000,
                    reference: 0,
                },
            ]),
            Ok(DisplayColor::high(
                ColorGamut::Rec2020,
                Err(CapabilityAbsence::NotOnThisDevice)
            )),
            "a zero reference white gives no headroom, not an infinite one"
        );
        assert_eq!(
            colour_of(&[ColorEvent::Transfer(TransferRange::Standard)]),
            Err(CapabilityAbsence::NotYetOnThisHost),
            "an ICC-only description is not read"
        );
    }

    /// The compositor's colour follows its image description changes, and an
    /// older description finishing late never replaces a newer one's colour.
    fn a_newer_image_description_wins_over_an_older_one() {
        let mut outputs = WaylandOutputs::default();
        announce(&mut outputs, GLOBAL, vec![mode(1920, 1080, 60_000)]);
        assert_eq!(
            only(&outputs).color,
            Err(CapabilityAbsence::NotOnThisDevice),
            "no colour before any description: a compositor without the protocol"
        );
        assert!(describe(&mut outputs, 0, &SDR_SRGB), "the first colour");

        // HDR goes on: description 1 is asked for, then the output changes
        // back and description 2 replaces it while 1's events still arrive.
        outputs.describe(GLOBAL, 1);
        outputs.describe(GLOBAL, 2);
        for event in HDR10 {
            outputs.apply_color(GLOBAL, 1, event);
        }
        assert!(
            !outputs.apply_color(GLOBAL, 1, ColorEvent::Done),
            "a superseded description's done changes nothing"
        );
        for event in SDR_SRGB {
            outputs.apply_color(GLOBAL, 2, event);
        }
        assert!(
            !outputs.apply_color(GLOBAL, 2, ColorEvent::Done),
            "the newest description is still SDR: no change"
        );
        assert_eq!(
            only(&outputs).color,
            Ok(DisplayColor::standard(ColorGamut::Srgb))
        );

        assert!(describe(&mut outputs, 3, &HDR10), "HDR on is a change");
        assert_eq!(
            only(&outputs)
                .color
                .map(|color| color.color_space.dynamic_range),
            Ok(flui_platform::DynamicRange::High)
        );
        outputs.describe(GLOBAL, 4);
        assert!(
            outputs.color_failed(GLOBAL, 4),
            "a description the compositor cannot give leaves no colour"
        );
        assert!(
            !outputs.color_failed(GLOBAL, 3),
            "a failure of a finished description is stale"
        );
        assert_eq!(
            only(&outputs).color,
            Err(CapabilityAbsence::NotOnThisDevice)
        );
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
                (
                    "wayland_image_descriptions_map_to_the_display_colour",
                    wayland_image_descriptions_map_to_the_display_colour,
                ),
                (
                    "a_newer_image_description_wins_over_an_older_one",
                    a_newer_image_description_wins_over_an_older_one,
                ),
            ],
        );
    }
}
