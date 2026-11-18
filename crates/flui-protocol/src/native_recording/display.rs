//! A read of the displays.

use super::Failure;

vocabulary! {
    /// Why the platform reports no value for a capability.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Absence {
        /// The platform has no such capability.
        NotOnThisPlatform => "not_on_this_platform",
        /// The platform has it, this device does not.
        NotOnThisDevice => "not_on_this_device",
        /// The platform has it, the host does not read it yet.
        NotYetOnThisHost => "not_yet_on_this_host",
        /// The window withdrew its capabilities: it can publish no more of
        /// what it honours.
        Withdrawn => "withdrawn",
    }
}

vocabulary! {
    /// The primaries a display shows surfaces in.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Gamut {
        /// sRGB.
        Srgb => "srgb",
        /// Display P3.
        DisplayP3 => "display_p3",
        /// Rec. 2020.
        Rec2020 => "rec2020",
    }
}

vocabulary! {
    /// The range a display shows surfaces in.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum DynamicRange {
        /// Standard dynamic range.
        Standard => "standard",
        /// High dynamic range.
        High => "high",
    }
}

/// How far above SDR white a display can drive a surface, or why the platform does not say.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Headroom {
    /// A finite multiple of SDR white, at least 1.
    Multiple(f32),
    /// The platform gives none.
    Absent(Absence),
}

/// A display's colour: its colour space and headroom.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Color {
    /// The primaries.
    pub gamut: Gamut,
    /// The range.
    pub dynamic_range: DynamicRange,
    /// The headroom over SDR white.
    pub headroom: Headroom,
}

/// A display's colour, or why the platform gives none.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum DisplayColor {
    /// The colour the platform reports.
    Known(Color),
    /// The platform gives none.
    Absent(Absence),
}

/// One display as a read of the system reported it.
///
/// `label` is the display's id as its platform issued it: it names the display within this
/// recording and nowhere else, so a replay issues its own id for each label.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Display {
    /// The display within this recording.
    pub label: u64,
    /// Its name, a label, never an identity.
    pub name: String,
    /// Where its top-left corner sits in the window system's desktop, in that window system's
    /// own units: x, y. It orders the displays and is never a logical position.
    pub origin: [i32; 2],
    /// Its extent in its own logical pixels: width, height.
    pub extent: [f64; 2],
    /// The area windows can occupy, in its own logical pixels from its top-left corner: left,
    /// top, right, bottom.
    pub usable: [f64; 4],
    /// Device pixels per logical pixel, within `1/32..=32`: the range a screen can
    /// have, and a reader refuses a row outside it.
    pub scale_factor: f64,
    /// The period of its mode, in nanoseconds, when known.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub refresh_period_ns: Option<u64>,
    /// Whether it is the primary display.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub primary: bool,
    /// The string that names it again after a restart, when the platform has one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub persistent_key: Option<String>,
    /// Its colour.
    pub color: DisplayColor,
}

/// What one read of the displays returned.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum DisplayRead {
    /// The displays the system reported, at the platform's snapshot revision.
    Observed {
        /// The snapshot's revision, as the platform counted it.
        revision: u64,
        /// The displays.
        displays: Vec<Display>,
    },
    /// The read failed.
    Failed(Failure),
}
