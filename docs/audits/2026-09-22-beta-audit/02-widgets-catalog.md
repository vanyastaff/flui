# FLUI Widget/Material/Cupertino Catalog Inventory

Scope: `crates/flui-widgets` (186 .rs files), `crates/flui-material` (42 files),
`crates/flui-cupertino` (11 files), plus `crates/flui-animation` and
`crates/flui-view` for the underlying view/state model.

## 1. Catalog by category (flui-widgets unless noted)

- **Layout**: Container-equivalent (via Padding/DecoratedBox/Align/ConstrainedBox
  composition — no single `Container` struct, but the `Container` name is used in
  examples as a builder helper — see below), Row/Column via `Flex`, `Stack`/`Positioned`,
  `Padding`, `Center`, `Align`, `SizedBox`, `ConstrainedBox`, `UnconstrainedBox`,
  `OverflowBox`/`SizedOverflowBox`, `LimitedBox`, `AspectRatio`,
  `FractionallySizedBox`, `FractionalTranslation`, `Transform`, `RotatedBox`,
  `Baseline`/`IgnoreBaseline`, `IntrinsicWidth`/`IntrinsicHeight`, `Table`,
  `Flow`, `FittedBox`, `Wrap`, `Flexible`/`Spacer`, `ListBody`,
  `CustomSingleChildLayout`/`CustomMultiChildLayout`, `LayoutBuilder`,
  `PreferredSize`.
- **Scrolling / slivers**: `ListView` (incl. `.builder`-style constructor),
  `GridView`, `PageView`, `SingleChildScrollView`, `CustomScrollView`,
  `Scrollable`, `ScrollController`, `ScrollPosition(Scope)`, `ScrollPhysics`,
  `Scrollbar`, `RefreshIndicator`, `Viewport`, `SliverList`,
  `SliverFixedExtentList`, `SliverGrid`, `SliverToBoxAdapter`, `SliverPadding`,
  `SliverOpacity`, `SliverOffstage`, `SliverIgnorePointer`,
  `SliverFillViewport`, `SliverFillRemaining`, `SliverMainAxisGroup`,
  `SliverPersistentHeader`. Material adds `SliverAppBar`.
- **Text / input**: `Text`, `RichText`, `DefaultTextStyle`,
  `TextEditingController`, `EditableText` (single-line only — see §3),
  `flui_widgets::TextField` (plain, untthemed) and `flui_material::TextField`
  (M3-decorated, via `InputDecorator`).
- **Navigation / routing**: `Navigator`, `Route`/`PageRoute`/`ModalRoute`/
  `TransitionRoute`/`OverlayRoute`, `NamedRoute`/`RouteRequest`/`RouteKey`
  (typed keys, but no name→route generator registry), `History`/`LocalHistory`,
  `PopScope`, `NavigatorObserver`, back-gesture support, `Hero`/`HeroController`/
  `HeroFlight`. Material adds `AppBar`, `BackButton`, `Drawer`, `Scaffold`,
  `ScaffoldMessenger`, `Tabs`/`TabController`/`TabBarView`, `NavigationBar`.
  Cupertino adds `CupertinoNavigationBar`, `CupertinoTabBar`/`CupertinoTabScaffold`/
  `CupertinoTabController`, `CupertinoPageScaffold`, and a `route.rs` (page
  transitions).
- **Gestures**: `GestureDetector`, `GestureArenaScope`, `Listener`,
  `MouseRegion`, `AbsorbPointer`/`IgnorePointer`, `Draggable`/`DragTarget`,
  `Dismissible`, `InteractiveViewer` + `TransformationController`,
  `Actions`/`Shortcuts`, `Focus`/`FocusScope`, `Offstage`, `VisibilityGate`/
  `Visibility`, `MetaData`.
