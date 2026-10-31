//! The user's regional formats (ADR-0278 §3) from the values the hosts report
//! them as: Windows' locale information strings, the POSIX environment's
//! category locales, and GNOME's clock format.

#[cfg(any(linux_host, test))]
use flui_platform::RegionalFormats;
use flui_platform::{HourCycle, Locale, MeasurementSystem, RegionOverride};
#[cfg(any(windows, test))]
use flui_platform::{NumberingSystem, Weekday};

/// `LOCALE_IFIRSTDAYOFWEEK`: `0` is Monday through `6`, Sunday.
#[cfg(any(windows, test))]
pub(crate) fn windows_first_weekday(value: &str) -> Option<Weekday> {
    let index: usize = value.trim().parse().ok()?;
    Weekday::ALL.get(index).copied()
}

/// `LOCALE_IMEASURE`: `0` is metric, `1` US customary.
#[cfg(any(windows, test))]
pub(crate) fn windows_measurement(value: &str) -> Option<MeasurementSystem> {
    match value.trim() {
        "0" => Some(MeasurementSystem::Metric),
        "1" => Some(MeasurementSystem::UsCustomary),
        _ => None,
    }
}

/// The hour cycle a time pattern writes its hours in: `h` 1 to 12, `H` 0 to
/// 23, `K` 0 to 11 and `k` 1 to 24, outside quoted literals. A pattern without
/// hours has none.
#[cfg(any(windows, test))]
pub(crate) fn hour_cycle_of_pattern(pattern: &str) -> Option<HourCycle> {
    let mut quoted = false;
    for unit in pattern.chars() {
        match unit {
            '\'' => quoted = !quoted,
            'h' if !quoted => return Some(HourCycle::H12),
            'H' if !quoted => return Some(HourCycle::H23),
            'K' if !quoted => return Some(HourCycle::H11),
            'k' if !quoted => return Some(HourCycle::H24),
            _ => {}
        }
    }
    None
}

/// The numbering system whose zero `digits` starts with, as Windows'
/// `LOCALE_SNATIVEDIGITS` lists them, ASCII digits as `latn`. A zero this
/// table does not name reports nothing.
#[cfg(any(windows, test))]
pub(crate) fn numbering_of_digits(digits: &str) -> Option<NumberingSystem> {
    let name = match digits.chars().next()? {
        '0' => "latn",
        '\u{0660}' => "arab",
        '\u{06F0}' => "arabext",
        '\u{0966}' => "deva",
        '\u{09E6}' => "beng",
        '\u{0A66}' => "guru",
        '\u{0AE6}' => "gujr",
        '\u{0B66}' => "orya",
        '\u{0BE6}' => "tamldec",
        '\u{0C66}' => "telu",
        '\u{0CE6}' => "knda",
        '\u{0D66}' => "mlym",
        '\u{0E50}' => "thai",
        '\u{0ED0}' => "laoo",
        '\u{0F20}' => "tibt",
        '\u{1040}' => "mymr",
        '\u{17E0}' => "khmr",
        '\u{1810}' => "mong",
        _ => return None,
    };
    NumberingSystem::try_new(name).ok()
}

/// The numbering system `LOCALE_IDIGITSUBSTITUTION` asks for: `1` always
/// ASCII digits (`latn`), `2` always the locale's native digits, which
/// `native_digits` lists. `0`, digits chosen by context, names none.
#[cfg(any(windows, test))]
pub(crate) fn windows_numbering(
    substitution: &str,
    native_digits: impl FnOnce() -> Option<String>,
) -> Option<NumberingSystem> {
    match substitution.trim() {
        "1" => NumberingSystem::try_new("latn").ok(),
        "2" => numbering_of_digits(&native_digits()?),
        _ => None,
    }
}

/// The region whose formats apply when it is not the region of the first
/// preferred language, as the formats locale names it. Without a region, or
/// when it is the language's own, there is no override.
pub(crate) fn region_override(
    formats_region: Option<&str>,
    first_language: Option<&Locale>,
) -> Option<RegionOverride> {
    let region = formats_region?;
    if first_language.and_then(Locale::country) == Some(region) {
        return None;
    }
    RegionOverride::try_new(region).ok()
}

/// GNOME's `clock-format`: `24h` or `12h`.
#[cfg(any(linux_host, test))]
pub(crate) fn gnome_clock_format(value: &str) -> Option<HourCycle> {
    match value {
        "24h" => Some(HourCycle::H23),
        "12h" => Some(HourCycle::H12),
        _ => None,
    }
}

