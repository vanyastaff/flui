### Added

- **The screen reader's cursor reaches widgets** (`flui-semantics`, `flui-rendering`,
  `flui-runtime`, `flui-view`, `flui-widgets`): `Semantics::on_assistive_focus_within` is told
  `true` when VoiceOver's or TalkBack's cursor enters its subtree and `false` when it leaves.
  `PresentationCapabilities::assistive_focus` and `LifecycleContext::assistive_focus_support` say
  whether the platform reports the cursor; of the native adapters only iOS's does (gap 36).
  `PlatformAccessibility` gains `assistive_focus_support` and `set_assistive_focus_listener`.
- **A carousel pauses while a screen reader reads it** (`flui-widgets`): the cursor entering a
  `Carousel` pauses its autoplay, where the platform reports it.
- **Headless cursor moves** (`flui-testing`): `HeadlessHost::move_assistive_focus` and
  `LaidOut::move_assistive_focus` move the cursor as a platform would; the recording format has
  an `assistive_focus` input.
- **VoiceOver's cursor on iOS** (`flui-native`): the iOS adapter reports where VoiceOver's
  cursor is, through a hook added to a vendored `accesskit_ios` 0.2.2 (`third_party/`) until
  upstream has one.
