# Recorded `fonts.xml` files

The system font configuration of two Android releases, which
`fonts::android`'s parse is checked against
(`fonts_xml_orders_fallback_by_language`):

| File | Release | API | Shape |
|------|---------|-----|-------|
| `android-6.0.1_r81-fonts.xml` | Android 6.0.1 | 23 | fallback families mostly untagged; `ja`, `ko`, `zh-Hans`, `zh-Hant` |
| `android-9.0.0_r61-fonts.xml` | Android 9 | 28 | `und-XXXX` script tags, space-separated languages, `index` into `.ttc`, `fallbackFor="serif"` |

Both are `data/fonts/fonts.xml` from AOSP's `platform/frameworks/base` at the
tags `android-6.0.1_r81` and `android-9.0.0_r61`, unmodified, under the
Apache License 2.0 of the Android Open Source Project. The format is frozen for API 23
to 28; from API 29 FLUI asks `AFontMatcher` instead.
