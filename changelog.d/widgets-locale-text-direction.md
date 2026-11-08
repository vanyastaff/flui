### Changed

- Text direction is now resolved from `Locale::is_rtl`, including explicit script subtags, instead of localized resource implementations. `Localizations` publishes that direction to its subtree.

### Removed

- Removed `GlobalWidgetsLocalizations`, `GlobalWidgetsLocalizationsDelegate`, `RTL_LANGUAGES`, and `WidgetsLocalizations::text_direction`. Use `DefaultWidgetsLocalizationsDelegate` for English resources and `Locale::is_rtl` for text direction.