- **Animation**: Implicit — `AnimatedContainer`, `AnimatedPadding`,
  `AnimatedOpacity`, `AnimatedSize`, `AnimatedAlign`, `AnimatedSwitcher`,
  `ImplicitlyAnimatedWidget` base, `TickerMode`, `VsyncScope`. Explicit —
  `AnimatedBuilder`, `FadeTransition`, `ScaleTransition`, `SlideTransition`,
  `RotationTransition`. Underlying engine (`flui-animation` crate, separate
  from flui-widgets): `AnimationController`, `Tween`/typed tweens, `Curve`/
  `CurvedAnimation`, spring `Simulation`, `compound`/`reverse`/`proxy`
  animations, a vsync registry. **Missing**: `AnimatedCrossFade`,
  `AnimatedList`/`AnimatedListView`, `AnimatedPositioned`,
  `AnimatedDefaultTextStyle`, `AnimatedTheme`, `AnimatedPhysicalModel`.
- **Images / assets**: `Image`, `NetworkImage`, `AssetImage`, an image
  `provider`/`resolve`/`decode_cache`/`cache_key` pipeline (own asset crate
  `flui-assets` backs it with tokio for fs/io).
- **Theming**: `InheritedTheme`, Material `Theme`/`ThemeData`/`ColorScheme`/
  `Typography`/`TextTheme`/`StateColor`/`Shape`, Cupertino `CupertinoTheme`/
  `CupertinoThemeData`/`CupertinoDynamicColor`/`CupertinoColors`/
  `CupertinoTextThemeData`. `IconTheme`/`IconThemeData` in flui-widgets.
- **Dialogs / overlays**: `Overlay`/`OverlayEntry`/`Theater` (the overlay
  stack) in flui-widgets; Material `Dialog`, `SnackBar` + `ScaffoldMessenger`.
  **Missing**: `BottomSheet`/`showModalBottomSheet` equivalent, `Tooltip`,
  `PopupMenuButton`, `showDatePicker`/`showTimePicker`.
- **Forms / validation**: `TextEditingController` exists but there is no
  `Form`/`FormField`/`TextFormField`/`FormState.validate()` layer anywhere in
  the three crates.
- **Accessibility**: a `semantics` module exists in flui-widgets (currently
  just `mod.rs`, i.e. a stub/aggregator — no widget-level semantics properties
  such as `Semantics`/`MergeSemantics`/`ExcludeSemantics` were found as
  distinct public types).
- **MediaQuery / app shell**: `MediaQuery`, `SafeArea`, `Directionality`,
  `Localizations`/`WidgetsLocalizations`/`LocaleResolution`, `WidgetsApp`.
  Material `MaterialApp`-equivalent is `app.rs`/`material.rs` (`Material`
  widget + app scaffold). Cupertino `CupertinoApp`.
- **State management primitives** (flui-view / flui-foundation, not
  flui-widgets, but load-bearing for "how do you build an app"):
  `StatefulView` trait with `ElementState::set_state`/`set_state_scheduled`
  (setState-equivalent), `InheritedView` (InheritedWidget-equivalent),
  `ValueNotifier`/`ChangeNotifier`/`Listenable` (flui-foundation),
  `ValueListenableBuilder`, and `async_builders.rs` providing Future/Stream
  snapshot builders (FutureBuilder/StreamBuilder-equivalent, generic over any
  `std::future::Future`/`futures::Stream`, not tied to a specific async
  runtime type).
- **Misc widgets present**: `ColoredBox`, `DecoratedBox`, `RepaintBoundary`,
  `Opacity`, `CustomPaint`, `ClipRect`/`ClipOval`/`ClipRRect`/`ClipPath`,
  `PhysicalModel`/`PhysicalShape`, `Icon`/`IconData`/`IconTheme`.
