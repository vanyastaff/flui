//! A preferences read's wire form: every field of a snapshot through
//! `PreferenceParts`, each checked on the way back as its builder checks it.

use std::sync::Arc;
use std::time::Duration;

use flui_foundation::geometry::Size;
use flui_platform::{
    GestureParts, HourCycle, InvalidPreference, Locale, MeasurementSystem, MotionPreference,
    NativeMouseGeometry, NativeTouchGeometry, NumberingSystem, PreferenceParts, RegionOverride,
    RegionalParts, ScrollBarParts, ScrollBarVisibility, ScrollTrackClick, SrgbColor,
    SystemPreferences, TextWeightPreference, Weekday, WheelParts, WheelStep,
};
use flui_protocol::native_recording as wire;

use super::{DtoError, DtoResult, duration_nanos, invalid, ratio, unread};

/// `values` in the schema, through [`PreferenceParts`]: a field added there
/// fails to compile here until it is mirrored.
pub(super) fn preferences_to_wire(values: SystemPreferences) -> DtoResult<wire::Preferences> {
    let PreferenceParts {
        text_scale,
        motion,
        high_contrast,
        haptics,
        text_weight,
        locales,
        regional,
        gestures:
            GestureParts {
                double_click_interval,
                double_tap_interval,
                long_press_timeout,
                native_mouse_geometry,
                native_touch_geometry,
            },
        wheel: WheelParts {
            vertical,
            horizontal_characters,
        },
        accent_color,
        scroll_bars: ScrollBarParts {
            visibility,
            track_click,
        },
    } = values.into_parts();
    Ok(wire::Preferences {
        accent_color: accent_color.map(SrgbColor::to_array),
        scroll_bar_visibility: visibility
            .map(|visibility| {
                Ok(match visibility {
                    ScrollBarVisibility::WhenScrolling => wire::ScrollBarVisibility::WhenScrolling,
                    ScrollBarVisibility::Always => wire::ScrollBarVisibility::Always,
                    _ => return Err(DtoError::Unmirrored("ScrollBarVisibility")),
                })
            })
            .transpose()?,
        scroll_track_click: track_click
            .map(|click| {
                Ok(match click {
                    ScrollTrackClick::Page => wire::ScrollTrackClick::Page,
                    ScrollTrackClick::JumpToSpot => wire::ScrollTrackClick::JumpToSpot,
                    _ => return Err(DtoError::Unmirrored("ScrollTrackClick")),
                })
            })
            .transpose()?,
        text_scale,
        motion: motion
            .map(|motion| {
                Ok(match motion {
                    MotionPreference::NoPreference => wire::Motion::NoPreference,
                    MotionPreference::Reduce => wire::Motion::Reduce,
                    MotionPreference::Scaled(scale) => wire::Motion::Scaled(scale.get()),
                    _ => return Err(DtoError::Unmirrored("MotionPreference")),
                })
            })
            .transpose()?,
        high_contrast,
        haptics,
        text_weight: text_weight
            .map(|weight| {
                Ok(match weight {
                    TextWeightPreference::NoPreference => wire::TextWeight::NoPreference,
                    TextWeightPreference::Bold => wire::TextWeight::Bold,
                    TextWeightPreference::Adjustment(value) => {
                        wire::TextWeight::Adjustment(value.get())
                    }
                    _ => return Err(DtoError::Unmirrored("TextWeightPreference")),
                })
            })
            .transpose()?,
        locales: locales.map(|locales| locales.iter().map(Locale::to_language_tag).collect()),
        regional: regional_to_wire(regional),
        gestures: wire::Gestures {
            double_click_interval_ns: double_click_interval.map(duration_nanos).transpose()?,
            double_tap_interval_ns: double_tap_interval.map(duration_nanos).transpose()?,
            long_press_timeout_ns: long_press_timeout.map(duration_nanos).transpose()?,
            native_mouse_geometry: native_mouse_geometry.map(|geometry| {
                let area = geometry.double_click_area();
                let tolerance = geometry.drag_tolerance();
                wire::MouseGeometry {
                    double_click_area: [area.width, area.height],
                    drag_tolerance: [tolerance.width, tolerance.height],
                    pixel_ratio: geometry.pixel_ratio().get(),
                }
            }),
            native_touch_geometry: native_touch_geometry.map(|geometry| wire::TouchGeometry {
                touch_slop: geometry.touch_slop(),
                double_tap_slop: geometry.double_tap_slop(),
                min_fling: geometry.min_fling(),
                max_fling: geometry.max_fling(),
                pixel_ratio: geometry.pixel_ratio().get(),
            }),
        },
        wheel_vertical: vertical
            .map(|step| {
                Ok(match step {
                    WheelStep::Lines(lines) => wire::WheelStep::Lines(lines),
                    WheelStep::Page => wire::WheelStep::Page,
                    _ => return Err(DtoError::Unmirrored("WheelStep")),
                })
            })
            .transpose()?,
        wheel_horizontal_characters: horizontal_characters,
    })
}

