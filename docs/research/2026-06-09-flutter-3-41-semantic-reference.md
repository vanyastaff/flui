# Flutter 3.41.9 Semantic / A11y — Reference Corpus (2026-06-09)

Caveman file:line map of Flutter 3.41.9 in `.flutter/flutter-master/`. This is the contract source flui ports against. Companion to [[2026-06-09-flui-semantic-current-state]] (flui side) and [[2026-06-09-flui-semantic-a11y-audit]] (synthesis).

## Where it lives

- `C:\Users\vanya\RustroverProjects\flui\.flutter` = Cygwin symlink → `C:\Users\vanya\RustroverProjects\.flutter\flutter-master`
- `flutter-master` = Flutter 3.41.9 master mirror, 184 MB, 15 881 files / 3 696 dirs, **no git, no .gitmodules**, mtime 2026-05-19
- Engine hash: `ade10cafbad7dae826a7641a2cb759f8f8865f52` (per `bin/internal/flutter_packages.version`)
- Pinned to `## Flutter 3.41 Changes` (CHANGELOG.md:33) → 3.41.9 latest

## Total semantic/a11y corpus

- ~202 files, ~82 213 LOC by filename
- Dart 2 502 025 LOC total; C++/Obj-C++/Java ~720K LOC; framework+engine+embedder+tests

## Framework Dart (the semantic contract)

### `packages/flutter/lib/src/semantics/semantics.dart` (7 232 LOC) — the load-bearing file

| Symbol | Line | Notes |
|---|---:|---|
| `class SemanticsTag` | :608 | |
| `class SemanticsData with Diagnosticable` | :1048 | |
| `class SemanticsHintOverrides extends DiagnosticableTree` | :1576 | |
| `class SemanticsProperties extends DiagnosticableTree` | :1634 | |
| **`class SemanticsNode with DiagnosticableTreeMixin`** | **:2773** | **definition**. 16-bit ID space (framework 0..2^16-1, engine gets upper 32 bits) |
| `class SemanticsOwner extends ChangeNotifier` | :4842 | |
| **`class SemanticsConfiguration`** | **:5196** | builder, flags, actions, absorb, role, inputType, etc. |
| `abstract class SemanticsSortKey` | :7004 | |
| `class CustomSemanticsAction` | :736 | |

### `SemanticsConfiguration.absorb` (THE merge contract, `semantics.dart:6790`)
- Merges child config into parent when `!explicitChildNodes`
- Recomputes `_actions` filtered through `_kUnblockedUserActions` if `isBlockingUserActions`
- Merges `_flags`, attrib strings, sortKey, role, inputType, tooltip
- Merges traversal ids
- Merges scroll extent
- Merges heading-level via `_mergeHeadingLevels`

### `packages/flutter/lib/src/semantics/binding.dart` (279 LOC)
- `:23` `mixin SemanticsBinding on BindingBase`
- subscribes to `platformDispatcher.onSemanticsEnabledChanged` / `onSemanticsActionEvent` / `onAccessibilityFeaturesChanged`
- owns `_handleSemanticsEnabledChanged`, exposes `addSemanticsEnabledListener`, `ensureSemantics`, `SemanticsHandle`

### Other framework semantic files
- `packages/flutter/lib/src/semantics/semantics_event.dart` (238 LOC) — `SemanticsEvent` + `AnnounceSemanticsEvent` + `TooltipSemanticsEvent` (the protocol sent to platform)
- `packages/flutter/lib/src/semantics/semantics_service.dart` (104 LOC) — `SemanticsService.requestSemanticsEnabled` (legacy probe)
- `packages/flutter/lib/src/semantics/debug.dart` (12 LOC) — `debugDisableSemantics` toggle
- `packages/flutter/lib/semantics.dart` (24 LOC) — public re-export

## Engine ui Dart (`dart:ui` surface)

`engine/src/flutter/lib/ui/semantics.dart` (2 273 LOC) — the bitfield contracts.

| Symbol | Line | Shape |
|---|---:|---|
| `class SemanticsAction` | :14 | **26 action bits** (`tap=1<<0` … `collapse=1<<25`): tap, longPress, scrollLeft/Right/Up/Down, increase, decrease, showOnScreen, moveCursorForwardByChar/BackByChar, setSelection, copy, cut, paste, didGain/LoseAccessibilityFocus, customAction, dismiss, moveCursorForwardByWord/BackByWord, setText, focus, scrollToOffset, expand, collapse. Max 1<<31 on web JS mode. |
| **`enum SemanticsRole`** | **:361** | **32 values**: none, tab, tabBar, tabPanel, dialog, alertDialog, table, cell, row, columnHeader, dragHandle, spinButton, comboBox, menuBar, menu, menuItem, menuItemCheckbox, menuItemRadio, list, listItem, form, tooltip, loadingSpinner, progressBar, hotKey, radioGroup, status, alert, complementary, contentInfo, main, navigation, region. Trailing `region` at :555. |
| `enum SemanticsInputType` | :561 | 6 values: none, text, url, phone, search, email. **MISSING IN flui** |
| `class SemanticsFlag` | :588 | **31 flag bits** (`hasCheckedState=1<<0` … `isRequired=1<<30`). |