- **Material widgets present**: `ElevatedButton`, `TextButton`,
  `OutlinedButton`, `FilledButton`, `IconButton`, `FloatingActionButton`,
  `ButtonStyle`/`ButtonStyleButton`, `Checkbox`, `Radio`, `Switch`, `Chip`,
  `Card`, `Divider`, `ListTile`, `DataTable`, `InkWell`, `AppBar`,
  `FlexibleSpaceBar`, `SliverAppBar`, `Drawer`, `Scaffold`/
  `ScaffoldMessenger`, `SnackBar`, `Dialog`, `NavigationBar`, `TabBar`/
  `TabBarView`/`TabController`, `InputDecorator`, `TextField`, `BackButton`.
- **Cupertino widgets present** (only 11 files / 15 public types total —
  by far the thinnest catalog): `CupertinoApp`, `CupertinoButton`,
  `CupertinoNavigationBar`, `CupertinoTabBar`/`CupertinoTabScaffold`/
  `CupertinoTabController`, `CupertinoPageScaffold`, `CupertinoTheme`/
  `CupertinoThemeData`/`CupertinoDynamicColor`/`CupertinoColors`,
  `CupertinoTextThemeData`, a page-transition `route.rs`. **Missing entirely**:
  `CupertinoSwitch`, `CupertinoSlider`, `CupertinoTextField`,
  `CupertinoActivityIndicator`, `CupertinoAlertDialog`/`CupertinoActionSheet`,
  `CupertinoPicker`/`CupertinoDatePicker`/`CupertinoTimerPicker`,
  `CupertinoSegmentedControl`/`CupertinoSlidingSegmentedControl`,
  `CupertinoContextMenu`, `CupertinoListTile`, `CupertinoSearchTextField`,
  `CupertinoScrollbar`, `CupertinoRefreshControl`, `CupertinoIcons` set.

## 2. Flutter widget → FLUI status table (~120 most-used)

