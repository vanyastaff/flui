# ADR-0278: The user's regional formats are the Unicode locale extension keys, carried beside the locale

- **Status:** Accepted (2026-10-31). Not implemented: no host reads the formats yet (§3).
- **Date:** 2026-10-31
- **Related:** [ADR-0172](ADR-0172-host-owned-system-preferences.md)
  (system preferences are a host observation the runtime accepts as one snapshot),
  [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md) (recordings mirror every
  preference field), [ADR-0271](ADR-0271-the-host-feed-asks-the-platform-which-faces-fall-back.md)
  (the font fallback follows the first preferred locale)

## Context and problem statement

Every desktop and mobile host lets the user set how dates, times and numbers are written apart
from the language: the first day of the week, a 12- or 24-hour clock, metric or US units, the
digits, and on Windows, macOS and Android 14 a region whose formats apply under another
language. `SystemPreferences` carried the preferred languages only, so an app in English on a
German machine showed a Sunday-first week and a 12-hour clock (`docs/gaps.md` entry 14). Hosts
spell these settings differently. Windows has `LOCALE_IFIRSTDAYOFWEEK` and a time-format pattern,
Apple has `NSCalendar.firstWeekday` and the `j` skeleton, Android has `Calendar.getFirstDayOfWeek`,
`DateFormat.is24HourFormat` and, from API 34, `-u-` keywords on the locale. The Web has
`Intl.Locale` hour cycles and week info. Formatting dates and numbers is a separate feature that
FLUI does not have yet. How does a host's setting reach an app and, later, a formatter, in one
shape for every host?

## Decision drivers

- One shape for every host, which a formatter (ICU4X `icu_datetime`, `icu_decimal`) takes without
  a lossy mapping.
- The locale `Localizations` resolves keeps matching delegates that compare it to a declared
  locale.
- A preference added to the snapshot reaches every recording mirror at compile time (ADR-0215).
- An app's own locale keywords are its decision and win over the user's setting.

## Considered options

1. **Typed fields, one per Unicode `u` extension key (`fw`, `hc`, `ms`, `nu`, `rg`), carried
   beside the locales and joined to a locale on request.**
2. **The host's own pattern strings** (Windows `LOCALE_SSHORTDATE`, Apple date formats).
3. **Add the keys to the locale `Localizations` resolves.**
4. **A boolean `always_use_24_hour_format`**, as in some other frameworks.

## Decision

Chosen option: 1, because the keys are the vocabulary every formatter already reads, and keeping
them beside the locale leaves resource resolution as it is.

### §1 The value, in `flui-platform`

```rust
#[non_exhaustive] pub enum Weekday { Monday, Tuesday, Wednesday, Thursday, Friday, Saturday, Sunday }
#[non_exhaustive] pub enum HourCycle { H11, H12, H23, H24 }
#[non_exhaustive] pub enum MeasurementSystem { Metric, UsCustomary, Imperial }
pub struct NumberingSystem(/* 3..=8 ASCII alphanumerics, lowercase */);
pub struct RegionOverride(/* two letters or three digits */);

#[non_exhaustive]
pub struct RegionalFormats { /* each field Option */ }
impl RegionalFormats {
    pub fn apply_to(&self, locale: &Locale) -> Locale;
    pub fn into_parts(self) -> RegionalParts;
}
impl SystemPreferences {
    pub const fn regional(&self) -> &RegionalFormats;
    pub fn with_regional(self, value: RegionalFormats) -> Self;
}
```

Each enum names itself with `unicode_value()` and reads one back with `from_unicode_value()` (any
case). `HourCycle` keeps all four UTS #35 values, though hosts report only `H12` and `H23`, so the
formatter mapping stays exact. `NumberingSystem::try_new` and `RegionOverride::try_new` refuse a
value their key does not allow (`InvalidRegionalFormat`). `Weekday` is the one weekday type in
FLUI; later date code uses it.

`apply_to` adds the formats to `locale` as `u` keywords, in canonical order. A key the locale
already carries keeps its value. A tag that cannot carry an extension (private-use or irregular
grandfathered) is returned unchanged. `PreferenceParts` gains `regional: RegionalParts`, so both
recording codecs break at compile time until they mirror it. The shared wire object writes each
field as its Unicode value and leaves the object out when empty.

### §2 The value reaches apps through `MediaQuery`, not through `Localizations`

```rust
pub struct MediaQueryData { /* … */ pub regional_formats: RegionalFormats }
impl MediaQuery { pub fn regional_formats_of(ctx: &dyn BuildContext) -> Option<RegionalFormats>; }
```

