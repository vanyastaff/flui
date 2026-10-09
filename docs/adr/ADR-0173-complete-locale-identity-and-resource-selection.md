# ADR-0173: Complete locale identity and resource selection

- **Status:** Implemented; OS-generated language-change verification pending.
- **Date:** 2026-10-08
- **Related:** [ADR-0172](ADR-0172-host-owned-system-preferences.md).

## Decision

`Locale` owns one parsed, complete BCP 47 language identity. Variants and
extensions participate in equality, hashing and serialization. Three independent
language/script/region strings cannot represent `ca-ES-valencia`: treating its
third subtag as a region corrupts the preference, while dropping the variant
can select the wrong resources. Native adapters and authored resource lists use
the same type; an adapter does not silently filter unrepresentable languages.

The private parser is `language-tags`. Its grammar retains full tags, including
grandfathered and private-use tags. Boundary checks also refuse empty/oversized
subtags and repeated variants or extension keys. Parsing normalizes syntax and
casing, not registry membership or CLDR likely subtags. Existing `in`/`iw`/`ji`
and deprecated-region equivalences remain; explicit scripts are not suppressed.
The existing language-based direction policy remains separate from identity.

`Locale::new` and `with_script` return `Result<Locale, InvalidLocale>` and reject
components supplied in the wrong role. `FromStr` accepts full tags; legacy
underscore separators remain accepted input. `Display`, `to_language_tag` and
serde emit a normalized hyphenated tag. Serde accepts that tag string rather
than the former three-field object. This is an intentional pre-1.0 API and wire
change: callers handle construction failure and convert persisted old objects
through their language/script/region components before serialization.

## Observation and selection

The host's ordered preferred languages are an observation, not the application's
resolved locale. Runtime publication places them in the presentation's existing
`MediaQuery`, preserving unavailable versus observed-empty values. A nested
provider replaces the whole value just like the other media fields; it does not
search past an unavailable locale field into another ancestor.

`WidgetsApp` subscribes to the locale field during build. An explicit application
locale keeps precedence; removing it exposes current inherited preferences.
Resolution returns a supported resource locale: an exact match compares the full
identity, then the established script/region/language fallback applies. A preferred
extension that is absent from the supported list cannot masquerade as an exact
match and prevent a supported delegate from loading.

Windows reads `GetUserPreferredUILanguages(MUI_LANGUAGE_NAME)` in the existing
host sample. It uses the user UI-language order, not the formatting-region locale
or an application's thread language override. Its UTF-16 list and count must
agree, with the documented double terminator. Capacity races retry at most three
times per sample; allocation is bounded to two MiB. Failure preserves the previous
accepted snapshot and existing paced retry obligation. The host's existing
`WM_SETTINGCHANGE` receiver invalidates this sample; no second observer is added.

## Evidence and limits

`preferred_locales_select_resources_and_direction` exercises mounted resource
selection, order changes, unknown/empty fallback, overrides and a late runtime's
first build. `locale_override_removal_uses_the_nearest_current_preferences` checks
removal and nested-provider behavior. Public preference tests cover complete tag
identity, malformed input and alias/serde construction.

The private `preferred_language_buffer_contract` covers malformed native buffers
and changing capacity. The public native `preferences_contract` reads Windows
settings before a user window and checks the existing receiver's wake/teardown.
Synthetic setting messages prove routing, not that an OS language change emits
a notification. Other backends still report unavailable locale observations.

Sources: [language-tags API](https://docs.rs/language-tags/0.3.2/language_tags/struct.LanguageTag.html),
[Windows language-list contract](https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-getuserpreferreduilanguages).