| Flutter widget | FLUI status | Notes |
|---|---|---|
| Container | Partial | No single `Container` type; composed from Padding/DecoratedBox/Align/ConstrainedBox. Examples use a `Container` builder (widgets_gallery.rs) so ergonomics are close, but it is not a first-class exported struct in the same sense. |
| Row / Column | Present | Built on `Flex` + `row!`/`column!` macros. |
| Stack / Positioned | Present | |
| SizedBox | Present | |
| Padding | Present | |
| Align / Center | Present | |
| ConstrainedBox / UnconstrainedBox | Present | |
| OverflowBox / SizedOverflowBox | Present | |
| LimitedBox | Present | |
| AspectRatio | Present | |
| FractionallySizedBox / FractionalTranslation | Present | |
| Transform | Present | |
| RotatedBox | Present | |
| Baseline | Present | |
| IntrinsicWidth / IntrinsicHeight | Present | |
| Table | Present | |
| Flow | Present | |
| FittedBox | Present | |
| Wrap | Present | |
| Flexible / Expanded / Spacer | Partial | `Flexible`/`Spacer` present; no distinct `Expanded` type found (may be `Flexible` with fit=tight, not confirmed as separate export). |
| LayoutBuilder | Present | |
| ListView | Present | builder-style ctor at `scroll/list_view.rs:129` |
| ListView.builder | Present | via lazy `builder` constructor |
| GridView | Present | |
| CustomScrollView | Present | |
| SliverList / SliverGrid / SliverFixedExtentList | Present | |
| SliverAppBar | Present | flui-material |
| SliverToBoxAdapter / SliverPadding / SliverFillViewport / SliverFillRemaining / SliverMainAxisGroup / SliverPersistentHeader | Present | |
| SingleChildScrollView | Present | |
| Scrollbar | Present | |
| RefreshIndicator | Present | |
| PageView | Present | |
| Text | Present | |
| RichText | Present | |
| TextField | Present | plain (flui-widgets) + Material-decorated (flui-material) |
| TextFormField | Missing | no `Form`/`FormField` layer at all |
| Form | Missing | no validation framework |
| Navigator | Present | imperative push/pop, `RouteResult`, `PopScope`, observers |
| Named routes (`Navigator.pushNamed`) | Partial | `RouteRequest`/`RouteKey`/`NamedRoute` types exist but no name→route-generator registry (open issue #542) |
| Router / declarative Pages API / GoRouter-like | Missing | issue #542 explicitly: "declarative Pages API is absent" |
| MaterialApp | Present-ish | `flui_material` app.rs/material.rs (no single `MaterialApp` struct name confirmed, but the app-shell role is filled) |
| CupertinoApp | Present | |
| Scaffold | Present | |
| AppBar | Present | |
| Drawer | Present | |
| BottomNavigationBar | Missing | only `NavigationBar` (M3) found |
| NavigationRail | Missing | |
| TabBar / TabBarView / TabController | Present | |
| Dialog | Present | Material `Dialog` |
| BottomSheet / showModalBottomSheet | Missing | |
| SnackBar | Present | with `ScaffoldMessenger` |
| Image / Image.network | Present | `Image`, `NetworkImage`, `AssetImage` |
| Icon | Present | |
| IconButton | Present | |
| ElevatedButton / TextButton / OutlinedButton / FilledButton | Present | full button family |
| Checkbox / Radio / Switch | Present | |
| Slider | Missing | not found in any of the three crates |
| DropdownButton | Missing | |
| PopupMenuButton | Missing | |
| DatePicker / TimePicker (showDatePicker/showTimePicker) | Missing | |
| Tooltip | Missing | |
| Card | Present | |
| Chip | Present | |
| DataTable | Present | |
| ListTile | Present | |
| ExpansionTile | Missing | |
| Stepper | Missing | |
| ReorderableListView | Missing | |
| Dismissible | Present | flui-widgets |
| Draggable / DragTarget | Present | |
| InteractiveViewer | Present | |
| Hero | Present | full `Hero`/`HeroController`/`HeroFlight` with extensive tests |
| PageView | Present | (dup, see above) |
| AnimatedContainer / AnimatedPadding / AnimatedOpacity / AnimatedSize / AnimatedAlign / AnimatedSwitcher | Present | |
| AnimatedCrossFade | Missing | |
| AnimatedList | Missing | |
| Explicit Transitions (Fade/Scale/Slide/Rotation) | Present | |
| AnimatedBuilder | Present | |
| AnimationController / Tween / Curves | Present | separate `flui-animation` crate, includes springs |
| CustomPaint | Present | |
| ClipRect / ClipOval / ClipRRect / ClipPath | Present | |
| Opacity | Present | |
| MediaQuery | Present | |
| Theme / ThemeData / ColorScheme | Present | |
| SafeArea | Present | |
| Directionality / Localizations | Present | |
| FutureBuilder / StreamBuilder | Present | generic async snapshot builders (`async_builders.rs`) |
| ValueListenableBuilder | Present | |
| InheritedWidget | Present | `InheritedView` trait |
| ChangeNotifier / ValueNotifier | Present | in `flui-foundation`, used pervasively (text controller, scroll, tabs, transformation controller) |
| GestureDetector | Present | |
| Focus / FocusScope | Present | |
| Shortcuts / Actions | Present | keybinding infra exists, but keyboard shortcuts inside `EditableText` itself are only "must bubble, not consumed" guards (Ctrl+S/Ctrl+Z/Cmd+C), not a shortcut-authoring surface for app code yet |
| Semantics (accessibility tree) | Stub | `semantics` module is effectively empty (`mod.rs` only) at the flui-widgets layer |
| Offstage / Visibility | Present | |
| PhysicalModel / PhysicalShape | Present | |

Overall estimated coverage against the ~120-item list: roughly **60-65% present**, **~10% partial**, **~25-30% missing** — concentrated missing areas are form validation, several common Material input/overlay controls (Slider, DropdownButton, PopupMenuButton, Tooltip, BottomSheet, date/time pickers, ExpansionTile, Stepper, ReorderableListView, BottomNavigationBar/NavigationRail), and nearly all of Cupertino beyond app-shell/navigation/button.

## 3. Text input depth (issues #540, #1131, #1132)

Read: `crates/flui-widgets/src/text/editable_text.rs` (3595 lines),
`crates/flui-widgets/src/text/controller.rs` (2173 lines),
`crates/flui-widgets/src/text/text_field.rs` (287 lines),
`crates/flui-material/src/text_field.rs`.

- **Selection**: `TextEditingController` models selection as a collapsed-vs-
  extended range mirroring Flutter's `TextSelection` (doc comment explicitly
  cites this design). Present, but issue #540 ("Finish EditableText
  selection, multiline, and obscured input", open, P-high) says selection
  *painting*, drag selection, and shift-click are still gaps at the widget
  layer.