/// The formats a POSIX environment sets apart from the messages locale, `var`
/// answering one environment variable. `LC_TIME` names the region whose dates
/// and times apply, and `LC_MEASUREMENT` the units, each overridden by
/// `LC_ALL` and standing in for `LANG`. A value the first preferred language,
/// or the region override, already implies reports nothing.
#[cfg(any(linux_host, test))]
pub(crate) fn from_posix_environment(
    var: impl Fn(&str) -> Option<String>,
    first_language: Option<&Locale>,
) -> RegionalFormats {
    let category = |name: &str| {
        ["LC_ALL", name, "LANG"]
            .into_iter()
            .find_map(|name| var(name).filter(|value| !value.is_empty()))
            .and_then(|value| posix_region(&value))
    };
    let mut formats = RegionalFormats::default();
    let time = category("LC_TIME");
    if let Some(region) = region_override(time.as_deref(), first_language) {
        formats = formats.with_region(region);
    }
    // A formatter takes the units from the region override when there is one
    // (UTS #35 `rg`), so the units are compared with that region's.
    let formats_region = time
        .as_deref()
        .or_else(|| first_language.and_then(Locale::country));
    if let Some(units) = category("LC_MEASUREMENT").map(|region| measurement_of_region(&region))
        && formats_region.map(measurement_of_region) != Some(units)
    {
        formats = formats.with_measurement(units);
    }
    formats
}

/// The region of a POSIX locale name, `language_REGION.charset@modifier`.
#[cfg(any(linux_host, test))]
fn posix_region(value: &str) -> Option<String> {
    let name = value.split(['.', '@']).next()?;
    let (_, region) = name.split_once('_')?;
    RegionOverride::try_new(region)
        .ok()
        .map(|region| region.code().to_owned())
}