## Engine C++ peers

`engine/src/flutter/lib/ui/semantics/`:

| File | LOC | Content |
|---|---:|---|
| `semantics_node.h` | 187 | `enum class SemanticsAction` (`:23`, mirrors dart 1:1) + `enum class SemanticsRole` (`:74`, 32 values) + `enum class SemanticsValidationResult` (`:116`, 3 values: None/Valid/Invalid) + `struct SemanticsNode` (`:122`) |
| `semantics_flags.h` | — | `enum SemanticsTristate` (`:13`), `enum SemanticsCheckState` (`:18`), `struct SemanticsFlags` (`:25`), `class NativeSemanticsFlags : public RefCountedDartWrappable<…>` (`:54`) |
| `semantics_update_builder.h/.cc` | 95+166 | `SemanticsUpdateBuilder::updateNode/applyRootNodeUpdate/...` |
| `semantics_update.h/.cc` | 40+47 | batch wire format |
| `string_attribute.h/.cc` | 88+59 | `StringAttribute` (LocaleStringAttribute, SpellOutStringAttribute) |
| `custom_accessibility_action.h/.cc` | 36+13 | `CustomAccessibilityAction` peer |
| `semantics_update_builder_unittests.cc` | 228 | Dart unit test runner surface |

## Embedder ABI (C interface — the platform integration contract)

`engine/src/flutter/shell/platform/embedder/embedder.h`:

| Symbol | Line | Content |
|---|---:|---|
| `kFlutterAccessibilityFeatureDeterministicCursor = 1 << 10` | :115 | `FlutterAccessibilityFeature` enum, 10-bit feature bitmask |
| `typedef enum { kFlutterSemanticsActionTap=1<<0 … kFlutterSemanticsActionCollapse=1<<25 }` | :122 | comment: "Must match the `SemanticsAction` enum in semantics.dart" |
| **DEPRECATED `FlutterSemanticsFlag`** | **:189** | **31-bit enum, FROZEN** (lines 195–276) |
| **CURRENT `FlutterSemanticsFlags` struct** | **:298** | **32 named uint32_t bit fields** + extras: `isMultiline`/`isReadOnly`/`isFocusable`/`isLink`/`isSlider`/`isKeyboardKey`/`isCheckStateMixed`/`hasExpandedState`/`isExpanded`/`hasSelectedState`/`hasRequiredState`/`isRequired` |
| `FlutterSemanticsStringAttributes` typedef | :395 | |
| **MIGRATION SEAM**: `FlutterSemanticsNode2` | **:1574** (`flags: FlutterSemanticsFlag`) + **:1743** (`flags2: FlutterSemanticsFlags*`) | **Dual field, old bitmask + new struct pointer** — this is the embedder pattern for the same surface in transition |
| `kFlutterSemanticsNodeIdBatchEnd` | :1504 | batch sentinel |
| `FlutterSemanticsNode` | :1637 | deprecated struct |
| `FlutterSemanticsNode2` | :1753 | current |
| `embedder_semantics_update.cc` | 426 | C++ side that calls embedder callback |
| `embedder_a11y_unittests.cc` | 966 | embedder a11y tests |

## Per-OS platform bridges

