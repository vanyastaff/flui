### Changed

- `OverScrollHeaderStretchConfiguration` takes an `on_stretch_trigger: Option<StretchTrigger>`
  callback in place of `stretch_trigger: Option<StretchTriggerSignal>`. A stretching persistent
  header runs it during layout, once each time its over-scroll reaches `stretch_trigger_offset`,
  and again only after the stretch fell below the offset. The polled `StretchTriggerSignal`
  counter is removed (`flui-objects`, `flui-widgets`).
