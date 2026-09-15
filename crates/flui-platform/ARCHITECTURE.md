# flui-platform Architecture

Per-crate ledger for architecture decisions that span more than one module in
this crate, as required by [`docs/PORT.md`](../../docs/PORT.md) §Per-crate
`ARCHITECTURE.md` template. Partial: this file exists for the `## Mapping
decisions` entries below; a full crate architecture writeup is deferred.

---

## Mapping decisions

### The winit backend delegates keyboard conversion to `ui-events-winit`; Win32/AppKit keep hand-written tables

**Rule:** every native keyboard event this crate receives must be normalized
into the canonical `ui_events`/`keyboard-types` vocabulary (`Code`, `Key`,
`Location`) at the platform boundary — see `traits/input.rs`'s module doc.
`Code::Unidentified` must mean the backend genuinely could not identify the
physical key, never that a conversion table was incomplete (issue #1092).

**Choice:** the winit backend (`platforms/winit/events.rs::keyboard_event`)
converts `PhysicalKey`/`Key`/`KeyLocation` through
`ui_events_winit::keyboard::{from_winit_code, from_winit_key,
from_winit_location}` (crates.io `ui-events-winit`, the same bridge Masonry
and Xilem use) instead of a hand-written match table maintained in this
crate. The Win32 (`shared/keys.rs`) and AppKit (`shared/keys_macos.rs`)
backends keep their own hand-written `Code`/`Key` tables, because no
equivalent bridge crate exists for `WM_KEYDOWN` scancodes or
`NSEvent.keyCode` — those two native id spaces are FLUI-specific translation
work with no upstream crate to delegate to.

This asymmetry is deliberate, not inconsistent: the rule is "delegate to an
audited bridge when one exists for this native surface," not "every backend
must look the same." `platforms/winit/events.rs`'s
`cross_backend_physical_key_agreement` test cross-checks a curated set of
physical keys against both hand-written tables so the two authored
translations and the delegated one cannot silently drift apart.

**Alternatives considered:**

- Complete the crate's own hand-written `winit::KeyCode → Code` table (kept
  as a dev-dependency oracle test against `ui-events-winit`, never shipped).
  Rejected: it is a second, unaudited copy of exactly the table the bridge
  crate already maintains and tests upstream — an ownership cost with no
  behavioral upside, and the class of bug this issue exists to fix (a
  hand-rolled table quietly falling behind the enum it mirrors).
- Vendor `ui-events-winit`'s tables directly into this crate (a `// PORT
  NOTE`-style copy). Rejected for the same reason as above, plus it would
  need re-syncing by hand on every winit/`ui-events` bump instead of
  `cargo update` doing it.

**Trade-off:** this crate's winit backend now tracks `ui-events-winit`'s
release cadence for keyboard fidelity — a regression or gap introduced
upstream reaches FLUI without a code review here. The `.github/dependabot.yml`
`ui-events-cohort` group (with `ui-events` itself) and the
`every_winit_keycode_maps_to_a_canonical_code` regression test are the
mitigations: a bump that drops coverage for a `KeyCode` variant fails CI
immediately rather than shipping silently. If the workspace's `winit` pin
ever needs to move ahead of what `ui-events-winit` supports (it pins
`winit ^0.30`), the escape hatch is a `[patch.crates-io]` pin at the
upstream `ui-events-winit` git repo (plus adding that repo to `deny.toml`'s
`sources.allow-git`), not reviving a hand table — see the dependency comment
in `Cargo.toml`.

**Survey:** [`.rust-studio/specs/1092-winit-physical-key-map/survey.md`](../../.rust-studio/specs/1092-winit-physical-key-map/survey.md)
— the market/reference survey behind this decision (`ui-events-winit`
coverage measurement, Flutter's generated `PhysicalKeyboardKey` catalog as
the completeness precedent, and the three options evaluated before choosing
production delegation).

**Replacement coverage:** `platforms/winit/events.rs`'s
`keyboard_conversion_tests` module (`every_winit_keycode_maps_to_a_canonical_code`
over all 194 winit 0.30.13 `KeyCode` variants, plus the issue's acceptance-
criteria spot pairs and the location/logical-key spot checks) and
`cross_backend_physical_key_agreement` (winit vs. Win32 vs. AppKit on a
shared physical-key set).