| OS | File(s) | LOC |
|---|---|---:|
| **Android** (Java) | `engine/src/flutter/shell/platform/android/io/flutter/view/AccessibilityBridge.java` + `AccessibilityStringBuilder.java` (109) + `AccessibilityViewEmbedder.java` (620) + `embedding/engine/systemchannels/AccessibilityChannel.java` (220) + `plugin/platform/AccessibilityEventsDelegate.java` (61) + `PlatformViewsAccessibilityDelegate.java` (39) | 4 441 |
| **iOS** (Obj-C++) | `darwin/ios/.../accessibility_bridge.h` (114) + `accessibility_bridge.mm` (383) + `accessibility_bridge_ios.h` (48) + `accessibility_bridge_test.mm` (2 449) + `SemanticsObject.h` (239) + `SemanticsObject.mm` (976) + `SemanticsObject+UIFocusSystem.mm` (245) + `SemanticsObjectTest.mm` (1 420) + `FlutterSemanticsScrollView.{h,mm}` (52+123) + `TextInputSemanticsObject.{h,mm}` (23+501) | 6 573 |
| **macOS** (Obj-C++) | `darwin/macos/.../AccessibilityBridgeMac.{h,mm}` (98+379) + `AccessibilityBridgeMacTest.mm` (319) + `FlutterTextInputSemanticsObject.{h,mm}` (106+226) + `FlutterTextInputSemanticsObjectTest.mm` (70) | 1 198 |
| **Linux** (C++ GLib) | `linux/fl_accessibility_channel.{h,cc}` (62+194) + `fl_accessibility_handler.{h,cc}` (47+84) + `fl_accessibility_handler_test.cc` (233) | 620 |
| **Windows** (C++) | `windows/accessibility_bridge_windows.{h,cc}` (82+212) + `accessibility_bridge_windows_unittests.cc` (406) + `accessibility_plugin.{h,cc}` (43+122) + `accessibility_plugin_unittests.cc` (156) | 1 021 |
| **Fuchsia** (C++) | `fuchsia/flutter/accessibility_bridge.{h,cc}` (275+976) + `accessibility_bridge_unittest.cc` (1 175) | 2 426 |
| **Common** (C++ header-only) | `shell/platform/common/accessibility_bridge.{h,cc}` (296+744) + `accessibility_bridge_unittests.cc` (639) + `test_accessibility_bridge.{h,cc}` (33+26) | 1 738 |
| **Third-party** (Windows UIA base) | `third_party/accessibility/base/win/enum_variant.{h,cc,unittest.cc}` + `scoped_variant.{h,cc,unittest.cc}` + `variant_util.h` + `variant_vector.{h,cc,unittest.cc}` | UIA COM helpers |

## Web — PR #168653 refactor (25 files, 6 537 LOC)

`engine/src/flutter/lib/web_ui/lib/src/engine/semantics/` — production model for manual ARIA path.

| Path | Purpose |
|---|---|
| `semantics.dart:632` | `abstract class SemanticRole` (per-role `apply()` to DOM) |
| `semantics.dart:1147` | `abstract class SemanticBehavior` (cross-role `update()` to DOM) |
| `semantics_helper.dart:69` | `abstract class SemanticsEnabler` |
| `semantics_helper.dart:133` | `DesktopSemanticsEnabler` |
| `semantics_helper.dart:235` | `MobileSemanticsEnabler` |

### Per-role ARIAElement files (each `extends SemanticRole`)

| File | Classes |
|---|---|
| `alert.dart` | `SemanticAlert`, `SemanticStatus` |
| `checkable.dart` | `SemanticRadioGroup`, `SemanticCheckable` + `Checkable`/`Selectable` behaviors |
| `disable.dart` | `CanDisable` |
| `expandable.dart` | `Expandable` |
| `focusable.dart` | `Focusable` |
| `form.dart` | `SemanticForm` |
| `header.dart` | `SemanticHeader` |
| `heading.dart` | `SemanticHeading` |
| `image.dart` | `SemanticImage` |
| `incrementable.dart` | `SemanticIncrementable` |
| `label_and_value.dart` | `LabelAndValue` (441-line behavior) |
| `landmarks.dart` | `SemanticComplementary`, `SemanticContentInfo`, `SemanticMain`, `SemanticNavigation`, `SemanticRegion` |
| `link.dart` | `SemanticLink` |
| `list.dart` | `SemanticList`, `SemanticListItem` |
| `live_region.dart` | `LiveRegion` |
| `menus.dart` | `SemanticMenu`, `SemanticMenuBar`, `SemanticMenuItem`, `SemanticMenuItemCheckbox`, `SemanticMenuItemRadio` |
| `platform_view.dart` | `SemanticPlatformView` |
| `progress_bar.dart` | `SemanticsProgressBar`, `SemanticsLoadingSpinner` |
| `requirable.dart` | `Requirable` |
| `route.dart` | `SemanticRouteBase` → `SemanticRoute`, `SemanticDialog`, `SemanticAlertDialog` + `RouteName` behavior |
| `scrollable.dart` | `SemanticScrollable` |
| `table.dart` | `SemanticTable`, `SemanticCell`, `SemanticRow`, `SemanticColumnHeader` |
| `tabs.dart` | `SemanticTab`, `SemanticTabPanel`, `SemanticTabList` |
| `tappable.dart` | `SemanticButton` (role) + `Tappable` (behavior) |
| `text_field.dart` | `SemanticTextField` + `SemanticsTextEditingStrategy` |

### Web tests
- `engine/src/flutter/lib/web_ui/test/engine/semantics/semantics_test.dart` (6 537 LOC)
- `semantics_tester.dart` (316 LOC) + 9 smaller: announcement, api, auto_enable, helper, multi_view, placeholder_enable, text, tappable, scrollable, selectable, text_field
- Total web test corpus: ~9 300 LOC

## flutter_test / flutter_driver / devicelab