/// The measurement system CLDR's supplemental data gives a region, which is
/// the default a formatter takes for it: US customary in the United States,
/// Liberia and Myanmar, the UK system in the United Kingdom, metric elsewhere.
#[cfg(any(linux_host, test))]
fn measurement_of_region(region: &str) -> MeasurementSystem {
    match region {
        "US" | "LR" | "MM" => MeasurementSystem::UsCustomary,
        "GB" => MeasurementSystem::Imperial,
        _ => MeasurementSystem::Metric,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows_values_read_as_their_documented_codes() {
        assert_eq!(windows_first_weekday("0"), Some(Weekday::Monday));
        assert_eq!(windows_first_weekday("6"), Some(Weekday::Sunday));
        for value in ["7", "-1", "", "mon"] {
            assert_eq!(windows_first_weekday(value), None, "{value:?}");
        }
        assert_eq!(windows_measurement("0"), Some(MeasurementSystem::Metric));
        assert_eq!(
            windows_measurement("1"),
            Some(MeasurementSystem::UsCustomary)
        );
        assert_eq!(windows_measurement("2"), None);
    }

    fn a_time_pattern_names_its_hour_cycle_outside_literals() {
        for (pattern, expected) in [
            ("HH:mm:ss", Some(HourCycle::H23)),
            ("h:mm:ss tt", Some(HourCycle::H12)),
            ("tt hh:mm", Some(HourCycle::H12)),
            ("'h'H:mm", Some(HourCycle::H23)),
            ("K:mm", Some(HourCycle::H11)),
            ("kk:mm", Some(HourCycle::H24)),
            ("mm:ss", None),
            ("'hh' mm", None),
        ] {
            assert_eq!(hour_cycle_of_pattern(pattern), expected, "{pattern:?}");
        }
    }

    fn native_digits_name_their_numbering_system() {
        let name = |digits: &str| {
            numbering_of_digits(digits).map(|system| system.unicode_value().to_owned())
        };
        assert_eq!(name("٠١٢٣٤٥٦٧٨٩").as_deref(), Some("arab"));
        assert_eq!(name("۰۱۲۳۴۵۶۷۸۹").as_deref(), Some("arabext"));
        assert_eq!(name("०१२३४५६७८९").as_deref(), Some("deva"));
        assert_eq!(name("0123456789").as_deref(), Some("latn"));
        assert_eq!(name(""), None);
        let substituted = |mode: &str, digits: &'static str| {
            windows_numbering(mode, || Some(digits.to_owned()))
                .map(|system| system.unicode_value().to_owned())
        };
        assert_eq!(substituted("1", "٠١٢٣٤٥٦٧٨٩").as_deref(), Some("latn"));
        assert_eq!(substituted("2", "٠١٢٣٤٥٦٧٨٩").as_deref(), Some("arab"));
        assert_eq!(substituted("0", "٠١٢٣٤٥٦٧٨٩"), None);
        assert_eq!(
            substituted("2", "0123456789").as_deref(),
            Some("latn"),
            "ASCII native digits under a non-Latin language are an observation"
        );
    }

    fn a_region_overrides_only_another_languages_region() {
        let en_us = Locale::en_us();
        assert_eq!(region_override(Some("US"), Some(&en_us)), None);
        assert_eq!(
            region_override(Some("DE"), Some(&en_us)).map(|region| region.unicode_value()),
            Some("dezzzz".to_owned())
        );
        assert_eq!(
            region_override(Some("DE"), None).map(|region| region.unicode_value()),
            Some("dezzzz".to_owned())
        );
        assert_eq!(region_override(None, Some(&en_us)), None);
        assert_eq!(region_override(Some("D"), Some(&en_us)), None);
    }

    fn gnome_clock_formats_read_as_hour_cycles() {
        assert_eq!(gnome_clock_format("24h"), Some(HourCycle::H23));
        assert_eq!(gnome_clock_format("12h"), Some(HourCycle::H12));
        assert_eq!(gnome_clock_format("auto"), None);
    }

    fn posix(variables: &[(&str, &str)], first: Option<&Locale>) -> RegionalFormats {
        from_posix_environment(
            |name| {
                variables
                    .iter()
                    .find(|(variable, _)| *variable == name)
                    .map(|(_, value)| (*value).to_owned())
            },
            first,
        )
    }

    fn posix_categories_set_what_the_language_does_not() {
        let en_us = Locale::en_us();
        let lang = [("LANG", "en_US.UTF-8")];
        assert!(posix(&lang, Some(&en_us)).is_empty());
        let split = posix(
            &[
                ("LANG", "en_US.UTF-8"),
                ("LC_TIME", "de_DE.UTF-8@euro"),
                ("LC_MEASUREMENT", "en_GB.UTF-8"),
            ],
            Some(&en_us),
        );
        assert_eq!(
            split.region().map(|region| region.unicode_value()),
            Some("dezzzz".to_owned())
        );
        // A British units region under a German formats region keeps its own.
        assert_eq!(split.measurement(), Some(MeasurementSystem::Imperial));
        let metric_in_gb = posix(
            &[("LANG", "en_GB.UTF-8"), ("LC_MEASUREMENT", "de_DE.UTF-8")],
            Some(&Locale::en_gb()),
        );
        assert_eq!(
            metric_in_gb.measurement(),
            Some(MeasurementSystem::Metric),
            "German units under a British language are not the UK default"
        );
        assert!(
            posix(&[("LANG", "en_GB.UTF-8")], Some(&Locale::en_gb())).is_empty(),
            "a British language alone implies its own units"
        );
        let us_units = posix(
            &[("LANG", "en_US.UTF-8"), ("LC_TIME", "de_DE.UTF-8")],
            Some(&en_us),
        );
        assert_eq!(
            us_units.measurement(),
            Some(MeasurementSystem::UsCustomary),
            "LANG's US units survive a German formats region"
        );
        let all = posix(
            &[
                ("LANG", "de_DE.UTF-8"),
                ("LC_TIME", "de_DE.UTF-8"),
                ("LC_ALL", "en_US.UTF-8"),
            ],
            Some(&Locale::de_de()),
        );
        assert_eq!(
            all.region().map(|region| region.unicode_value()),
            Some("uszzzz".to_owned())
        );
        assert_eq!(all.measurement(), None, "the US override implies US units");
        for unusable in [
            &[("LC_TIME", "C")][..],
            &[("LC_TIME", "POSIX")][..],
            &[][..],
        ] {
            assert!(posix(unusable, Some(&en_us)).is_empty(), "{unusable:?}");
        }
    }

    #[test]
    fn regional_reads_contract() {
        crate::table_test::run_table(
            "regional_reads_contract",
            &[
                (
                    "windows_values_read_as_their_documented_codes",
                    windows_values_read_as_their_documented_codes as fn(),
                ),
                (
                    "a_time_pattern_names_its_hour_cycle_outside_literals",
                    a_time_pattern_names_its_hour_cycle_outside_literals as fn(),
                ),
                (
                    "native_digits_name_their_numbering_system",
                    native_digits_name_their_numbering_system as fn(),
                ),
                (
                    "a_region_overrides_only_another_languages_region",
                    a_region_overrides_only_another_languages_region as fn(),
                ),
                (
                    "gnome_clock_formats_read_as_hour_cycles",
                    gnome_clock_formats_read_as_hour_cycles as fn(),
                ),
                (
                    "posix_categories_set_what_the_language_does_not",
                    posix_categories_set_what_the_language_does_not as fn(),
                ),
            ],
        );
    }
}
