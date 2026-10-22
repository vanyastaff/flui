//! The X11 output model: RandR 1.5 monitors, with the output, CRTC and mode
//! behind each.
//!
//! The adapter reads the server's whole RandR state each time it changes and
//! hands it over as a [`RandrState`]; [`RandrState::views`] turns it into the
//! displays. The model is pure and knows nothing of the connection.
//!
//! # What a display is
//!
//! - A **monitor** (`RRGetMonitors`) is a display. Its identity is the XID of
//!   its **first** output ([`DisplayKey::X11Output`]); an output XID is stable
//!   for as long as the server runs and across a replug of the same connector.
//!   A monitor with no output (a rectangle made with `xrandr --setmonitor` over
//!   part of a screen) is not listed: it has no key and no mode to read.
//! - **Bounds** are the monitor's rectangle in screen pixels.
//! - **Refresh** is the rate of the mode on the first output's CRTC, computed
//!   as `xrandr` does: `dot_clock / (htotal * vtotal)`, with a double-scanned
//!   mode scanning every line twice and an interlaced one counting fields.
//!   A mode that gives no total is unknown, not a division by zero.
//! - **Scale** is `Xft.dpi / 96`, the value toolkits read; 1 when it is not set.
//! - **Usable bounds** are the monitor's rectangle cut to the window manager's
//!   work area (`_NET_WORKAREA` of the current desktop), the bounds without one.
//! - **Native numbers** are the CRTCs behind the monitor's outputs: a winit
//!   window on X11 names its monitor by its CRTC.

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::DisplayKey;

use super::view::{Identity, OutputView, intersection};
use crate::display::source::Refresh;

/// One RandR mode, with the fields the refresh rate needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RandrMode {
    pub id: u32,
    pub dot_clock: u32,
    pub htotal: u16,
    pub vtotal: u16,
    pub interlace: bool,
    pub double_scan: bool,
}

/// A CRTC and the mode it runs; `mode` is 0 when it is off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RandrCrtc {
    pub id: u32,
    pub mode: u32,
}

/// An output and the CRTC that drives it; `crtc` is 0 when none does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RandrOutput {
    pub id: u32,
    pub crtc: u32,
    /// The connector, such as `HDMI-1`.
    pub name: String,
    /// The panel's EDID base block, when the output has one.
    pub edid: Option<Vec<u8>>,
}

/// One `RRGetMonitors` monitor.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RandrMonitor {
    pub name: String,
    pub is_primary: bool,
    pub bounds: Bounds<i32>,
    /// The XIDs of its outputs, in the order the server lists them.
    pub outputs: Vec<u32>,
}

/// The server's RandR state at one moment.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct RandrState {
    pub monitors: Vec<RandrMonitor>,
    pub outputs: Vec<RandrOutput>,
    pub crtcs: Vec<RandrCrtc>,
    pub modes: Vec<RandrMode>,
    /// `Xft.dpi / 96`; `None` when the server sets no such resource.
    pub scale_factor: Option<f64>,
    /// The current desktop's work area; `None` when the window manager sets none.
    pub work_area: Option<Bounds<i32>>,
}

impl RandrState {
    /// The listed monitors as displays.
    pub(crate) fn views(&self) -> Vec<OutputView> {
        self.monitors
            .iter()
            .filter_map(|monitor| self.view_of(monitor))
            .collect()
    }

