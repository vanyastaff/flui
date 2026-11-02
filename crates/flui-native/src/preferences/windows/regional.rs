//! The user's regional formats from the user default locale's information
//! (ADR-0278 §3). A `WM_SETTINGCHANGE` with `"intl"` marks the preferences
//! stale with every other setting change, so the next sample reads them again.

use flui_platform::{Locale, RegionalFormats};
use windows::Win32::Globalization::{
    GetLocaleInfoEx, LOCALE_IDIGITSUBSTITUTION, LOCALE_IFIRSTDAYOFWEEK, LOCALE_IMEASURE,
    LOCALE_SISO3166CTRYNAME, LOCALE_SNATIVEDIGITS, LOCALE_STIMEFORMAT,
};
use windows::core::PCWSTR;

use crate::preferences::regional;

/// Every value read here is far shorter: a time pattern is at most 80
/// characters.
const VALUE_UNITS: usize = 128;

/// What the user set in Region settings. A value the system does not answer
/// is no observation of that field, never a failed sample.
pub(super) fn sample(first_language: Option<&Locale>) -> RegionalFormats {
    let mut formats = RegionalFormats::default();
    if let Some(day) = info(LOCALE_IFIRSTDAYOFWEEK)
        .as_deref()
        .and_then(regional::windows_first_weekday)
    {
        formats = formats.with_first_weekday(day);
    }
    if let Some(cycle) = info(LOCALE_STIMEFORMAT)
        .as_deref()
        .and_then(regional::hour_cycle_of_pattern)
    {
        formats = formats.with_hour_cycle(cycle);
    }
    if let Some(measurement) = info(LOCALE_IMEASURE)
        .as_deref()
        .and_then(regional::windows_measurement)
    {
        formats = formats.with_measurement(measurement);
    }
    if let Some(numbering) = info(LOCALE_IDIGITSUBSTITUTION)
        .as_deref()
        .and_then(|mode| regional::windows_numbering(mode, || info(LOCALE_SNATIVEDIGITS)))
    {
        formats = formats.with_numbering(numbering);
    }
    if let Some(region) =
        regional::region_override(info(LOCALE_SISO3166CTRYNAME).as_deref(), first_language)
    {
        formats = formats.with_region(region);
    }
    formats
}

/// One value of the user default locale, without its terminator.
fn info(kind: u32) -> Option<String> {
    let mut buffer = [0u16; VALUE_UNITS];
    // SAFETY: a null locale name is LOCALE_NAME_USER_DEFAULT, and the buffer is
    // an exclusively borrowed local whose length the binding passes as its
    // capacity, so the call writes only inside it.
    let written = unsafe { GetLocaleInfoEx(PCWSTR::null(), kind, Some(&mut buffer)) };
    let written = usize::try_from(written).ok().filter(|&units| units > 0)?;
    let value = buffer.get(..written)?;
    let value = value.strip_suffix(&[0]).unwrap_or(value);
    String::from_utf16(value).ok()
}
