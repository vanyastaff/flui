### Added
- The web host reports high contrast, from `prefers-contrast` and `forced-colors`, and the browser's preferred languages, from `navigator.languages`, and follows their changes. A browser that knows neither contrast feature leaves high contrast unobserved, and an unparsable language tag is skipped without hiding the others.
