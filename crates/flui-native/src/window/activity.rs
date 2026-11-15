//! What an Activity's window asks of the system when it opens: the colour
//! mode of its surface and which input sources reach it unbuffered. The pure
//! rules of those requests, apart from the JNI calls that make them, so every
//! host's tests run them. What the window honours is `window::capabilities`.

#![forbid(unsafe_code)]

use flui_platform::InputLatency;
use flui_platform::os_options::AndroidColorMode;

use crate::version::gate::android::{AndroidApi, UnbufferedDispatch};

/// The `InputDevice.SOURCE_CLASS_*` mask an Activity's decor view asks for
/// unbuffered dispatch to deliver `latency`: the pointer class for
/// [`InputLatency::Low`], none otherwise, so a window reopened on the same
/// Activity drops an earlier request. `None` below API 30, where the system
/// has no such request and batches every source.
pub(crate) fn unbuffered_input_sources(latency: InputLatency, api: AndroidApi) -> Option<i32> {
    /// `InputDevice.SOURCE_CLASS_NONE`.
    const SOURCE_CLASS_NONE: i32 = 0;
    /// `InputDevice.SOURCE_CLASS_POINTER`: touch, stylus and mouse.
    const SOURCE_CLASS_POINTER: i32 = 0x2;
    api.since::<UnbufferedDispatch>()?;
    Some(match latency {
        InputLatency::Low => SOURCE_CLASS_POINTER,
        // `Balanced`, and a latency the contract adds later, keep the
        // system's per-frame batches.
        _ => SOURCE_CLASS_NONE,
    })
}

/// The `ActivityInfo.COLOR_MODE_*` value an Activity's window asks for to
/// show `mode`; `None` for the default, which the window has without asking.
pub(crate) fn activity_color_mode(mode: AndroidColorMode) -> Option<i32> {
    /// `ActivityInfo.COLOR_MODE_WIDE_COLOR_GAMUT`.
    const COLOR_MODE_WIDE_COLOR_GAMUT: i32 = 1;
    /// `ActivityInfo.COLOR_MODE_HDR`.
    const COLOR_MODE_HDR: i32 = 2;
    match mode {
        AndroidColorMode::WideColorGamut => Some(COLOR_MODE_WIDE_COLOR_GAMUT),
        AndroidColorMode::Hdr => Some(COLOR_MODE_HDR),
        // `Default`, and a mode the contract adds later, ask for nothing.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::InputLatency::{Balanced, Low};

    use super::{AndroidApi, AndroidColorMode, activity_color_mode, unbuffered_input_sources};

    /// A window asks its decor view for unbuffered pointer input when the
    /// application wants low latency, withdraws the request otherwise, and
    /// asks nothing where the system has no such request.
    #[test]
    fn an_activity_asks_for_unbuffered_pointer_input() {
        let api = AndroidApi::from_level;
        let rows = [
            (
                "low, API 30",
                unbuffered_input_sources(Low, api(30)),
                Some(0x2),
            ),
            (
                "low, API 35",
                unbuffered_input_sources(Low, api(35)),
                Some(0x2),
            ),
            (
                "balanced, API 30",
                unbuffered_input_sources(Balanced, api(30)),
                Some(0),
            ),
            ("low, API 29", unbuffered_input_sources(Low, api(29)), None),
            (
                "balanced, API 24",
                unbuffered_input_sources(Balanced, api(24)),
                None,
            ),
            (
                "low, unknown API",
                unbuffered_input_sources(Low, AndroidApi::UNKNOWN),
                None,
            ),
        ];
        let failures: Vec<_> = rows.iter().filter(|(_, got, want)| got != want).collect();
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The Activity window asks for the colour mode the application's surface
    /// is requested in, and asks nothing for the default.
    #[test]
    fn an_activity_asks_for_the_colour_mode_of_its_surface() {
        assert_eq!(activity_color_mode(AndroidColorMode::Default), None);
        assert_eq!(
            activity_color_mode(AndroidColorMode::WideColorGamut),
            Some(1)
        );
        assert_eq!(activity_color_mode(AndroidColorMode::Hdr), Some(2));
    }
}
