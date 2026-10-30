# Material animated icon frames

Key frames of the Material animated icons, one SVG per icon. Each `<g id="frame-NN">` is one
key frame; `data-flui-match-text-direction` marks the icons that mirror under right-to-left
text.

| Field | Value |
|-------|-------|
| Source | `packages/flutter/lib/src/material/animated_icons/data/*.g.dart` in [flutter/flutter](https://github.com/flutter/flutter) |
| Revision | `5259851357f0c28026db9ba09e5e413278129c1b` |
| License | BSD-3-Clause, [`LICENSE-flutter.txt`](LICENSE-flutter.txt) |
| Frames written by | `cargo xtask icon-import flutter --flutter <checkout>` |
| Constants written by | `cargo xtask icon-import material --write` (`src/animated_icons/data/`) |

`cargo xtask checks` runs `cargo xtask icon-import material`, which fails when the constants
disagree with these frames. A new revision is taken by checking Flutter out at it, running both
commands and updating the revision here.

New icons that are not Flutter's come in through `cargo xtask icon-import svg` (one file or
group per frame) or `cargo xtask icon-import avd` (an Android animated vector drawable).