    fn view_of(&self, monitor: &RandrMonitor) -> Option<OutputView> {
        let first = *monitor.outputs.first()?;
        let crtcs_of_outputs: Vec<u32> = monitor
            .outputs
            .iter()
            .filter_map(|id| self.outputs.iter().find(|output| output.id == *id))
            .map(|output| output.crtc)
            .filter(|&crtc| crtc != 0)
            .collect();
        let mut natives = Vec::with_capacity(crtcs_of_outputs.len());
        for crtc in crtcs_of_outputs {
            if !natives.contains(&crtc) {
                natives.push(crtc);
            }
        }
        // The rate is the first output's: a monitor spanning several CRTCs
        // (a tiled panel) runs them at one rate, and a cloned one shares its CRTC.
        let refresh_period = natives
            .first()
            .and_then(|crtc| self.crtcs.iter().find(|candidate| candidate.id == *crtc))
            .and_then(|crtc| self.modes.iter().find(|mode| mode.id == crtc.mode))
            .and_then(mode_refresh)
            .and_then(Refresh::period);
        let usable_bounds = self
            .work_area
            .and_then(|area| intersection(monitor.bounds, area))
            .unwrap_or(monitor.bounds);
        let first_output = self.outputs.iter().find(|output| output.id == first);
        let persistent_key =
            first_output.and_then(|output| persistent_key(&output.name, output.edid.as_deref()));
        Some(OutputView {
            identity: Identity::Keyed(DisplayKey::X11Output(first)),
            natives,
            name: monitor.name.clone(),
            bounds: monitor.bounds,
            usable_bounds,
            scale_factor: self.scale_factor.unwrap_or(1.0),
            refresh_period,
            is_primary: monitor.is_primary,
            persistent_key,
        })
    }
}

/// What names a monitor again after a restart: its connector, and the panel on
/// it when the EDID says which (`HDMI-1|DEL4115-0000002A`: manufacturer,
/// product code and serial number). Two identical panels differ by connector,
/// and one panel moved to another connector is another display. `None` for an
/// output with no name.
pub(crate) fn persistent_key(connector: &str, edid: Option<&[u8]>) -> Option<String> {
    if connector.is_empty() {
        return None;
    }
    Some(match edid.and_then(panel_identity) {
        Some(panel) => format!("{connector}|{panel}"),
        None => connector.to_owned(),
    })
}

/// The manufacturer, product code and serial number of an EDID base block, as
/// `DEL4115-0000002A`. `None` for a block that is too short or lacks the fixed
/// header, and for a manufacturer code that is not three letters.
fn panel_identity(edid: &[u8]) -> Option<String> {
    const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
    let header = edid.get(..8)?;
    if header != HEADER {
        return None;
    }
    let manufacturer = u16::from_be_bytes([*edid.get(8)?, *edid.get(9)?]);
    let product = u16::from_le_bytes([*edid.get(10)?, *edid.get(11)?]);
    let serial = u32::from_le_bytes([
        *edid.get(12)?,
        *edid.get(13)?,
        *edid.get(14)?,
        *edid.get(15)?,
    ]);
    // Three 5-bit letters, 1 for `A` to 26 for `Z`, under a clear top bit.
    if manufacturer & 0x8000 != 0 {
        return None;
    }
    let mut letters = String::with_capacity(3);
    for shift in [10, 5, 0] {
        let code = u8::try_from((manufacturer >> shift) & 0x1F).ok()?;
        if !(1..=26).contains(&code) {
            return None;
        }
        letters.push(char::from(b'A' + code - 1));
    }
    Some(format!("{letters}{product:04X}-{serial:08X}"))
}

/// The refresh rate of `mode` in millihertz, or `None` when its totals are zero.
///
/// `xrandr` prints `dot_clock / (htotal * vtotal)` with `vtotal` doubled for a
/// double-scanned mode and halved for an interlaced one (a field is the unit
/// of an interlaced mode's refresh). The halving is folded into the numerator
/// so no precision is lost, and every product is taken in 128 bits: the dot
/// clock alone is a 32-bit hertz value, and `htotal * vtotal * 2` already
/// passes 32 bits.
fn mode_refresh_millihertz(mode: &RandrMode) -> Option<u128> {
    if mode.htotal == 0 || mode.vtotal == 0 {
        return None;
    }
    let mut numerator = u128::from(mode.dot_clock) * 1000;
    let mut denominator = u128::from(mode.htotal) * u128::from(mode.vtotal);
    if mode.double_scan {
        denominator *= 2;
    }
    if mode.interlace {
        numerator *= 2;
    }
    // Rounded to the nearest millihertz.
    Some((numerator + denominator / 2) / denominator)
}

/// `mode`'s refresh, which `Refresh::period` makes a period of; `None` for a
/// mode whose totals give no rate.
fn mode_refresh(mode: &RandrMode) -> Option<Refresh> {
    Some(Refresh::Millihertz(
        i64::try_from(mode_refresh_millihertz(mode)?).ok()?,
    ))
}