/// `parts` in the schema, each format as its Unicode value; none when no
/// format was observed.
fn regional_to_wire(parts: RegionalParts) -> Option<Box<wire::Regional>> {
    if parts == RegionalParts::default() {
        return None;
    }
    let RegionalParts {
        first_weekday,
        hour_cycle,
        measurement,
        numbering,
        region,
    } = parts;
    Some(Box::new(wire::Regional {
        first_weekday: first_weekday.map(|day| day.unicode_value().to_owned()),
        hour_cycle: hour_cycle.map(|cycle| cycle.unicode_value().to_owned()),
        measurement: measurement.map(|system| system.unicode_value().to_owned()),
        numbering: numbering.map(|numbering| numbering.unicode_value().to_owned()),
        region: region.map(|region| region.code().to_owned()),
    }))
}

/// The formats `wire` names, each checked as its type checks it.
fn regional_from_wire(wire: Option<Box<wire::Regional>>) -> DtoResult<RegionalParts> {
    let Some(wire) = wire else {
        return Ok(RegionalParts::default());
    };
    let wire::Regional {
        first_weekday,
        hour_cycle,
        measurement,
        numbering,
        region,
    } = *wire;
    Ok(RegionalParts {
        first_weekday: first_weekday
            .map(|value| {
                Weekday::from_unicode_value(&value)
                    .ok_or_else(|| invalid(format!("first weekday {value}")))
            })
            .transpose()?,
        hour_cycle: hour_cycle
            .map(|value| {
                HourCycle::from_unicode_value(&value)
                    .ok_or_else(|| invalid(format!("hour cycle {value}")))
            })
            .transpose()?,
        measurement: measurement
            .map(|value| {
                MeasurementSystem::from_unicode_value(&value)
                    .ok_or_else(|| invalid(format!("measurement system {value}")))
            })
            .transpose()?,
        numbering: numbering
            .map(|value| {
                NumberingSystem::try_new(&value).map_err(|error| invalid(error.to_string()))
            })
            .transpose()?,
        region: region
            .map(|value| {
                RegionOverride::try_new(&value).map_err(|error| invalid(error.to_string()))
            })
            .transpose()?,
    })
}
/// The preferences `wire` describes, validated as the `with_*` methods
/// validate them.
pub(super) fn preferences_from_wire(wire: wire::Preferences) -> DtoResult<SystemPreferences> {
    let wire::Preferences {
        text_scale,
        motion,
        high_contrast,
        haptics,
        text_weight,
        locales,
        regional,
        gestures,
        wheel_vertical,
        wheel_horizontal_characters,
        accent_color,
        scroll_bar_visibility,
        scroll_track_click,
    } = wire;
    let rejected = |error: InvalidPreference| invalid(error.to_string());
    PreferenceParts {
        accent_color: accent_color
            .map(|[red, green, blue, alpha]| {
                SrgbColor::try_new(
                    f64::from(red),
                    f64::from(green),
                    f64::from(blue),
                    f64::from(alpha),
                )
                .map_err(rejected)
            })
            .transpose()?,
        scroll_bars: ScrollBarParts {
            visibility: scroll_bar_visibility
                .map(|visibility| {
                    Ok(match visibility {
                        wire::ScrollBarVisibility::WhenScrolling => {
                            ScrollBarVisibility::WhenScrolling
                        }
                        wire::ScrollBarVisibility::Always => ScrollBarVisibility::Always,
                        _ => return Err(unread("ScrollBarVisibility")),
                    })
                })
                .transpose()?,
            track_click: scroll_track_click
                .map(|click| {
                    Ok(match click {
                        wire::ScrollTrackClick::Page => ScrollTrackClick::Page,
                        wire::ScrollTrackClick::JumpToSpot => ScrollTrackClick::JumpToSpot,
                        _ => return Err(unread("ScrollTrackClick")),
                    })
                })
                .transpose()?,
        },
        text_scale,
        motion: motion
            .map(|motion| {
                Ok(match motion {
                    wire::Motion::NoPreference => MotionPreference::NoPreference,
                    wire::Motion::Reduce => MotionPreference::Reduce,
                    wire::Motion::Scaled(scale) => {
                        MotionPreference::from_duration_scale(scale).map_err(rejected)?
                    }
                    _ => return Err(unread("Motion")),
                })
            })
            .transpose()?,
        high_contrast,
        haptics,
        text_weight: text_weight
            .map(|weight| {
                Ok(match weight {
                    wire::TextWeight::NoPreference => TextWeightPreference::NoPreference,
                    wire::TextWeight::Bold => TextWeightPreference::Bold,
                    wire::TextWeight::Adjustment(value) => {
                        TextWeightPreference::from_adjustment(value)
                    }
                    _ => return Err(unread("TextWeight")),
                })
            })
            .transpose()?,
        locales: locales
            .map(|tags| {
                tags.iter()
                    .map(|tag| {
                        Locale::from_language_tag(tag)
                            .ok_or_else(|| invalid(format!("locale {tag}")))
                    })
                    .collect::<DtoResult<Arc<[Locale]>>>()
            })
            .transpose()?,
        regional: regional_from_wire(regional)?,
        gestures: GestureParts {
            double_click_interval: gestures.double_click_interval_ns.map(Duration::from_nanos),
            double_tap_interval: gestures.double_tap_interval_ns.map(Duration::from_nanos),
            long_press_timeout: gestures.long_press_timeout_ns.map(Duration::from_nanos),
            native_mouse_geometry: gestures
                .native_mouse_geometry
                .map(|geometry| {
                    let [area_w, area_h] = geometry.double_click_area;
                    let [drag_w, drag_h] = geometry.drag_tolerance;
                    NativeMouseGeometry::try_new(
                        Size::new(area_w, area_h),
                        Size::new(drag_w, drag_h),
                        ratio(geometry.pixel_ratio)?,
                    )
                    .map_err(rejected)
                })
                .transpose()?,
            native_touch_geometry: gestures
                .native_touch_geometry
                .map(|geometry| {
                    NativeTouchGeometry::try_new(
                        geometry.touch_slop,
                        geometry.double_tap_slop,
                        geometry.min_fling,
                        geometry.max_fling,
                        ratio(geometry.pixel_ratio)?,
                    )
                    .map_err(rejected)
                })
                .transpose()?,
        },
        wheel: WheelParts {
            vertical: wheel_vertical
                .map(|step| {
                    Ok(match step {
                        wire::WheelStep::Lines(lines) => WheelStep::Lines(lines),
                        wire::WheelStep::Page => WheelStep::Page,
                        _ => return Err(unread("WheelStep")),
                    })
                })
                .transpose()?,
            horizontal_characters: wheel_horizontal_characters,
        },
    }
    .try_into_preferences()
    .map_err(rejected)
}
