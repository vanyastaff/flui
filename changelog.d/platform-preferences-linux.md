### Added
- On Linux the host reports high contrast, from the Settings portal's `contrast` or GNOME's `high-contrast`, and the text scale, from GNOME's `text-scaling-factor`, and follows their changes. A desktop without either key leaves the value unobserved and does not retry it.
- On Linux the preferred languages come from the environment as gettext reads it: `LANGUAGE`'s list unless the messages locale (`LC_ALL`, `LC_MESSAGES`, `LANG`) is `C` or `POSIX`. A script modifier names the script (`sr_RS@latin` is `sr-Latn-RS`); other modifiers and the charset are dropped.
