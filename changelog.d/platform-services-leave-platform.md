### Removed

- `flui_native::Platform` no longer has `open_url`, `reveal_path`, `open_path`, `prompt_for_paths`,
  `prompt_for_new_path` or `app_path`. Open a URI or a file, reveal it or trash it through
  `flui_platform::launch`, and show file dialogs through `flui-system` (the facade's `system`
  feature); find the running executable with `std::env::current_exe`.
- `flui_native::Task`, `flui_native::TaskLabel` and `flui_native::PathPromptOptions` are gone with
  the file prompts, and `PlatformError` no longer has the `AppPath` and `Dialog` variants.

### Changed

- The host table's `flui_native::host::Services` trait is now `flui_native::host::Launch` and holds
  only `launch_value` and `page_origin`.
