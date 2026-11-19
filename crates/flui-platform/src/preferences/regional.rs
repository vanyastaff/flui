//! The user's regional formats: how dates, times and numbers are written,
//! apart from the language (ADR-0278).
//!
//! Each field is one key of the Unicode BCP 47 `u` extension (UTS #35): the
//! first day of the week (`fw`), the hour cycle (`hc`), the measurement
//! system (`ms`), the numbering system (`nu`) and a region override (`rg`).
//! A host reports what the user set; [`RegionalFormats::apply_to`] puts it on
//! the locale a formatter is given. Pattern strings a host lets the user type
//! (a custom short date on Windows) are not carried: their syntax is the
//! host's own.

use crate::Locale;

/// A regional value outside what its Unicode key allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidRegionalFormat {
    /// A numbering system is three to eight ASCII letters or digits.
    #[error("invalid numbering system")]
    NumberingSystem,
    /// A region is two ASCII letters or three ASCII digits.
    #[error("invalid region")]
    Region,
}

/// A day of the week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Weekday {
    /// Monday.
    Monday,
    /// Tuesday.
    Tuesday,
    /// Wednesday.
    Wednesday,
    /// Thursday.
    Thursday,
    /// Friday.
    Friday,
    /// Saturday.
    Saturday,
    /// Sunday.
    Sunday,
}

impl Weekday {
    /// Every day, Monday first.
    pub const ALL: [Self; 7] = [
        Self::Monday,
        Self::Tuesday,
        Self::Wednesday,
        Self::Thursday,
        Self::Friday,
        Self::Saturday,
        Self::Sunday,
    ];

    /// The day's value for the `fw` key, such as `mon`.
    #[must_use]
    pub const fn unicode_value(self) -> &'static str {
        match self {
            Self::Monday => "mon",
            Self::Tuesday => "tue",
            Self::Wednesday => "wed",
            Self::Thursday => "thu",
            Self::Friday => "fri",
            Self::Saturday => "sat",
            Self::Sunday => "sun",
        }
    }

    /// The day an `fw` value names, in any case.
    #[must_use]
    pub fn from_unicode_value(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|day| day.unicode_value().eq_ignore_ascii_case(value))
    }
}

/// How the hours of a day are numbered (the `hc` key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HourCycle {
    /// Twelve hours, `0` to `11`, as in Japan's `午前0時`.
    H11,
    /// Twelve hours, `1` to `12`: the 12-hour clock.
    H12,
    /// Twenty-four hours, `0` to `23`: the 24-hour clock.
    H23,
    /// Twenty-four hours, `1` to `24`.
    H24,
}

impl HourCycle {
    /// Every cycle, in the order UTS #35 lists them.
    pub const ALL: [Self; 4] = [Self::H11, Self::H12, Self::H23, Self::H24];

    /// The cycle's value for the `hc` key, such as `h23`.
    #[must_use]
    pub const fn unicode_value(self) -> &'static str {
        match self {
            Self::H11 => "h11",
            Self::H12 => "h12",
            Self::H23 => "h23",
            Self::H24 => "h24",
        }
    }

    /// The cycle an `hc` value names, in any case.
    #[must_use]
    pub fn from_unicode_value(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|cycle| cycle.unicode_value().eq_ignore_ascii_case(value))
    }

    /// Whether the cycle counts twenty-four hours.
    #[must_use]
    pub const fn is_24_hour(self) -> bool {
        matches!(self, Self::H23 | Self::H24)
    }
}

/// The units measurements are given in (the `ms` key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MeasurementSystem {
    /// The metric system.
    Metric,
    /// United States customary units.
    UsCustomary,
    /// The imperial units used in the United Kingdom.
    Imperial,
}

impl MeasurementSystem {
    /// Every system, in the order UTS #35 lists them.
    pub const ALL: [Self; 3] = [Self::Metric, Self::UsCustomary, Self::Imperial];

    /// The system's value for the `ms` key, such as `metric`.
    #[must_use]
    pub const fn unicode_value(self) -> &'static str {
        match self {
            Self::Metric => "metric",
            Self::UsCustomary => "ussystem",
            Self::Imperial => "uksystem",
        }
    }

    /// The system an `ms` value names, in any case.
    #[must_use]
    pub fn from_unicode_value(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|system| system.unicode_value().eq_ignore_ascii_case(value))
    }
}