- **Multiline**: **Not implemented.** `editable_text.rs` module doc says
  explicitly: "Multi-line — newlines are inserted as literal characters but
  line wrapping, multi-line layout, and vertical scrolling are not
  implemented." Both `flui_widgets::TextField` and `flui_material::TextField`
  are single-line by construction per their doc comments.
- **IME composing**: Present — `ComposingState`/composing-range handling
  exists in the controller (doc comments describe it explicitly), and issue
  #540's problem statement says "EditableText supports core IME composition"
  already, i.e. this part is done; the gap is the surrounding UX
  (selection painting, multiline, drag).
- **Clipboard**: Referenced/wired (copy shortcut is recognized as
  "must bubble" in a keyboard test — meaning it's routed rather than
  swallowed by the text buffer), full clipboard cut/copy/paste plumbing not
  independently verified beyond that.
- **Grapheme clusters**: **Not done.** Issue #1131 ("make editable character
  operations grapheme-cluster aware", open, P2) states current operations
  (`backspace`, `delete_forward`, `move_caret_left/right`, password masking)
  advance by Unicode scalar (`char`), not extended grapheme cluster — i.e.
  editing emoji/combining-character text can break visually.
- **Obscure (password) text**: Present — `obscure_text` field/builder method
  and an `obscure()` masking function exist with careful documentation about
  offset-mapping correctness, but per #1131 the masking itself is scalar-
  based, not grapheme-based.
- **Undo/redo**: **Not implemented.** Only reference found is a test
  asserting "Ctrl+Z must undo, not type" — i.e. the key event is *not*
  consumed by the text field (bubbles up for the host/app to interpret), not
  that FLUI performs undo itself. No undo-stack code found.
- **Word boundaries / double-tap word select**: Issue #1132 (open, P3→P2)
  says `get_word_boundary` returns "a run of non-ASCII-whitespace bytes, not
  Unicode word boundaries" and that double-tap word selection is "still
  deferred at the widget layer" — so this is explicitly not production-ready
  for non-English text.
- **Keyboard shortcuts**: A `Shortcuts`/`Actions` widget pair exists at the
  framework level (flui-widgets/interaction), but inside `EditableText` the
  only shortcut-related logic found is negative (recognizing and *not*
  swallowing Ctrl+S/Ctrl+Z/Cmd+C style chords) rather than a rich text-editing
  shortcut map (word-left/right, select-line, etc. not confirmed present).

