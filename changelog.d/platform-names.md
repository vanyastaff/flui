### Changed

- `flui_platform` names say what each type is: `DisplayInfo` is `DisplayDescription` (one
  display as the operating system describes it; `DisplaySnapshot` is the list of them) and
  `DisplayParts::into_info` is `into_description`; `PointerInfo` is `PointerSource` (the contact,
  device and kind an event comes from) and `MismatchedPointerInfo` is `MismatchedPointerSource`;
  an activity result's `ResultData` is `ResultIntent` and its host side `ResultDataHold` is
  `ResultIntentHold`.