### Test infrastructure
- `packages/flutter_test/lib/src/accessibility.dart` (825 LOC) — `AccessibilityGuideline` enum + `AccessibilityGuidelineViolation` + `AccessibilityController.evaluate(...)` (color contrast, tap-target size, label presence)
- `packages/flutter_test/lib/src/controller.dart:65` — `class SemanticsController` (used by widget tests to dump/enable semantics)
- `packages/flutter_test/lib/src/binding.dart` (3 256 LOC) — `AutomatedTestWidgetsFlutterBinding` with `_kSemanticsHandle` plumbing

### flutter_test test files
- `packages/flutter_test/test/accessibility_test.dart` (~30 KB)
- `accessibility_window_test.dart`
- `multi_view_accessibility_test.dart`
- `pre_test_message_accessibility_test.dart`
- `semantics_finder_test.dart`
- `semantics_checker/pipeline_owner_semantics_handle_test.dart`
- `semantics_checker/semantics_binding_semantics_handle_test.dart`
- `test_fixes/.../semantics_controller.{dart,dart.expect}`
- `fix_data/.../fix_semantics_controller.yaml`

### packages/flutter/test/semantics/ (8 files)
- `custom_semantics_action.dart`
- `semantics_binding_set_semantics_tree_enabled.dart`
- `semantics_binding_set_semantics_tree_enabled_1.dart`
- `semantics_binding.dart`
- `semantics_node_send_event.dart`
- `semantics_owner.dart`
- `semantics_service.dart`
- `semantics_test.dart`
- `semantics_update.dart`

### packages/flutter/test/widgets/semantics_*.dart (~30 files)
- `semantics_1..11_split_test.dart` (config absorb scenarios)
- `semantics_checks.dart`
- `semantics_child_configs_delegate.dart`
- `semantics_clipping.dart`
- `semantics_debugger.dart`
- `semantics_event.dart`
- `semantics_keep_alive_offstage.dart`
- `semantics_merge.dart`
- `semantics_refactor_regression.dart`
- `semantics_role_checks.dart`
- `semantics_tester.dart`
- `semantics_tester_generate_test_semantics_expression.dart`
- `semantics_traversal.dart`
- `semantics_zero_surface_size.dart`
- `scrollable_semantics.dart`
- `scrollable_semantics_traversal_order.dart`
- `sliver_semantics.dart`
- `sliver_semantics_widget.dart`
- `text_semantics.dart`
- `gesture_detector_semantics.dart`
- `implicit_semantics.dart`
- `list_view_semantics.dart`
- `accessibility_evaluations.dart`
- `accessibility_evaluations_service_extension.dart`

### dev/ — a11y test infra
- `packages/flutter_driver/lib/src/common/semantics.dart` (46 LOC) — driver-side `SemanticsClient`
- `dev/devicelab/lib/framework/talkback.dart` (82 LOC) — Android TalkBack test harness
- `dev/integration_tests/android_semantics_testing/{lib,test}/` — semantic trees-on-device probe library
- `dev/benchmarks/complex_layout/test_driver/semantics_perf/` — perf bench
- `dev/benchmarks/macrobenchmarks/lib/src/web/bench_material_3_semantics.dart` — web a11y perf
- `dev/devicelab/bin/tasks/{android_semantics_integration_test,complex_layout_semantics_perf,flutter_gallery__transition_perf_with_semantics}.dart`
- `dev/a11y_assessments/test/accessibility_guideline_test.dart` — gallery-driven a11y audit
- `dev/integration_tests/flutter_gallery/test/accessibility_test.dart` + `test_driver/transitions_perf_e2e_with_semantics.dart`

## Docs (sparse on architecture)

- `docs/platforms/desktop/windows/Accessibility-on-Windows.md` — Windows UIA integration doc
- `docs/contributing/Contributor-access.md` — ACL for a11y
- `dev/a11y_assessments/` — internal widget-by-widget a11y audits (concentrated, not architecture)
- **NO `docs/semantics/*` index, NO top-level a11y architecture doc**

## Version-specific notes (3.41)

- ARIA refactor PR #168653 (May 2025) → in 3.27+ stable, fully landed by 3.41
- 3.36 deprecated `focusable` in favor of `focused` (input focus vs accessibility focus)
- 32-value `SemanticsRole` enum is stable since 3.27
- Embedder migration seam (`flags` deprecated + `flags2` struct) is the current pattern

## Known open issues (transcribed from research)

- Flutter web iOS WebKit regression #179784 (Dec 2025, still open) — `ensureSemantics()` + `ListView.builder` 100×`SizedBox` causes severe scroll lag on iOS Safari; Android Chrome/Desktop unaffected
- Android WebView delay #175465 (Sept 2025) — Android WebView doesn't propagate AT state until first user interaction
- Web perf issue #150234 (Jun 2024 → Feb 2025) — fixed by PR #161195 from 2.6ms in 6ms frame → 0.3ms in 4ms frame (~5x)