/// The digits numbers are written with (the `nu` key), such as `latn` or
/// `arab`: three to eight ASCII letters or digits, kept in lowercase.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NumberingSystem(Box<str>);

impl NumberingSystem {
    /// The numbering system named `value`, in any case.
    ///
    /// # Errors
    /// [`InvalidRegionalFormat::NumberingSystem`] unless `value` is three to
    /// eight ASCII letters or digits.
    pub fn try_new(value: &str) -> Result<Self, InvalidRegionalFormat> {
        if !(3..=8).contains(&value.len()) || !value.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(InvalidRegionalFormat::NumberingSystem);
        }
        Ok(Self(value.to_ascii_lowercase().into_boxed_str()))
    }

    /// The system's value for the `nu` key.
    #[must_use]
    pub fn unicode_value(&self) -> &str {
        &self.0
    }
}

/// The region whose formats the user chose apart from the language's own
/// region (the `rg` key): two ASCII letters (kept in uppercase) or three
/// ASCII digits, as in a language tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionOverride([u8; 3], u8);

impl RegionOverride {
    /// The region `code` names, in any case.
    ///
    /// # Errors
    /// [`InvalidRegionalFormat::Region`] unless `code` is two ASCII letters
    /// or three ASCII digits.
    pub fn try_new(code: &str) -> Result<Self, InvalidRegionalFormat> {
        let bytes = code.as_bytes();
        let valid = match bytes.len() {
            2 => bytes.iter().all(u8::is_ascii_alphabetic),
            3 => bytes.iter().all(u8::is_ascii_digit),
            _ => false,
        };
        if !valid {
            return Err(InvalidRegionalFormat::Region);
        }
        let mut region = [0; 3];
        for (slot, byte) in region.iter_mut().zip(bytes) {
            *slot = byte.to_ascii_uppercase();
        }
        let len = u8::try_from(bytes.len()).expect("BUG: a region code is at most three bytes");
        Ok(Self(region, len))
    }

    /// The region code, such as `GB` or `419`.
    #[must_use]
    pub fn code(&self) -> &str {
        std::str::from_utf8(&self.0[..usize::from(self.1)])
            .expect("BUG: a region code holds ASCII only")
    }

    /// The region's value for the `rg` key: the code in lowercase followed
    /// by `zzzz`, the subdivision UTS #35 uses for a whole region.
    #[must_use]
    pub fn unicode_value(&self) -> String {
        let mut value = self.code().to_ascii_lowercase();
        value.push_str("zzzz");
        value
    }

    /// The region an `rg` value names: a region code followed by a
    /// subdivision suffix, which is dropped.
    #[must_use]
    pub fn from_unicode_value(value: &str) -> Option<Self> {
        let code_len = if value.bytes().next()?.is_ascii_digit() {
            3
        } else {
            2
        };
        let (code, subdivision) = value.split_at_checked(code_len)?;
        if !(1..=4).contains(&subdivision.len())
            || !subdivision.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return None;
        }
        Self::try_new(code).ok()
    }
}

/// What the user set for writing dates, times and numbers, apart from the
/// language. A field the host did not report is `None`, and a formatter
/// then uses the locale's own default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RegionalFormats {
    first_weekday: Option<Weekday>,
    hour_cycle: Option<HourCycle>,
    measurement: Option<MeasurementSystem>,
    numbering: Option<NumberingSystem>,
    region: Option<RegionOverride>,
}

impl RegionalFormats {
    /// The day a week starts on.
    #[must_use]
    pub const fn first_weekday(&self) -> Option<Weekday> {
        self.first_weekday
    }

    /// Record the day a week starts on.
    #[must_use]
    pub const fn with_first_weekday(mut self, value: Weekday) -> Self {
        self.first_weekday = Some(value);
        self
    }

    /// How the hours of a day are numbered.
    #[must_use]
    pub const fn hour_cycle(&self) -> Option<HourCycle> {
        self.hour_cycle
    }

