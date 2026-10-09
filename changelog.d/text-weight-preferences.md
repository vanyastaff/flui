### Added

- Text, RichText and EditableText apply inherited text-weight adjustments after
  authored style inheritance, including explicit variable-font weight axes.
- The UIKit backend observes Bold Text through its host-owned preference source.

### Changed

- Replace the unused Boolean system `bold_text` API with `TextWeightPreference`
  and `SystemPreferences::text_weight` / `with_text_weight` to retain categorical
  and signed numeric observations.
- Public `MediaQueryData` and `ParagraphSpec` literals now include
  `font_weight_adjustment`; use zero to preserve authored weights. Categorical
  Bold Text projects to a +300 adjustment, with adjusted weights bounded to
  1..1000 and explicit nested providers retaining authority.
