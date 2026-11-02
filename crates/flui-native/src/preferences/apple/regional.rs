//! The user's regional formats from Foundation's current locale and calendar
//! (ADR-0278 §3), alike on macOS and iOS. A change in Language & Region posts
//! `NSCurrentLocaleDidChangeNotification`, which both hosts observe, and the
//! next sample reads the autoupdating locale and calendar again.

// The locale keys are Foundation's statics.
#![expect(unsafe_code)]

use flui_platform::{Locale, RegionalFormats};
use objc2_foundation::{
    NSCalendar, NSDateFormatter, NSLocale, NSLocaleCountryCode, NSLocaleMeasurementSystem,
    NSString, ns_string,
};

use crate::preferences::regional;

/// What the user set in Language & Region. A value Foundation does not
/// answer is no observation of that field.
pub(crate) fn sample(first_language: Option<&Locale>) -> RegionalFormats {
    let locale = NSLocale::autoupdatingCurrentLocale();
    let mut formats = RegionalFormats::default();
    if let Some(day) =
        regional::sunday_first_weekday(NSCalendar::autoupdatingCurrentCalendar().firstWeekday())
    {
        formats = formats.with_first_weekday(day);
    }
    // The `j` skeleton asks for an hour in the locale's preferred cycle, which
    // the user's 24-hour switch overrides.
    if let Some(cycle) =
        NSDateFormatter::dateFormatFromTemplate_options_locale(ns_string!("j"), 0, Some(&locale))
            .and_then(|pattern| regional::hour_cycle_of_pattern(&pattern.to_string()))
    {
        formats = formats.with_hour_cycle(cycle);
    }
    let string = |key: &NSString| {
        locale
            .objectForKey(key)
            .and_then(|value| value.downcast::<NSString>().ok())
            .map(|value| value.to_string())
    };
    // SAFETY: both keys are Foundation's immutable constants, initialised
    // before any Objective-C code runs.
    let (measurement_key, region_key) = unsafe { (NSLocaleMeasurementSystem, NSLocaleCountryCode) };
    if let Some(measurement) = string(measurement_key)
        .as_deref()
        .and_then(regional::apple_measurement)
    {
        formats = formats.with_measurement(measurement);
    }
    if let Some(numbering) = regional::icu_numbering(&locale.localeIdentifier().to_string()) {
        formats = formats.with_numbering(numbering);
    }
    // The country key, not `regionCode`, which macOS 13 and iOS 16 added.
    let region = string(region_key);
    if let Some(region) = regional::region_override(region.as_deref(), first_language) {
        formats = formats.with_region(region);
    }
    formats
}