/// `Xft.dpi / 96` from the text of the root window's `RESOURCE_MANAGER`
/// property, or `None` when it sets no usable dpi.
pub(crate) fn xft_scale_factor(resources: &str) -> Option<f64> {
    let dpi: f64 = resources
        .lines()
        .find_map(|line| line.trim().strip_prefix("Xft.dpi:"))?
        .trim()
        .parse()
        .ok()?;
    (dpi.is_finite() && dpi > 0.0).then(|| dpi / 96.0)
}

/// The work area of the desktop `current` out of `_NET_WORKAREA`, a flat list
/// of `x, y, width, height` per desktop. A window manager that sets fewer
/// desktops than the current index names the first.
pub(crate) fn current_work_area(values: &[i32], current: Option<u32>) -> Option<Bounds<i32>> {
    let desktops = values.len() / 4;
    if desktops == 0 {
        return None;
    }
    let index = current
        .and_then(|current| usize::try_from(current).ok())
        .filter(|&index| index < desktops)
        .unwrap_or(0);
    let area = &values[index * 4..index * 4 + 4];
    Some(Bounds::new(
        Point::new(area[0], area[1]),
        Size::new(area[2], area[3]),
    ))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::view::{FreshIds, display_of_native_number};
    use super::*;
    use flui_platform::{DisplayRegistry, RefreshPeriod};

    /// The period of `mode`'s refresh, as a monitor on it publishes it.
    fn period_of_mode(mode: &RandrMode) -> Option<RefreshPeriod> {
        mode_refresh(mode).and_then(Refresh::period)
    }

    fn mode(id: u32, dot_clock: u32, htotal: u16, vtotal: u16) -> RandrMode {
        RandrMode {
            id,
            dot_clock,
            htotal,
            vtotal,
            interlace: false,
            double_scan: false,
        }
    }

    fn period_micros(mode: &RandrMode) -> Option<u128> {
        period_of_mode(mode).map(|period| period.get().as_micros())
    }

    /// 1920x1080 at 60 Hz (CEA 1080p60): 148.5 MHz over 2200 x 1125.
    fn p1080() -> RandrMode {
        mode(10, 148_500_000, 2200, 1125)
    }

    fn monitor(name: &str, outputs: &[u32], x: i32) -> RandrMonitor {
        RandrMonitor {
            name: name.into(),
            is_primary: false,
            bounds: Bounds::new(Point::new(x, 0), Size::new(1920, 1080)),
            outputs: outputs.to_vec(),
        }
    }

    fn state(monitors: Vec<RandrMonitor>) -> RandrState {
        // Output n is driven by CRTC n + 100, which runs 1080p60.
        let outputs = monitors
            .iter()
            .flat_map(|monitor| monitor.outputs.iter().copied())
            .map(|id| RandrOutput {
                id,
                crtc: id + 100,
                name: format!("OUT-{id}"),
                edid: None,
            })
            .collect::<Vec<_>>();
        let crtcs = outputs
            .iter()
            .map(|output| RandrCrtc {
                id: output.crtc,
                mode: 10,
            })
            .collect();
        RandrState {
            monitors,
            outputs,
            crtcs,
            modes: vec![p1080()],
            scale_factor: None,
            work_area: None,
        }
    }

    fn x11_refresh_is_dot_clock_over_totals() {
        // Exactly 60 Hz.
        assert_eq!(mode_refresh_millihertz(&p1080()), Some(60_000));
        assert_eq!(period_micros(&p1080()), Some(16_666));
        // 59.94 Hz: the 1000/1001 television rate, 148.5 MHz / 1.001.
        let ntsc = mode(11, 148_351_648, 2200, 1125);
        assert_eq!(mode_refresh_millihertz(&ntsc), Some(59_940));
        // 144 Hz at 2560x1440: 587.52 MHz over 2720 x 1500.
        let fast = mode(12, 587_520_000, 2720, 1500);
        assert_eq!(mode_refresh_millihertz(&fast), Some(144_000));
        assert_eq!(period_micros(&fast), Some(6_944));
        // A clock that is not a whole multiple of the totals rounds to the
        // nearest millihertz: 148.5 MHz over 2200 x 1126 is 59.94671 Hz.
        let off = mode(13, 148_500_000, 2200, 1126);
        assert_eq!(mode_refresh_millihertz(&off), Some(59_947));
    }

    fn x11_double_scan_and_interlace_adjust_the_rate() {
        // 640x400 doublescan: every line is scanned twice, so the rate is
        // half of what the totals alone give.
        let plain = mode(1, 25_175_000, 800, 449);
        let doubled = RandrMode {
            double_scan: true,
            ..plain
        };
        assert_eq!(mode_refresh_millihertz(&plain), Some(70_086));
        assert_eq!(mode_refresh_millihertz(&doubled), Some(35_043));
        // 1080i: 74.25 MHz over 2200 x 1125 is 30 frames, 60 fields.
        let progressive = mode(2, 74_250_000, 2200, 1125);
        let interlaced = RandrMode {
            interlace: true,
            ..progressive
        };
        assert_eq!(mode_refresh_millihertz(&progressive), Some(30_000));
        assert_eq!(mode_refresh_millihertz(&interlaced), Some(60_000));
        // Both flags cancel.
        let both = RandrMode {
            interlace: true,
            double_scan: true,
            ..progressive
        };
        assert_eq!(mode_refresh_millihertz(&both), Some(30_000));
    }

    fn x11_zero_htotal_or_vtotal_is_unknown() {
        for (htotal, vtotal) in [(0, 1125), (2200, 0), (0, 0)] {
            let broken = mode(3, 148_500_000, htotal, vtotal);
            assert_eq!(mode_refresh_millihertz(&broken), None);
            assert_eq!(period_of_mode(&broken), None);
        }
        // The same through a monitor: the display is listed, its rate unknown.
        let mut broken = state(vec![monitor("DP-1", &[1], 0)]);
        broken.modes[0].htotal = 0;
        let views = broken.views();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].refresh_period, None);
    }

    fn x11_big_dot_clock_does_not_overflow() {
        // The largest dot clock with the smallest totals: no 32-bit product
        // of these holds, and the rate is far past 1000 Hz.
        let huge = mode(4, u32::MAX, 1, 1);
        assert_eq!(
            mode_refresh_millihertz(&huge),
            Some(u128::from(u32::MAX) * 1000)
        );
        assert_eq!(period_of_mode(&huge), None, "beyond 1000 Hz");
        // The largest totals with a double scan: 65535 * 65535 * 2 passes u32.
        let slow = RandrMode {
            double_scan: true,
            ..mode(5, u32::MAX, u16::MAX, u16::MAX)
        };
        assert_eq!(mode_refresh_millihertz(&slow), Some(500));
        assert_eq!(period_of_mode(&slow), None, "below 1 Hz");
        // A large clock that is a real rate: 4.2948 GHz over 10000 x 7158 is
        // 60 Hz, and its numerator (4.29e12) passes u32 by a thousandfold.
        let wide = mode(6, 4_294_800_000, 10_000, 7_158);
        assert_eq!(mode_refresh_millihertz(&wide), Some(60_000));
        assert_eq!(
            period_of_mode(&wide).map(RefreshPeriod::get),
            Some(Duration::from_nanos(16_666_667))
        );
    }

    fn x11_monitor_is_keyed_by_its_first_output() {
        let registry = DisplayRegistry::new();
        let mut fresh = FreshIds::default();
        let mut resolve =
            |state: &RandrState| fresh.resolve(&state.views(), |key| registry.mint(key).ok());

        // A cloned pair: one monitor, two outputs.
        let cloned = resolve(&state(vec![monitor("Clone", &[20, 10], 0)]));
        assert_eq!(cloned.displays.len(), 1);
        let views = state(vec![monitor("Clone", &[20, 10], 0)]).views();
        assert_eq!(
            views[0].identity,
            Identity::Keyed(DisplayKey::X11Output(20)),
            "the first output, not the lowest"
        );

        // The second output leaves: the monitor is still the one keyed by 20.
        let alone = resolve(&state(vec![monitor("Clone", &[20], 0)]));
        assert_eq!(cloned.displays[0].id, alone.displays[0].id);

        // The monitor's name is a label: renaming it changes no identity, and
        // a monitor of another output is another display.
        let renamed = resolve(&state(vec![
            monitor("Renamed", &[20], 0),
            monitor("Renamed", &[30], 1920),
        ]));
        assert_eq!(renamed.displays[0].id, alone.displays[0].id);
        assert_ne!(renamed.displays[0].id, renamed.displays[1].id);
    }

    /// An EDID base block for a Dell (`DEL`) panel, product `0x4115`, serial
    /// `0x2A`.
    fn edid() -> Vec<u8> {
        let mut block = vec![0_u8; 128];
        block[..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
        block[8..10].copy_from_slice(&[0x10, 0xAC]);
        block[10..12].copy_from_slice(&[0x15, 0x41]);
        block[12..16].copy_from_slice(&[0x2A, 0x00, 0x00, 0x00]);
        block
    }

    /// A case, the connector, its EDID and the key.
    type KeyCase = (
        &'static str,
        &'static str,
        Option<Vec<u8>>,
        Option<&'static str>,
    );

    fn x11_persistent_key_is_the_connector_and_the_panel() {
        // (case, connector, EDID, key)
        let mut wrong_header = edid();
        wrong_header[1] = 0;
        let mut zero_letter = edid();
        zero_letter[8..10].copy_from_slice(&[0x00, 0x00]);
        let mut past_z = edid();
        // Letter codes 27 to 31 are not letters.
        past_z[8..10].copy_from_slice(&[0x04, 0x3B]);
        let table: &[KeyCase] = &[
            (
                "a panel on a connector",
                "HDMI-1",
                Some(edid()),
                Some("HDMI-1|DEL4115-0000002A"),
            ),
            ("no EDID", "DP-2", None, Some("DP-2")),
            (
                "EDID too short",
                "DP-2",
                Some(edid()[..15].to_vec()),
                Some("DP-2"),
            ),
            (
                "EDID without its header",
                "DP-2",
                Some(wrong_header),
                Some("DP-2"),
            ),
            (
                "manufacturer letter zero",
                "DP-2",
                Some(zero_letter),
                Some("DP-2"),
            ),
            (
                "manufacturer letter past Z",
                "DP-2",
                Some(past_z),
                Some("DP-2"),
            ),
            ("no connector name", "", Some(edid()), None),
        ];
        for (case, connector, edid, expected) in table {
            assert_eq!(
                persistent_key(connector, edid.as_deref()).as_deref(),
                *expected,
                "{case}"
            );
        }

        // The monitor is keyed by its first output; its key is that output's.
        let mut state = state(vec![monitor("Clone", &[20, 10], 0)]);
        state.outputs[0].name = "HDMI-1".into();
        state.outputs[0].edid = Some(edid());
        state.outputs[1].name = "DP-1".into();
        assert_eq!(
            state.views()[0].persistent_key.as_deref(),
            Some("HDMI-1|DEL4115-0000002A")
        );
    }

    fn x11_screen_change_replaces_outputs() {
        let registry = DisplayRegistry::new();
        let mut fresh = FreshIds::default();
        let mut resolve =
            |state: &RandrState| fresh.resolve(&state.views(), |key| registry.mint(key).ok());
        let before = resolve(&state(vec![
            monitor("DP-1", &[1], 0),
            monitor("HDMI-1", &[2], 1920),
        ]));
        assert_eq!(before.displays.len(), 2);
        // HDMI-1 is unplugged.
        let after = resolve(&state(vec![monitor("DP-1", &[1], 0)]));
        assert_eq!(after.displays.len(), 1);
        assert_eq!(after.displays[0].id, before.displays[0].id);
        assert_eq!(after.natives[0].natives, vec![101]);
        // And plugged in again: the same connector, the same display.
        let again = resolve(&state(vec![
            monitor("DP-1", &[1], 0),
            monitor("HDMI-1", &[2], 1920),
        ]));
        assert_eq!(again.displays[1].id, before.displays[1].id);
    }

    fn winit_window_matches_its_output_by_native_id() {
        // Two monitors with one rectangle (mirrored by two CRTCs) and a tiled
        // one spanning two CRTCs: a window names its monitor by CRTC alone.
        let registry = DisplayRegistry::new();
        let mut fresh = FreshIds::default();
        let mirrored = state(vec![
            monitor("DP-1", &[1], 0),
            monitor("HDMI-1", &[2], 0),
            monitor("Tiled", &[3, 4], 1920),
        ]);
        let resolved = fresh.resolve(&mirrored.views(), |key| registry.mint(key).ok());
        assert_eq!(resolved.displays[0].bounds, resolved.displays[1].bounds);
        let lookup = |native| display_of_native_number(&resolved.natives, native);
        assert_eq!(lookup(101), Some(resolved.displays[0].id));
        assert_eq!(lookup(102), Some(resolved.displays[1].id));
        assert_eq!(lookup(103), Some(resolved.displays[2].id));
        assert_eq!(lookup(104), Some(resolved.displays[2].id));
        assert_eq!(lookup(999), None, "a CRTC no listed monitor uses");
    }

    fn scale_and_work_area_come_from_the_server() {
        let mut randr = state(vec![monitor("DP-1", &[1], 0)]);
        assert_eq!(randr.views()[0].scale_factor, 1.0);
        randr.scale_factor = xft_scale_factor("*.background: white\nXft.dpi:\t144\n");
        randr.work_area = current_work_area(&[0, 28, 1920, 1052, 0, 0, 1920, 1080], Some(0));
        let view = &randr.views()[0];
        assert_eq!(view.scale_factor, 1.5);
        assert_eq!(view.usable_bounds.origin.y, 28);
        assert_eq!(view.usable_bounds.size.height, 1052);
        assert_eq!(view.bounds.size.height, 1080);
        // The second desktop's area, and an index past the list falls back.
        let areas = [0, 0, 100, 100, 5, 5, 200, 200];
        assert_eq!(
            current_work_area(&areas, Some(1)).map(|a| a.origin.x),
            Some(5)
        );
        assert_eq!(
            current_work_area(&areas, Some(9)).map(|a| a.origin.x),
            Some(0)
        );
        assert_eq!(current_work_area(&areas[..3], Some(0)), None);
        // A work area that misses the monitor leaves the bounds.
        randr.work_area = Some(Bounds::new(Point::new(5000, 0), Size::new(10, 10)));
        assert_eq!(randr.views()[0].usable_bounds, randr.views()[0].bounds);
    }

    fn xft_dpi_parsing() {
        assert_eq!(xft_scale_factor("Xft.dpi: 96"), Some(1.0));
        assert_eq!(xft_scale_factor("Xft.dpi: 192\n"), Some(2.0));
        assert_eq!(xft_scale_factor("Xft.dpi: 0"), None);
        assert_eq!(xft_scale_factor("Xft.dpi: lots"), None);
        assert_eq!(xft_scale_factor("Xft.antialias: 1"), None);
        assert_eq!(xft_scale_factor("Xft.dpi: -96"), None);
    }

    #[test]
    fn x11_output_model() {
        crate::table_test::run_table(
            "x11_output_model",
            &[
                (
                    "x11_refresh_is_dot_clock_over_totals",
                    x11_refresh_is_dot_clock_over_totals as fn(),
                ),
                (
                    "x11_double_scan_and_interlace_adjust_the_rate",
                    x11_double_scan_and_interlace_adjust_the_rate,
                ),
                (
                    "x11_zero_htotal_or_vtotal_is_unknown",
                    x11_zero_htotal_or_vtotal_is_unknown,
                ),
                (
                    "x11_big_dot_clock_does_not_overflow",
                    x11_big_dot_clock_does_not_overflow,
                ),
                (
                    "x11_monitor_is_keyed_by_its_first_output",
                    x11_monitor_is_keyed_by_its_first_output,
                ),
                (
                    "x11_persistent_key_is_the_connector_and_the_panel",
                    x11_persistent_key_is_the_connector_and_the_panel,
                ),
                (
                    "x11_screen_change_replaces_outputs",
                    x11_screen_change_replaces_outputs,
                ),
                (
                    "winit_window_matches_its_output_by_native_id",
                    winit_window_matches_its_output_by_native_id,
                ),
                (
                    "scale_and_work_area_come_from_the_server",
                    scale_and_work_area_come_from_the_server,
                ),
                ("xft_dpi_parsing", xft_dpi_parsing),
            ],
        );
    }
}
