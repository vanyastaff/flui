### Changed

- `flui.toml` names the application by one identifier, `[app] id = "com.example.my_app"`, in
  place of `organization`; `flui create` makes it from `--org` and the project name and refuses a
  pair that does not form one. The Android package, the Apple bundle identifier (`_` as `-`) and
  the `FLUI_APP_ID` the application is built with are all derived from it, so they cannot
  disagree. A `flui.toml` that still names `organization` is refused with the `id` line to write.
- An application built without a `flui.toml` gets the identifier `dev.flui.<name>`.
