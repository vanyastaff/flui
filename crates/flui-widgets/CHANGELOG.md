# Changelog

All notable changes to `flui-widgets` are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: per `docs/release.md` policy.

## [Unreleased]

### Added

- **`TextEditingController::clear()`/`set_text()`**. `set_text` replaces
  the whole buffer, collapses the caret to the end (a deliberate
  divergence from Flutter's `TextEditingController.text` setter, which
  collapses to an off-the-end `-1` sentinel that paints no caret at all —
  see the method's own doc), and clears any active composing region;
  `clear()` is defined in terms of it. No-op without notifying when the
  value is unchanged.
- **`EditableText::on_submitted(Fn(&str))`**, forwarded through
  `RawTextField::on_submitted`. Fires on Enter while focused — never while
  composing, never on a command chord, never on Shift+Enter (reserved for
  a future multiline newline), never twice for one held key.
- **`SubmitCallback`** (`Rc<dyn Fn(&str)>`) exported at the crate root, the
  shared alias `EditableText::on_submitted`/`RawTextField::on_submitted`
  (and `flui_material::TextField::on_submitted`) all use.
- **Word-boundary caret/selection/delete**: `TextEditingController` gained
  `move_caret_word_left`/`move_caret_word_right`,
  `extend_selection_word_left`/`extend_selection_word_right`, and
  `delete_word_backward`/`delete_word_forward` — UAX #29 word segments
  (`unicode-segmentation`), not ASCII whitespace runs. `EditableText`
  wires them to Alt (macOS/iOS's Option) or Control (every other
  platform) + Left/Right/Backspace/Delete, per-platform rather than
  accepting both chords everywhere (Flutter's own
  `DefaultTextEditingShortcuts` binds a different modifier per platform
  too), composing with Shift the same way character movement already
  does. See the `TextEditingController` type doc's `# Word unit` section
  and `ARCHITECTURE.md`'s Mapping decision #19 for the forward/backward
  asymmetry and the known Thai/Lao/Khmer/Myanmar/Chinese/Japanese
  segmentation limitations.
- **Double-tap word selection**: `GestureDetector` gained
  `on_double_tap_down` (Flutter parity:
  `DoubleTapGestureRecognizer.onDoubleTapDown`), fired at the second
  contact's own DOWN rather than waiting for it to also lift.
  `EditableText` composes a `GestureDetector` around its existing
  `Listener`-based pointer handlers to widen the caret into the enclosing
  word on double-tap, via `TextLayout::get_word_boundary`
  (`flui-painting`) — the same UAX #29 machinery the word-jump modifier
  uses one layer down through a separate, independently-tie-broken
  implementation (see the Mapping decision for why). See
  `ARCHITECTURE.md`'s Mapping decision #20.
- Initial `flui-widgets` Core.1 vertical-slice catalog.
- Layout family: `Padding`, `Align`, `Center`, `SizedBox`, `ConstrainedBox`, `LimitedBox`, `Transform`, `AspectRatio`, `Baseline`, `FittedBox`, `FractionallySizedBox`, `FractionalTranslation`.
- Flex/stack family: `Row`, `Column`, `Flex`, `Expanded`, `Flexible`, `Stack`, `Positioned`.
- Paint/effect family: `ColoredBox`, `DecoratedBox`, `Opacity`, `ClipRect`, `ClipRRect`, `ClipOval`, `RepaintBoundary`.
- Scrolling family: `SingleChildScrollView`, `ListView`, `SliverFixedExtentList`, `SliverOpacity`, `SliverPadding`, `SliverToBoxAdapter`, `Viewport`.
- Pointer family: `Listener` (raw pointer routing with `HitTestBehavior`), `GestureDetector` (`on_tap` plus `on_pan_start`/`on_pan_update`/`on_pan_end`, where recognizers compete in the presentation binding's shared arena).
- Modifiers: `IgnorePointer`, `AbsorbPointer`, `Offstage`.
- Text widget: `Text` over `RenderParagraph`.
- Transition family: `FadeTransition`, `ScaleTransition`, `RotationTransition` driven by `flui-animation`.
- Stateful widget harness and 64 integration tests covering layout parity, `setState`, scroll, gestures, transitions, and composition.

### Changed

- **`TextField` renamed to `RawTextField`** (and `TextFieldState` to
  `RawTextFieldState`) — a breaking rename, sanctioned pre-1.0, so the
  facade's `flui::prelude` can give `TextField` one unconditional meaning
  (`flui_material::TextField`) instead of shadowing it feature-dependently.
  See the root `CHANGELOG.md` and `ARCHITECTURE.md`'s `## Mapping
  decisions` for the full history.
- Widget-visible consequences of `flui-animation`'s zero-duration synchronous
  settle (issue #1171): an implicitly-animated widget (`AnimatedContainer`,
  `AnimatedOpacity`, …) retargeted with `Duration::ZERO` now lays out the new
  target on the SAME pump that observes the widget's new configuration,
  instead of one frame later (`ImplicitController::restart_from_zero`'s
  `forward_from(Some(0.0))` settles at the call). A `Navigator` route pushed
  or popped with `transition_duration(Duration::ZERO)` finalizes its POP
  synchronously too: `handle_pop` reads `finished_when_popped()` in the same
  call `did_pop` returns from, so the entry disposes with no `Popping` park
  and no pump in between. PUSH does not get the same synchronous settle: the
  underlying `AnimationController` still completes at the call, but
  `RouteLifecycle::Pushing -> Idle` is driven by a `RouteCommand` a
  continuation queues post-flush, so it still needs one pump regardless of
  how quickly the controller itself resolved.

### Fixed

- **A tap on an obscured `EditableText` placed the caret at the wrong
  source offset whenever a source character's byte width differed from
  the obscuring character's** (the default bullet is 3 UTF-8 bytes;
  almost any real password has 1-byte ASCII characters, so this was not
  a corner case). `source_offset_at_global`'s masked→source conversion
  passed `RenderEditable::plain_text()` — the MASKED string on an
  obscured field — as the `source` argument to
  `source_offset_for_masked_offset`, which needs the actual source
  string to walk its (differently-sized) grapheme clusters; walking the
  masked string's own uniform-width clusters instead just echoed the
  masked offset back, unconverted. No caret-placement test on an
  obscured field previously existed to catch it. Found while auditing
  the analogous double-tap-word-selection code added earlier in this
  release for the same bug (caught there before merge); fixed in both
  places, both now take the controller's source text as an explicit
  parameter instead of reading it off the render object.
