### Fixed

- Text, RichText and EditableText never shape at a size derived from an invalid
  text scale: `TextSizing::linear` refuses a factor that is not finite and
  positive, and a system observation outside `1/64..=64` never enters
  `SystemPreferences`.