**Bottom line**: EditableText is a serious, heavily-documented single-line
text input with real IME composing and selection *data model*, but three
open, self-filed issues (#540, #1131, #1132) block it from Flutter-grade UX:
no multiline, no grapheme-safe editing, no Unicode word boundaries, and no
undo/redo. Any real app needing a multi-line text area, a chat compose box,
or correct emoji/CJK editing will hit a wall today.

## 4. State management

- **Local component state**: `StatefulView` trait (`flui-view/src/view/stateful.rs`)
  is FLUI's `StatefulWidget` analog; its element exposes
  `set_state`/`set_state_scheduled` (`flui-view/src/element/unified.rs:426,447`)
  as the `setState()` equivalent, scheduling a rebuild through the element
  owner.
- **Cross-tree data**: `InheritedView` trait (`flui-view/src/view/inherited.rs`)
  plus `inherited_access.rs`/`inherited_dependencies.rs` machinery in
  `flui-view` is the `InheritedWidget` analog; used by Material's
  `InheritedTheme`, Cupertino's page scaffold, `TabController` scope, and
  `Focus`.
- **Observable values**: `ValueNotifier`/`ChangeNotifier`/`Listenable` live in
  `flui-foundation::notifier` and are used pervasively — `TextEditingController`,
  `ScrollController`/`RefreshIndicator`, `TransformationController`,
  `TabController`, `ModalRoute`/`Navigator`/`OverlayEntry`, and a
  `ValueListenableBuilder` widget (`crates/flui-widgets/src/value_listenable_builder.rs`)
  to rebuild on notification.
- **Async data**: `crates/flui-widgets/src/async_builders.rs` provides
  Future/Stream "snapshot" builders (`FutureBuilder`/`StreamBuilder` analogs,
  named generically) that rebuild from the latest resolved state — explicitly
  documented as avoiding "Flutter's worst [FutureBuilder foot-gun]" pattern.
- **App-level / global state**: No dedicated state-management library
  (no Provider/Riverpod/Bloc equivalent) was found in these three crates.
  Apps would compose `InheritedView` + `ChangeNotifier` themselves, same as
  vanilla Flutter without a package.
- **Async runtime**: Tokio is the framework's own async runtime, not just an
  example dependency — `flui-app/Cargo.toml` depends on
  `tokio = { features = ["rt-multi-thread", "sync", "time"] }` plus
  `tokio-util`; `flui-assets/Cargo.toml` uses tokio for fs/io-util; `flui-build`
  and `flui-platform` also depend on tokio. So app authors doing async I/O are
  expected to ride the same tokio runtime the framework itself is built on,
  not bring their own executor.

## 5. Routing/navigation depth

- `Navigator` (`crates/flui-widgets/src/navigator/navigator.rs`, ~2600+ lines)
  supports imperative push/pop, typed `NavigatorRoute` results
  (`RouteResult<R::Output>`), `PopScope`, `NavigatorObserver`, local history
  entries, back-gesture handling, and a full `Hero` transition subsystem with
  its own dedicated test files (`hero_tests.rs`, `hero_flight_tests.rs`,
  `hero_gesture_tests.rs`, `hero_controller_tests.rs`).
- Named routes: `named_route.rs` defines `RouteRequest`, `RouteKey<T>`,
  `KeyedSettings<T>`, `GeneratedRoute` — typed scaffolding for named/keyed
  routes exists, but per open issue #542 there is no realm-owned router
  registry that resolves a route *name* to a route generator (no
  `Navigator.pushNamed`-equivalent end-to-end flow yet).
- Declarative Pages API (Router/Router.pages, GoRouter-style): **Missing**.
  Issue #542 states plainly: "declarative Pages API is absent... Model
  declarative pages as a keyed desired history reconciled against current
  routes" is listed as *future* recommended design, not done.
- Deep links: No OS-level URL-scheme/deep-link intent handling found. The
  closest thing is `WidgetsApp`'s "deep-link analog" — seeding the initial
  route stack programmatically at startup (`seed_initial` calls per module
  docs), which is not the same as parsing an incoming URI at runtime.

## 6. Examples inventory (`examples/`)

| Example | What it demonstrates |
|---|---|
| `hello_world.rs` | Minimal render pipeline smoke test |
| `widgets_gallery.rs` | Curated widgets-catalog showcase (Container/Column/Row/ClipOval/Opacity) — see snippet below |
| `material_demo/` | Material widget showcase (`main.rs` + `tree.rs`) |
| `cupertino_demo/` | Cupertino widget showcase |
| `android_app/`, `android_demo/`, `android_scene/` | Android platform integration |
| `ios_demo.rs` | iOS platform integration |
| `desktop_scene/` | Desktop windowing scene |
| `web_counter/`, `web_demo/` | Wasm/web target counter demos |
| `hot_reload_counter/`, `hot_reload_lifecycle_fixture/` | Hot-reload plumbing (separate `logic`/`host`/`types` crates, `flui.toml`) |
| `vertical_slice_demo/` | End-to-end pipeline slice with a frame histogram (perf-oriented, not app-oriented) |
| `sliver_demo.rs` | Sliver/scroll showcase |
| `animated_box_app.rs`, `colored_box_app.rs`, `color_filter_demo.rs`, `filter_demo.rs`, `painting_demo.rs`, `image_demo.rs` | Rendering/paint feature demos |
| `text_app.rs`, `input_test.rs` | Text/input smoke tests |
| `windows11_demo.rs`, `windows11_features.rs`, `window_features.rs` | Windows platform demos |
| `resize_jitter_probe.rs`, `direct_render.rs`, `scene_render.rs`, `wgpu_window.rs`, `screenshot.rs`, `test_background.rs`, `aa_showcase.rs` | Low-level rendering/graphics probes, not app demos |

**No "real app" example exists** — there is no todo list, notes app, or chat
app in `examples/`. Every example is either a rendering/pipeline probe, a
single-screen widget showcase, or a counter (the classic Flutter starter,
present as `hot_reload_counter` and `web_counter`). This means a developer
evaluating FLUI for a real app has no reference implementation showing
multi-screen navigation, forms, or persisted state together.

## 7. Widgets-gallery ergonomics snippet

From `examples/widgets_gallery.rs` (lines 34-56):

```rust
#[derive(Clone, Debug, StatelessView)]
pub struct Gallery;

impl StatelessView for Gallery {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Container::new()
            .color(Color::rgb(18, 18, 24))
            .padding(EdgeInsets::all(px(24.0)))
            .alignment(Alignment::TOP_LEFT)
            .child(Column::new(column![
                Text::new("FLUI widget gallery"),
                SizedBox::height(16.0),
                Row::new(row![
                    avatar(Color::rgb(229, 57, 53)),
                    SizedBox::width(12.0),
                    avatar(Color::rgb(30, 136, 229)),
                    SizedBox::width(12.0),
                    avatar(Color::rgb(67, 160, 71)),
                ]),
                SizedBox::height(12.0),
                Row::new(row![
                    faded_avatar(Color::rgb(229, 57, 53)),
                    SizedBox::width(12.0),
```

This reads close to Flutter's builder-style declarative tree (`Container()
  .color(...).padding(...).child(Column(...))`), with `column!`/`row!` macros
standing in for Dart's trailing-comma widget-list literals. The derive macro
`#[derive(Clone, Debug, StatelessView)]` plus `impl StatelessView for Gallery`
is FLUI's `StatelessWidget` analog and is comparably terse to Flutter's own
`class Gallery extends StatelessWidget { ... Widget build(...) }`.

## Sources consulted

- `crates/flui-widgets/src/**` (186 files), `crates/flui-material/src/**` (42),
  `crates/flui-cupertino/src/**` (11), `crates/flui-animation/src/**`,
  `crates/flui-view/src/**` (StatefulView/InheritedView/ElementState),
  `crates/flui-foundation` (notifier types).
- `crates/flui-widgets/src/text/{editable_text.rs,controller.rs,text_field.rs}`,
  `crates/flui-material/src/text_field.rs`.
- GitHub issues (via `gh issue view`, repo `vanyastaff/flui`): #540, #1131,
  #1132, #542, #546.
- `examples/` directory listing and `examples/widgets_gallery.rs`.