    /// Record how the hours of a day are numbered.
    #[must_use]
    pub const fn with_hour_cycle(mut self, value: HourCycle) -> Self {
        self.hour_cycle = Some(value);
        self
    }

    /// The units measurements are given in.
    #[must_use]
    pub const fn measurement(&self) -> Option<MeasurementSystem> {
        self.measurement
    }

    /// Record the units measurements are given in.
    #[must_use]
    pub const fn with_measurement(mut self, value: MeasurementSystem) -> Self {
        self.measurement = Some(value);
        self
    }

    /// The digits numbers are written with.
    #[must_use]
    pub const fn numbering(&self) -> Option<&NumberingSystem> {
        self.numbering.as_ref()
    }

    /// Record the digits numbers are written with.
    #[must_use]
    pub fn with_numbering(mut self, value: NumberingSystem) -> Self {
        self.numbering = Some(value);
        self
    }

    /// The region whose formats the user chose apart from the language.
    #[must_use]
    pub const fn region(&self) -> Option<RegionOverride> {
        self.region
    }

    /// Record the region whose formats the user chose.
    #[must_use]
    pub const fn with_region(mut self, value: RegionOverride) -> Self {
        self.region = Some(value);
        self
    }

    /// Whether the host reported none of the formats.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.first_weekday.is_none()
            && self.hour_cycle.is_none()
            && self.measurement.is_none()
            && self.numbering.is_none()
            && self.region.is_none()
    }

    /// The formats as `u` extension keywords, by key.
    fn keywords(&self) -> Vec<(&'static str, String)> {
        let mut keywords = Vec::new();
        if let Some(day) = self.first_weekday {
            keywords.push(("fw", day.unicode_value().to_owned()));
        }
        if let Some(cycle) = self.hour_cycle {
            keywords.push(("hc", cycle.unicode_value().to_owned()));
        }
        if let Some(system) = self.measurement {
            keywords.push(("ms", system.unicode_value().to_owned()));
        }
        if let Some(numbering) = &self.numbering {
            keywords.push(("nu", numbering.unicode_value().to_owned()));
        }
        if let Some(region) = self.region {
            keywords.push(("rg", region.unicode_value()));
        }
        keywords
    }

    /// `locale` with these formats as `u` extension keywords: the locale to
    /// give a date, time or number formatter. A key `locale` already carries
    /// keeps its value, so an app's own keyword wins over the user's
    /// setting. A locale that cannot carry an extension (a grandfathered or
    /// private-use tag) is returned as it is.
    #[must_use]
    pub fn apply_to(&self, locale: &Locale) -> Locale {
        locale.with_unicode_keywords(&self.keywords())
    }

    /// Take the formats apart into their fields.
    #[must_use]
    pub fn into_parts(self) -> RegionalParts {
        let Self {
            first_weekday,
            hour_cycle,
            measurement,
            numbering,
            region,
        } = self;
        RegionalParts {
            first_weekday,
            hour_cycle,
            measurement,
            numbering,
            region,
        }
    }
}

/// The fields of a [`RegionalFormats`], public, for the mirrors and for the
/// same reason as [`PreferenceParts`](crate::PreferenceParts).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegionalParts {
    /// [`RegionalFormats::first_weekday`].
    pub first_weekday: Option<Weekday>,
    /// [`RegionalFormats::hour_cycle`].
    pub hour_cycle: Option<HourCycle>,
    /// [`RegionalFormats::measurement`].
    pub measurement: Option<MeasurementSystem>,
    /// [`RegionalFormats::numbering`].
    pub numbering: Option<NumberingSystem>,
    /// [`RegionalFormats::region`].
    pub region: Option<RegionOverride>,
}

impl RegionalParts {
    /// The formats these fields describe. Every field is already a valid
    /// value, so this cannot fail.
    #[must_use]
    pub fn into_formats(self) -> RegionalFormats {
        let Self {
            first_weekday,
            hour_cycle,
            measurement,
            numbering,
            region,
        } = self;
        RegionalFormats {
            first_weekday,
            hour_cycle,
            measurement,
            numbering,
            region,
        }
    }
}