The UI runtime commits the accepted snapshot's formats with the other preferences. A reader
depends on that field alone. The locale `Localizations` resolves stays without the keys. A
formatter is given `formats.apply_to(&locale)`.

### §3 Each host reads its own setting

| Host | First weekday | Hour cycle | Measurement | Numbering, region | Change signal |
|------|---------------|------------|-------------|-------------------|---------------|
| Windows | `LOCALE_IFIRSTDAYOFWEEK` | `H` in `LOCALE_STIMEFORMAT` | `LOCALE_IMEASURE` | digit substitution, user GeoID | `WM_SETTINGCHANGE` `"intl"` |
| macOS, iOS | `NSCalendar.currentCalendar.firstWeekday` | `j` skeleton of `dateFormatFromTemplate` | `NSLocaleMeasurementSystem` | `NSLocale` numbering, region | `NSCurrentLocaleDidChangeNotification` |
| Android | `Calendar.getFirstDayOfWeek` | `DateFormat.is24HourFormat` | `LocaleData` | the locale's `-u-` keywords from API 34 | `ACTION_LOCALE_CHANGED`, a `TIME_12_24` observer |
| Linux | `nl_langinfo` of `LC_TIME` | the portal's `clock-format` | `LC_MEASUREMENT` | none | portal `SettingChanged` |
| Web | `Intl.Locale.getWeekInfo` where present | `resolvedOptions().hourCycle` | none | none | `languagechange` |

Each host parses its strings and flags in a pure `pub(crate)` function, tested on Linux. Not
implemented: every row of this table.

### Consequences

- Good, because one value carries every host's setting, and a formatter reads it as the keys it
  already knows.
- Good, because delegates keep matching the resolved locale exactly.
- Bad, because a pattern the user typed (a custom Windows short date) is not carried. It is named
  in `docs/gaps.md` entry 14.
- Bad, because nothing formats dates or numbers yet. The formatter is a later feature that takes
  `apply_to`.
- Neutral, because a host that reports nothing leaves the formats empty, and a formatter then
  uses the locale's defaults.

### Confirmation

- `regional_formats_join_the_locale_a_formatter_takes` (flui-platform, `preferences_contract`)
  covers key order, extension order, private use, an app's own keyword kept, and tags left as
  they are.
- `regional_values_read_back_their_unicode_names` (same table) covers every variant's name read
  back, and the refusals.
- `preferences_round_trip_through_their_parts` (flui-platform, `parts_table`) carries every
  regional field through the parts.
- `regional_formats_reach_media_query` (flui-runtime, `owner_metrics_contract`) checks that the
  formats reach `MediaQuery::regional_formats_of`, and that another preference does not rebuild
  their reader.
- `every_vocabulary_variant_round_trips_through_the_recording_dto` and
  `preference_rows_keep_their_json_spelling` (flui-testing), and
  `platform_recordings_replay_on_the_headless_host` (flui-native), carry the formats through
  both recordings.

## Pros and cons of the options

### 1. Typed fields per Unicode key

- Good, because the mapping to UTS #35 and ICU4X is one to one.
- Good, because every value is checked when it is built.
- Bad, because a setting outside the keys (a typed pattern) is dropped.

### 2. Host pattern strings

- Good, because a custom pattern survives.
- Bad, because each host has its own pattern syntax, and nothing in FLUI could render it.
- Bad, because a recording would carry a host's syntax.

### 3. Keys on the resolved locale

- Good, because one value carries language and formats.
- Bad, because a delegate comparing `*locale == Locale::en_us()` stops matching
  `en-US-u-hc-h23`, so resources fail to load.

### 4. A 24-hour flag

- Good, because it is small.
- Bad, because it covers one of five settings and cannot say `H11` or `H24`.

## More information

- Unicode Technical Standard #35, Unicode Locale Data Markup Language, §3.6.1 key and type
  definitions (`fw`, `hc`, `ms`, `nu`, `rg`): <https://unicode.org/reports/tr35/#Key_And_Type_Definitions_>.
- Android regional preferences, API 34:
  <https://developer.android.com/about/versions/14/features#regional-preferences>.
- Windows `GetLocaleInfoEx`:
  <https://learn.microsoft.com/windows/win32/api/winnls/nf-winnls-getlocaleinfoex>.
- Revisit when FLUI formats dates and numbers: the formatter decides whether a typed pattern is
  worth carrying.
