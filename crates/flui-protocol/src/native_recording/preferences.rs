//! A read of the system preferences, in the plain fields a backend copies out of the OS.

/// The system's motion preference.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Motion {
    /// No restriction.
    NoPreference,
    /// The user asked for reduced motion.
    Reduce,
    /// Animation durations are multiplied by this finite, positive factor.
    Scaled(f64),
}

/// The system's text-weight request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum TextWeight {
    /// The authored weight, unadjusted.
    NoPreference,
    /// Heavier, more legible text.
    Bold,
    /// A signed, nonzero adjustment to every resolved weight.
    Adjustment(i32),
}

/// What one discrete vertical wheel notch scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum WheelStep {
    /// This many lines; zero disables the axis.
    Lines(u32),
    /// One page.
    Page,
}

/// The native mouse geometry, in device pixels at `pixel_ratio`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MouseGeometry {
    /// The double-click rectangle, width and height.
    pub double_click_area: [i32; 2],
    /// The drag threshold, horizontal and vertical.
    pub drag_tolerance: [i32; 2],
    /// The device pixel ratio the values were sampled at, within `1/32..=32`: the range a
    /// screen can have, and a reader refuses a row outside it.
    pub pixel_ratio: f64,
}

/// The native touch geometry, in device pixels at `pixel_ratio`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TouchGeometry {
    /// The distance a touch travels before it is a drag.
    pub touch_slop: i32,
    /// The distance between two taps of a double tap.
    pub double_tap_slop: i32,
    /// The slowest fling, per second.
    pub min_fling: i32,
    /// The fastest fling, per second.
    pub max_fling: i32,
    /// The device pixel ratio the values were sampled at, within `1/32..=32`: the range a
    /// screen can have, and a reader refuses a row outside it.
    pub pixel_ratio: f64,
}

/// The system's gesture timing and geometry; an absent field was not observed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Gestures {
    /// The mouse double-click interval, in nanoseconds.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub double_click_interval_ns: Option<u64>,
    /// The touch double-tap interval, in nanoseconds.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub double_tap_interval_ns: Option<u64>,
    /// The shortest long press, in nanoseconds.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub long_press_timeout_ns: Option<u64>,
    /// The native mouse geometry.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub native_mouse_geometry: Option<MouseGeometry>,
    /// The native touch geometry.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub native_touch_geometry: Option<TouchGeometry>,
}

/// The user's regional formats, each as the value of its Unicode `u` extension key (UTS #35); an
/// absent field was not observed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Regional {
    /// The first day of the week, as an `fw` value such as `mon`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub first_weekday: Option<String>,
    /// The hour cycle, as an `hc` value such as `h23`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub hour_cycle: Option<String>,
    /// The measurement system, as an `ms` value such as `metric`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub measurement: Option<String>,
    /// The numbering system, as an `nu` value such as `arab`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub numbering: Option<String>,
    /// The region override, as its region code such as `GB`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub region: Option<String>,
}

/// The host's system preferences (ADR-0172); an absent field was not observed.
///
/// The same object is a native `platform` recording's `preferences` row and, beside its
/// `revision`, a host input recording's preferences row, so the two never spell a preference
/// differently.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "schemars",
    derive(schemars::JsonSchema),
    schemars(rename = "NativePreferences")
)]
pub struct Preferences {
    /// The text-size multiplier, in `1/64..=64`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub text_scale: Option<f64>,
    /// The motion preference.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub motion: Option<Motion>,
    /// Whether high contrast is on.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub high_contrast: Option<bool>,
    /// Whether system haptics are on.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub haptics: Option<bool>,
    /// The text-weight request.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub text_weight: Option<TextWeight>,
    /// The preferred UI locales as BCP 47 tags, in priority order.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub locales: Option<Vec<String>>,
    /// The regional formats; absent when none was observed. Boxed, since a preferences read is
    /// carried by value and most hosts observe none.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub regional: Option<Box<Regional>>,
    /// Gesture timing and geometry; always written, empty when nothing was observed.
    #[cfg_attr(feature = "serde", serde(default))]
    pub gestures: Gestures,
    /// What one vertical wheel notch scrolls.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub wheel_vertical: Option<WheelStep>,
    /// The characters one horizontal wheel notch scrolls; zero disables the axis.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub wheel_horizontal_characters: Option<u32>,
    /// The accent colour: red, green, blue and alpha, sRGB-encoded with straight alpha, each
    /// finite and in `0..=1`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub accent_color: Option<[f32; 4]>,
    /// When the system shows a scroll bar.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub scroll_bar_visibility: Option<ScrollBarVisibility>,
    /// What a click in a scroll bar's track does.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub scroll_track_click: Option<ScrollTrackClick>,
}

vocabulary! {
    /// When the system shows a scroll bar.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ScrollBarVisibility {
        /// While the content scrolls, fading once it stops.
        WhenScrolling => "when_scrolling",
        /// Whenever the content is longer than its viewport.
        Always => "always",
    }
}

vocabulary! {
    /// What a click in a scroll bar's track, off the thumb, does.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ScrollTrackClick {
        /// Scroll by a page toward the click.
        Page => "page",
        /// Move the thumb to the spot clicked.
        JumpToSpot => "jump_to_spot",
    }
}

vocabulary! {
    /// Which error a failed read of the system returned, as the platform's error names it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum FailureKind {
        /// An observation was already running or waiting for its retry, so none was available.
        Deferred => "deferred",
        /// The preference source could not complete an observation.
        Preferences => "preferences",
        /// The display source could not complete a sample.
        Displays => "displays",
        /// The host has no source for what was read.
        Unsupported => "unsupported",
        /// The event loop failed.
        EventLoop => "event_loop",
        /// Another error, which only its message describes.
        Other => "other",
    }
}

/// Why a read of the system failed: the error's kind, and its message when it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Failure {
    /// Which error.
    pub kind: FailureKind,
    /// What failed, in the host's words.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub message: Option<String>,
}

/// What one read of the system preferences returned.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum PreferenceRead {
    /// The preferences the system reported.
    Observed(Preferences),
    /// The read failed; a failure is not an observation.
    Failed(Failure),
}
