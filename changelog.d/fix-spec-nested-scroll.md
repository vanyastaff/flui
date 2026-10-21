### Added

- **`TrackpadScript::unended_pan`** (`flui-testing`): a phased pan whose `Ended` event never comes,
  as a platform that begins a phased scroll without ending it sends (Wayland).

### Removed

- **`RefreshIndicatorBuilder`** (`flui-widgets`): the alias named no type a caller passes or
  receives; `RefreshIndicatorStyle::new` takes the builder closure directly.
