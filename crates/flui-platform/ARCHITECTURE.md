# flui-platform Architecture

The contract half of the platform layer ([ADR-0082](../../docs/adr/ADR-0082-platform-api-contract-crate.md)).
`flui-native` holds the hosts and does not re-export this crate: every item has
this one path (ADR-0151 §9).

## Invariants

- **No backend types.** No OS (`windows`, `objc2-*`, `android-activity`,
  `ndk`, `web-sys`), winit, AccessKit or tokio type appears in a signature,
  and none of those crates is a dependency. `cargo xtask reach` holds it: tier
  C forbids the OS crates and winit, and this crate's own `reach-forbid` adds
  `accesskit` and `tokio`. `flui-native`'s `allowed-dependents` keeps the
  crates above from reaching the backends through a side door.
- **No logging.** A condition the contract meets is the caller's outcome, never a
  log line the contract decides on (ADR-0151 §7): a refusal is an error the
  function returns, a bound applied to untrusted input returns its count
  beside the value for the host that bounds to report (`Opened::bounded`), and
  an edit refused in a text-store grant that runs after its request returned
  goes to the reporter the caller handed in (`commit_composition_in_place`,
  `project_ime_event`). `tracing` is in this crate's `reach-forbid`; the one path to
  it, `flui-foundation`'s own instrumentation, is excused there.
- **No `unsafe`.** `#![forbid(unsafe_code)]`: FFI belongs to the backends.
- **A display id cannot be built from a number, and a period cannot be invalid.**
  `DisplayId` is an opaque `NonZeroU64` with no constructor outside a
  `DisplayRegistry`, which issues it from the OS's own output identity, never wraps and
  keeps resolving known keys after exhaustion (`exhausted_counter_refuses_without_wrapping`;
  the compile-fail fixture `display_id_cannot_be_built_from_a_number`). `RefreshPeriod`
  is 1 ms to 1 s and every constructor refuses what does not describe a cadence, with
  rational rates converted in `u128` (`refresh_value_classification_table`). A snapshot
  never substitutes 60 Hz; that is a consumer's policy (ADR-0236). A display's scale
  is a `DevicePixelRatio`, held by its type to the range a screen can have
  (`DevicePixelRatio::MIN..=MAX`): a host reads the number its operating system reports
  once, where it enters, by `flui-native`'s one scale rule (out of range reads as the
  base ratio and is reported once per source; `crates/flui-native/ARCHITECTURE.md`), and
  no reader of a snapshot checks it again. A recording keeps the number and refuses
  one out of range when it is read.
- **The largest surface is stated here.** `MAX_SURFACE_EXTENT` is the longest side, in
  device pixels, of a surface a window may ask a renderer to draw: the engine caps its
  GPU's texture limit with it and the native hosts read it (what each does with it is in
  `docs/gaps.md`, "A window larger than a renderer draws"), since `flui-native` and
  `flui-engine` both read this crate and neither the other.
- **A headroom is finite and at least white; an absent colour says why.** A display's
  colour profile is `DisplayInfo::color`: a `DisplayColor` or the `CapabilityAbsence` the
  platform gives, which `color_space()` and `headroom()` answer as `Unsupported`.
  `DisplayColor` is `Standard(gamut)` or `High(gamut, headroom)`: only the high range
  carries a headroom (or the absence of one), so a standard-range display that
  contradicts its range cannot be built, and its `headroom()` is `Headroom::SDR`
  (`display_colour_table`). `Headroom` refuses a NaN, an infinity, a value below 1 and a
  luminance ratio that overflows. A profile change
  is a display change: a new revision of the same monotone snapshot, so an older sample
  never replaces a newer profile.
- **Flat root.** The contract's everyday names are re-exported at the crate
  root, the input vocabulary's (`PointerEvent`, `Key`, `Modifiers`, …) among
  them. A module is public where its other types keep their module path
  because they are not everyone's vocabulary: `os_options` (one operating
  system's window creation options), `haptics` (the second haptic tier, which
  the facade and `flui-sdk` re-export only under `unstable-input`, ADR-0210 §9,
  while `PlatformHaptics` and `HapticFeedback` stay at the root),
  `data_transfer`, `drag`, `text_store` (whose four traits are also at the
  root), `pointer`, `keyboard`, `input_capabilities`, `input_regions`,
  `capability`, `executor`, `http` and `launch`. `testing` exists only under
  the `testing` feature: what a test double's window is written with
  (`NoNativeSurface` and the `window_takes_no_frame_verbs!` macro) and the
  doubles of this crate's contracts, none of which production code uses.
- **The text store is owner-thread and UTF-16.** `TextStore` is shared as
  `Rc<dyn TextStore>`; `CommitGate` is not `Send`
  (a `static_assertions` test pins it). Every offset on the surface is a
  `Utf16Offset`, and the conversion to a field's own representation is
  `text_store::utf16`, nowhere else.
- **Input values are finite where they are built.** Every number in `pointer`
  enters through a validated type (`PointerPosition`, `Pressure`,
  `PenOrientation`, `ScrollDelta`, `PanZoomTransform`, …) whose `try_new`
  refuses NaN, infinity and out-of-range values with an `InputValueError`;
  a missing sensor is `None`, never a W3C stand-in value.
  `keyboard::{NamedKey, Code}` are generated by `cargo xtask key-vocabulary`,
  which `cargo xtask checks` runs, so they cannot drift from the pinned
  `keyboard-types`.
- **`PlatformWindow` has no `accessibility()` and no `as_winit`.** The
  accessibility bridge speaks AccessKit, so `flui-native`'s
  `HostWindow: PlatformWindow` carries it host-side: `open_window` returns an
  `Rc<dyn HostWindow>`, and the runner reads the bridge once before handing
  the UI runtime an `Rc<dyn PlatformWindow>`. A `compile_fail` doctest on the
  trait, paired with a twin that compiles, pins that the method is gone.
- **A window fact has one source: the `WindowState` snapshot (ADR-0254 §1).**
  `PlatformWindow` has no getter and no per-fact callback beside `state()`
  and the handler's `state_changed`; its capabilities are a field of the
  snapshot. The one query beside it, `gesture_geometry`, answers no fact of
  the window: it is the system's gesture distances read at the window's
  scale, which can fail and is retried, so it stays a query. Each fact is
  stated once in one shape: the theme is a `Brightness` (what the window
  system draws behind the content is its `WindowMaterial`, a command), and
  focus and activation are one `WindowFocus` (`Inactive`, `Active`,
  `Focused`), so a window that holds the keyboard and is not active cannot
  be published; a host whose window system reports the two apart reads them
  through `WindowFocus::reported`, where the focus wins, and `successor`
  publishes a hidden window `Inactive`
  (`focus_implies_activation_and_a_hidden_window_has_neither`).
  Showing, activating and closing are `WindowCommand`s (`Show`, `Activate`,
  `Close`, the last admitted by `WindowCapabilities::closes`), with no method
  twin. A `compile_fail` doctest on the trait pins that a getter does not
  compile.
- **Every implementation says what it does.** No method of `PlatformWindow`
  has a default body: a backend that defers its reveal and forgot
  `reveal_after_first_frame` would never show its window, and one that forgot
  `window_at` or `gesture_geometry` would report an absence it never chose. A
  `compile_fail` doctest on `reveal_after_first_frame` pins it.
- **The window system's refusal has one shape.** A command
  (`ApplyError::Platform`), a cursor (`CursorError::Platform`) and a
  preference query (`PreferenceQueryError::Platform`) the window system
  refused or failed carry a `PlatformFailure`, so a caller matches one type.
- **A snapshot holds no number that is not a window's.** Its rectangles are
  valid by construction, as its metrics and insets are: `WindowGeometry` is a
  finite position and a `WindowExtent` (each side finite and not negative),
  the content size is a `WindowExtent`, and so is each bound of
  `SizeConstraints`. A window system's report can be numbers no window has (a
  size over a scale of zero, a NaN a compositor sent); a host builds the
  values with `try_new`, and a report it cannot build them from costs the
  snapshot its geometry and nothing else (`flui-native`'s window state cell
  holds the rule for every host):
  - the outer rectangle, the content extent and the drawable metrics stay the
    published ones (a window's first snapshot has an empty rectangle and no
    metrics);
  - `normal_geometry` follows the mode the report gives, as the field's own
    rule says: none for a normal window; the published outer rectangle for a
    window that left the normal mode since, when it encloses an area, none
    otherwise; the published one for a window already away from it;
  - every other fact of the report (focus, activation, visibility, mode,
    occlusion, execution state, a close, the capabilities) is published as
    from any report.

  A NaN in a snapshot would also never equal itself, so every read
  would publish a revision (`window_geometry` table,
  `a_refusal_equals_itself`). What a command asks of its numbers
  (a positive size, a minimum not above the maximum) stays admission's
  (`WindowCommand::validate`), and `WindowOptions::geometry` holds an opening
  to the same checks, naming the option it refuses (`InvalidWindowOption`).
- **A read of the window runs no handler.** `PlatformWindow::state` may find
  the window system ahead of the published snapshot and publish a revision;
  the `state_changed` that revision is owed is delivered at the owner's turn,
  never from inside the read, whose caller may hold a borrow the handler
  takes again (ADR-0254 §4). `WindowHandle::is_open` asks whether a window is
  open without reading it.
- **Shown, minimized and occluded are three facts.** `visible` is the
  window's show state (`Show`, `Hide`, the opening option), which minimizing
  and covering leave `true`, as Win32's `WS_VISIBLE` and ICCCM's
  Normal/Iconic do; `mode` says minimized; `occluded` says the window system
  keeps a shown, unminimized window off the screen (a covering it reports, a
  cloak, a locked session, a suspended toplevel, a background page or app).
  `WindowState::successor` drops an occlusion reported for a hidden or
  minimized window, so the three never overlap, and `WindowState::is_seen`
  (shown, not minimized, not occluded) is the one reading of "the surface is
  seen" that frame scheduling, haptics and the lifecycle aggregate
  (ADR-0345 §1) take. Whether a covering by other windows is among the
  occlusions a host reports is the capabilities' `reports_occlusion`, so an
  unoccluded snapshot from a host without it (Wayland, mobile, the Web) says
  "not known to be covered", not "seen". Folding occlusion into `visible` made the hosts
  disagree (X11 read a minimized window hidden, Win32 shown); a separate fact
  is what winit (`Occluded`), AppKit (`occlusionState` beside `isVisible`)
  and Chromium (`OcclusionState` visible/occluded/hidden) keep too.
- **A window takes only an admitted command (ADR-0254 §2, ADR-0341).**
  `WindowCommand::admit(&WindowCapabilities)` is the one constructor of
  `AdmittedCommand`, and `PlatformWindow::apply` takes nothing else
  (`tests/compile_fail/{admitted_command_has_one_constructor,
  apply_takes_only_an_admitted_command}.rs`). Admission refuses with
  `InvalidGeometry` first, then `AdmissionError::Unpublished` for
  capabilities no window published, then `Unsupported` exactly when the
  capability is `Absent`. The token carries the *publication* of the
  capabilities it was admitted against, an identity this crate alone mints
  (one process-wide `AtomicIdMint`, never reissued):
  `WindowCapabilities::publish` mints one for a window's first snapshot, and
  `WindowState::successor` keeps it while what the window honours stays the
  same and mints a new one, at the new snapshot's revision, when it changes; a
  successor of capabilities of no publication stays of none. Capabilities of
  a publication cannot say anything but what the window published:
  `WindowCapabilities` hands each capability out by value through an
  accessor (`capabilities.material()`) and has no field to write
  (`tests/compile_fail/published_capabilities_cannot_be_edited.rs`), and
  `into_parts` is the one way out, to parts that turn back only into
  capabilities of no publication, as `unavailable` and the constants are. A
  window opens the token with `AdmittedCommand::against` its own published
  capabilities, which answers `ApplyError::Superseded`, giving the command
  back to be admitted again, unless they are the token's very publication;
  one publication is one set of capabilities, so nothing is checked twice.
  A window carries out only what its own published capabilities honour
  (`an_edit_of_published_capabilities_is_refused_at_admission`,
  `a_successor_a_caller_computes_opens_nothing`,
  `a_successor_of_unpublished_capabilities_stays_unpublished`), and a window
  double publishes its capabilities once and opens against those. Equality of
  `WindowCapabilities` compares what is honoured, not publication or
  revision. `ApplyError` is the world's (`Closed`,
  `Superseded { command }`, `Platform(PlatformFailure)`, where
  `PlatformFailure` tells `OutOfRange` from `Failed { message }`);
  `WindowControl::submit` answers `SubmitError` (`Admission`, `Closed`).
  `WindowCommand::admit_and_apply` is the one path that applies at once
  (admit against the snapshot, apply, admit once more on `Superseded`), and
  answers `CommandRefusal` (`Admission` or `Apply`). `successor` refuses
  only at the last revision (`WindowRevisionExhausted`); with no publication
  left, it withdraws changed capabilities (`WindowCapabilities::withdraw`:
  every command absent as `CapabilityAbsence::WITHDRAWN`, reports kept, nothing
  admitted, and so in every later snapshot) while the window's other
  facts publish on.
- **The cursor is a value of the region that asked for it; a grab lasts while the focus does
  (ADR-0354).** `set_cursor` takes a `Cursor` (`Icon`, `Image`, `Hidden`): hidden is a cursor,
  shown over that window until the next one, never a window-wide switch beside it, so nothing
  must remember to show it again. A `CursorImage` is checked where it is built (each side
  `1..=CursorImage::MAX_SIDE`, sides before pixels, four bytes per pixel, the hotspot a pixel)
  and shares its pixels, so a host keeps the last native cursor it made and compares with `==`;
  `cursor_image` is a verb's `Availability`, answered as the frame-rate hint is
  (`a_cursor_image_holds_only_what_a_window_system_shows`). `SetCursorGrab(Confined | Locked)`
  is admitted against `confines_cursor`/`locks_cursor`, `SetCursorGrab(None)` and
  `SetLevel(Normal)` against nothing, as the normal mode; `WindowState::cursor_grab` is
  `None` for every window without the keyboard focus, which `successor` holds
  (`a_grab_is_published_only_for_a_focused_window`) and every host's window system holds too
  (the conformance check `a_grab_ends_with_the_focus`). A locked pointer's motion is
  `PointerEvent::RawMotion` instead of moves. The level is a snapshot fact (`WindowLevel`); the
  resize steps and the proportion (`AspectRatio`, lowest terms) are requests the window system
  holds during the user's resize, not facts, as the material. A live resize is
  `WindowInteraction::UserResizing`, and `WindowInteraction::is_user` is the one reading of "the
  user changed it".
- **A window is opened with its handler (ADR-0254 §4).** `WindowHandler` has
  one required method per obligation (`input`, `state_changed`,
  `frame_requested`, `close_requested`, `closed`, `surface_changed`) and no
  registration beside it, so nothing a window delivers can arrive before a
  handler exists or fall into an inert default. The host holds it as an
  `Rc<dyn WindowHandler>`, so it stays on the thread that opened the window
  and every method runs there. A method takes no platform argument: the
  owner's platform is a host type (`OwnerPlatform`), which this crate cannot
  name, and a handler that drives the platform captures what it needs.
  `close_requested` answers a `CloseDecision`;
  `surface_changed` names its `SurfaceChange` (`Release` before the surface
  goes, `Rebuild` when a valid one must exist again).
- **"Not here" has one vocabulary.** Every error and outcome that can mean
  absence carries an `Unsupported`, built with `Unsupported::new(reason)` and
  never written as a literal, whose `UnsupportedReason` says why
  (`NotOnThisPlatform`, `NotOnThisDevice`, `NotYetOnThisHost`, `NotDeclared { what }`,
  `Unavailable`, `NotRegistered`, `NoWindow`); each answers
  `unsupported() -> Option<Unsupported>`, and only `Unavailable` is worth
  asking again (`is_retryable`). No reason names an operating-system version
  (ADR-0244). The table test `every_error_carries_its_reason` holds every
  carrier to it.
- **A closed enum is a decision** (ADR-0329, the workspace's
  `clippy::exhaustive_enums`): a
  public enum is `#[non_exhaustive]` where a backend may report something new
  to an application (`WindowExecutionState`, `WindowCommand`, the errors), and
  closed with an `expect` that says why where its set is fixed by what it
  names (a key goes down or up, TSF's lock kinds, the W3C `location` values),
  where `Custom` is its open end (`TransferFormat`), or where every consumer
  must handle a new variant (`PlatformInput`; `WindowMaterial`, which every
  host maps).
- **What a window cannot always honour answers in the same type.** A
  capability is `Absent` exactly when admission refuses its command as
  `Unsupported`; a command admitted and then refused by the operating system
  is `ApplyError::Platform`, a failure rather than a missing capability. A
  capability is one value, a `WindowCapability`: `Native`, `Emulated`, or
  `Absent` with where it is missing, a `CapabilityAbsence` (an
  `UnsupportedReason` narrowed to the three lasting reasons below, one byte):
  `NotOnThisDevice` where this device or its OS build lacks what its platform
  has (a version gate that refused), `NotYetOnThisHost` where the OS has it
  and the host does not implement it yet, `NotOnThisPlatform` otherwise. The
  type holds no retryable reason, and nothing else records the reason, so the
  snapshot and the refusal cannot disagree; `CapabilityParts`, the mirror a
  recording goes through, holds the same values. A host starts from
  `WindowCapabilities::NONE` and names what its window has, so a capability
  the contract gains is absent there until the host carries it out. The
  command-to-field mapping comes from one declaration that
  `WindowCapabilities::slot` matches exhaustively. The window material
  is such a command (`WindowCommand::SetMaterial`, `WindowCapabilities::material`);
  its set is the vocabulary every host shares (`Opaque`, `System`), and a
  material one OS alone has is that OS's extension. The frame-rate hint and
  the input regions stay verbs, because they are sent every frame and
  commands apply at the owner's next turn (ADR-0254 §3): each returns
  `Result<(), Unsupported>` exactly when its capability
  (`WindowCapabilities::frame_rate_hint`, `InputCapabilities::honours_input_regions`)
  says none, has no default body, and the window conformance checks
  both. A verb answers its reason itself: nothing admits it against the
  capabilities, so the capabilities keep no second copy of it. A test double whose window system takes neither writes them with
  `window_takes_no_frame_verbs!`.
- **The window stays on its owner thread; other threads get a target and a
  requester (ADR-0254 §5).** `PlatformWindow` is neither `Send` nor `Sync`, so
  no window, nor any `Rc`, `Arc` or borrow of one, crosses a thread
  (`a_platform_window_cannot_cross_a_thread`, a compile-fail fixture, and
  `assert_not_impl_any!` in `tests/surface_target.rs`). The thread that
  presents frames takes `surface_target()`: a `SurfaceTarget`
  (`HasWindowHandle + HasDisplayHandle + Send + Sync + 'static`) that answers
  `HandleError::Unavailable` once the window is gone, does not keep the
  window open (it may keep the native object its surface draws into alive,
  as wgpu needs), and has the required `pre_present_notify` the raster side
  calls before each present. The Apple targets hand out their view on the
  main thread only, where AppKit and UIKit make a surface, and Android's on
  its looper thread. A thread that wants a frame asks
  `redraw_requester()`, a `RedrawRequester`; any `Fn() + Send + Sync` closure
  is one, which is what a test double hands out. `testing::NoNativeSurface` (under the `testing` feature) is the
  target of a window with no native surface. `raw-window-handle` appears only
  through the two handle traits and their
  `WindowHandle`/`DisplayHandle`/`HandleError` (ADR-0089).
- **The GPU instance is made on the host's display, not a window's
  (ADR-0305 §2).** `DisplayConnection` (`HasDisplayHandle + Send + Sync +
  'static`) is what a host hands out once for its life: X11 and Wayland their
  connection, so it answers the display handle every target of the host
  answers and keeps answering it after any window closed; Win32, AppKit,
  UIKit, Android and the web their pointerless handle; headless none
  (`HandleError::NotSupported`). No window target implements it, since a
  target answers `Unavailable` once its window is gone.
- **A frame request carries when earlier frames reached the screen, on the
  event timeline, each report naming its frame (ADR-0347).**
  `FrameRequest::presented` is a `FramePresented { frame, at, refresh }`: the
  platform's newest report since the window's last request, naming the frame
  shown by the `FrameStamp` its present carried, the instant the display
  started showing it, an `EventTime` like every input stamp, so
  input-to-photon time is a subtraction, and the display's period as a checked
  `RefreshPeriod`; never a prediction (a platform that names when a frame is
  due fills `target_present`). The raster side hands each present's stamp to
  the target (`pre_present_notify(Option<FrameStamp>, SwapChainStatistics)`,
  `None` for a present that shows no frame), and a host carries it to the
  platform's answer for that present, never to "the newest present". Whether a
  window reports at all is a capability,
  `WindowCapabilities::reports_presentation`, `Absent` with why not; no verb
  answers for it, so the capability holds its reason. Where the platform reports
  presentation only to whoever holds the swap chain (DXGI), the presenter
  hands the surface target what it read before each present
  (`SwapChainStatistics::Dxgi(DxgiStatistics)`: the presents made so far,
  which number the next, and `DxgiShown`, in the platform's own units), and
  the host rebases it; a presenter that reads nothing passes
  `SwapChainStatistics::None`.

## Mapping decisions

### Coordinate conversion preserves admitted reading identity

`PointerMove::try_map_positions` changes only checked positions and preserves
times, sensors, metadata and measured/predicted membership. It does not repeat
source admission when distinct readings collapse to equal local values.
Mapper refusal leaves the source unchanged (ADR-0184). The public
`input_vocabulary_contract` row
`position_mapping_preserves_readings_and_refuses_partial_output` pins refusal
at each sample family and later successful conversion.

### Locale identity survives native observation and resource selection

`Locale` validates a complete BCP 47 tag through one parser boundary (ADR-0173).
Variants, extensions and private use survive equality and serialization;
component constructors reject malformed roles rather than creating unchecked
values. The public `preferences_contract` rows
`preferred_language_identity_preserves_variants_and_extensions`,
`malformed_preferred_language_tags_are_refused` and
`deprecated_subtags_canonicalize_everywhere_a_locale_is_built` pin identity,
admission and the retained aliases, including serde when enabled.

### Preference observations preserve unavailable values and native units

`SystemPreferences` records observations rather than framework fallback values
(ADR-0172). Text factors use the bounded accessibility range `1/64..=64`;
motion observations distinguish
reduced motion from duration scaling. Vertical wheel lines/pages and horizontal
character counts have separate representations. The accent colour is an
`SrgbColor`: finite sRGB components and alpha in `0..=1`, refused rather than
clamped outside it; scroll-bar visibility and what a click in the track does
are separate observations (ADR-0359). The `preferences_contract` table
pins numeric admission, including adjacent rejected boundary values, subnormal
text factors and both signs of zero. The widget contract
`media_text_scaling_changes_the_laid_out_text` checks real paragraph layout at
both admitted boundaries and fallback after extreme observations. Source
lifecycle, ordered delivery and consumer projection remain under implementation in
the platform-layer spec; these value tests do not establish those behaviors.

Mouse double-click timing and touch double-tap timing are independent observations.
`NativeMouseGeometry` keeps full double-click rectangle dimensions separate from
drag half-extents; `NativeTouchGeometry` retains physical slop and pixel/second
fling limits with their validated sampling `DevicePixelRatio`. Host observations
are comparison and invalidation evidence. `PlatformWindow::gesture_geometry`
queries the consuming presentation's actual coordinate context: `Ok(None)` means
unsupported, while a failed or invalid query is an error and must preserve the
consumer's accepted projection. `preferences_contract` covers independent timing,
per-context projection, zero axes and overflowing division.

### Platform services are capability traits in a contract crate

Text input, haptics, clipboard and system chrome are typed capability traits
(ADR-0030, ADR-0031, ADR-0312), not messages to one embedder, and the traits
live apart from their implementations, so a crate that names a capability links no OS code and a
plugin can implement one without the backends. The contract of each
capability is that of its own ADR.

What only the host owns reaches a provider through its `ProviderContext`
(ADR-0152 §2, item 6; ADR-0154 §9), which grows by methods: `owner()`,
`window()`, and `activity()`, the running Android Activity, read from the
`HostBridge` the composition root fills for each run's host and `None`
elsewhere. The Activity is scoped by the `CapabilityId` the registrar minted
for the provider's registration, which only its `RegisteredProvider` puts in
the context, so no caller names another capability by its string; the
values the host hands providers (`ResultTicket`, `ResultData`,
`ActivityResult`, `ResultRequest`) are made only with the `ResultMint`
`HostBridge::install_activity` hands the bridge. `capability::activity_result` holds that vocabulary and no JNI
type: a request is identified by a `ResultToken` the host mints per call and
named by a `RequestKey`, the acquiring capability's `NAME` and the
provider's `ResultKey` kept apart, which outlives an Activity Android
replaces; it ends exactly once, with an `ActivityResult` or a typed
`ResultRefused`, and lasts while the provider holds its `ResultTicket`, whose
`Drop` the host reads as a cancel (the `Resolver` answers no liveness query);
an answer's `Intent` stays with the host until the provider
hands its `ResultData` back, and a `ResultData` dropped unread releases it;
and the provider's own `jni` crate works on raw pointers the
host lends for one call (`JniFrame`, `IntentFrame`), filling objects the host
made instead of handing the host a reference it would have to trust.

### A provider learns that its window closed, and a system failure names its source

A UI runtime outlives each of its windows but one, so a provider that answers a window later
cannot take "the runtime retired" as the end of the window. `WindowRef::lifetime` is the
window's `WindowLifetime`: `is_closed`, and `on_close`, whose `WindowCloseWatch` runs a
listener once when the window closes. The runtime keeps the window's `WindowCloser` in the
presentation and closes it when the presentation's close begins, before its state drops, from
the one function every close path calls (`begin_close`: a window closed alone, the last window
of a runtime, the headless host's close); a closer dropped unclosed closes too. A listener that
panics does not keep the window open. The file dialogs end their dialog there (ADR-0344).

What a service's own error does not name is `SystemFailure`: an OS error code, a portal's
response code, a bus failure, or an answer outside the system's contract. `LaunchError::Failed`
and `flui-system`'s `DialogError::Failed` carry it, so no backend invents a number or guesses a
named variant for a code it cannot read.

### A launch is decided at the seam, by what its caller allows

`launch` holds the launch policy for every backend, the scripted double included, so no backend
re-implements it (ADR-0360). A URI reaches a backend only as an `ExternalUri`, built from the
schemes its caller names (`&[Scheme::HTTPS]`): bounded before it is read, with no character it
must percent-encode and no hidden one, an authority's `//` written and no user in it, a `mailto`
query of the five admitted fields, and never a scheme of `REFUSED_SCHEMES`, which no `Scheme` can
be. `Launcher`'s path verbs, from the path alone and with no file system call on the owner
thread, refuse a relative path and any Windows prefix but a drive letter, and `open_path` refuses
a name that `runs_when_opened` (`RUNNABLE_EXTENSIONS`, a class id). That name list is the floor; a
backend opens the file off the owner thread and judges what it opened, by its resolved name and
by its own system, before it hands it on.

### A capability's double is built from one scenario primitive

`capability::scripted`, under the `scripted` feature (which `testing` turns on), is what every
simulated provider is made of (ADR-0154 §10), in the contract because a capability crate may
not depend on `flui-testing`. A `Scenario` is one `ManualClock` and one ledger of failures; on
it a double makes a `Script<R, A>` per request type, a `Track<T>` per value the platform changes
(a permission, a `ServiceState`), and `Events<E>` per event stream.

- **Owner-thread state.** Nothing in the primitive is `Send`: the answer of a script, a
  track's value and a stream's event need not be, because `CompletionSource::pending` and
  `Resolver::resolve` ask `Send` of nothing, and a resolver is `Send` exactly when its answer
  is. The one value that crosses a thread is `FailureReport`, through which a `Send + Sync`
  double reports into the ledger.
- **A request is matched by its value.** A step is an `Expect<R>` (`any`, `exactly`, `when`)
  with an `Answer` (`value`, `computed`, `abandon`, each optionally `after` a delay) or, with
  `expect_held`, no answer until the test releases the request; a held step has no delay to
  ignore. The earliest step that accepts a request answers it and is used up. A request type
  fixes its answer type, so an open dialog cannot be answered with a save result.
- **No answer nobody wrote.** A request no step and no standing answer (`otherwise`) takes is
  `ScenarioFailure::Unexpected` and its resolver is dropped, so the caller's completion ends
  `ProviderDropped`; a held request never answered, a step never used, a change the clock never
  reached and an event after its stream's end are the other failures `Scenario::verify` lists.
  A double keeps a standing answer only where it models the service's state (the clipboard's
  text), never as a default.
- **Debts outlive handles, and an unverified run fails.** A script dropped with a held request
  or an unused step hands them to the scenario's ledger, every one before a panic of a request's
  `Debug` is resumed. The scenario panics as its last clone drops when a failure is left that
  no `verify` returned, unless the thread is already unwinding (ADR-0281), so a test that never
  calls `verify` cannot pass with work undone. What `verify` returned is acknowledged by
  position and identity, never by value: how far the owner-thread failures and the cross-thread
  reports had been read, and the ids of the debts and scheduled changes it listed; a failure
  recorded after it is new even when it equals one it returned.
- **Reports from other threads are bounded.** A `FailureReport` keeps `MAX_REPORTS` and counts
  the rest as one `ReportsDropped`; once the scenario has ended, `report` answers `false` and
  nothing accumulates. A script keeps its first `MAX_RECORDED_REQUESTS` requests and counts
  the rest.
- **One driver holds the clock.** The scenario shows only `now()`; the `ManualClock` is the
  `ScenarioDriver`'s, which `Scenario::drive` hands out once (a second is `AlreadyDriven`, so
  two hosts never move each other's time). `ScenarioDriver::advance` moves the clock and applies
  what fell due in one call, and nothing else can move it: `HeadlessHost::clock()` and
  `Tester::clock()` hand out a `HostClock` that reads the time and advances only through the
  driver, and every path of the host that moves time (`pump`, `advance`, a replayed recording or
  pointer script) goes through it. `Scenario::next_due` is a deadline of the host's loop like a
  gesture's: `time_to_next_frame` and `pump_until_settled` wait for it.
- **Every answer is a `Completion`.** A script, a track's `changed` and a stream's `next`
  answer through a `Resolver`, so an answer settles at an owner turn and never inside the call
  that asked; no borrow is held while a matcher, a computed answer or a value's `Clone`,
  `PartialEq`, `Debug` or `Drop` runs, so a caller that asks again from its answer finds the
  script idle. When that code panics, the request it served stays held, every other request
  is still answered and the first panic is resumed (`FirstPanic`). A stream ends once, with a
  `StreamEnd`, and an ended stream answers its end to every later `next`.
- **A request its caller calls off is withdrawn, not owed.** `Script::receive_with_ticket`
  hands back a `RequestTicket`, the scenario's form of a system request that lives while its
  requester holds it (a portal's `Request` object, an activity result's `ResultTicket`).
  Dropping it while the request is held or scheduled removes the request, drops its resolver,
  takes a scheduled answer off the timeline, tells the observers it ended unanswered and counts
  it in `Script::withdrawn`; a withdrawn
  request is neither `Unanswered` nor `NotReached`, and an answered one is untouched.

The launcher, file dialog, clipboard and primary selection doubles are built on it. Three
doubles are not, each for its contract's sake. `testing::FakeHaptics` answers synchronously
through a window's trait object, with no completion to settle. `testing::ScriptedHttp` serves a
`Send + Sync` client whose futures run off the owner thread (ADR-0280); a URL nobody scripted
fails with `HttpError::Connect` naming it and, once `report_to` attaches the client to a
scenario's `FailureReport` (the headless host's own client is attached), is that scenario's
`Unexpected` failure too. `flui_testing::storage::MemoryStorage` is a store behind the
`Send + Sync` storage contract: it keeps what was written and answers reads from it, so there
is no request it could find unexpected.
**Tests:** `deny_then_allow_then_revoke_at_three_seconds`,
`a_request_nothing_answers_ends_its_completion_and_fails_the_run`,
`a_step_answers_the_request_it_matches_not_the_next_one_in_line`,
`a_delayed_answer_settles_at_the_first_turn_at_or_after_its_instant`,
`an_event_stream_delivers_on_virtual_time_and_ends_once`,
`a_script_dropped_with_debts_hands_them_to_the_scenario`,
`a_scenario_dropped_unverified_with_debts_fails_the_test`,
`a_panicking_release_answer_keeps_its_request_held_and_answers_the_rest`,
`a_scenario_drives_one_host_at_a_time`,
`the_host_waits_for_a_scheduled_change_as_for_any_deadline`
(`crates/flui-testing/tests/scenario.rs`);
`an_owner_local_answer_settles_through_its_resolver` (`crates/flui-platform/tests/capability.rs`).

### A TSF-shaped pull store carries text input

Text-input authority is divided by contract rather than by one combined event count:

| Boundary | Authority |
|---|---|
| Presentation session, composing geometry and rendering | [ADR-0030](../../docs/adr/ADR-0030-platform-text-input-ime-capability.md), with its scoped successors |
| AppKit direct-key/input-method routing | [ADR-0069](../../docs/adr/ADR-0069-a-keydown-produces-one-semantic-event.md) |
| Portable UTF-16 store, locks, projection and conformance kit | [ADR-0090](../../docs/adr/ADR-0090-ime-pull-text-store-contract.md) |
| Committed-text notification, owner-code containment, composition exits and commit-anchor debt | [ADR-0142](../../docs/adr/ADR-0142-text-store-commit-gate-and-owner-code-containment.md) |
| Win32 owner-thread host, TSF document and COM lifetime | [ADR-0135](../../docs/adr/ADR-0135-win32-text-services-hold-the-text-store-on-the-owner-thread.md) |

Implementation and native acceptance remain separate: ADR-0142's outstanding
composition-exit, slot-residence and anchor-debt requirements are still obligations.
The native adapter does not replace the portable store's lock/settle contract.

`text_store` is a pull protocol (ADR-0090), not a push of whole editing values
between engine and client: the field is
a `TextStore` the platform locks, reads and edits, in UTF-16 offsets, the
shape of TSF's `ITextStoreACP` that AppKit's `NSTextInputClient` and Android's
`InputConnection` also map onto. A push source (`ImeEvent`, shaped after winit's `Ime`) goes
through `project_ime_event`, so a field has one editing path. The contract
holds the lock's values and the `CommitGate` a store reads; the state machine
that keeps the lock rules (`LockArbiter`), the ledger of the committed text
(`CompositionLedger`) and the edit count (`EditGeneration`) have one consumer,
a widget's store, and live in `flui_interaction::text_input` (ADR-0151 §6),
as does the reference store, `flui_interaction::testing::InMemoryTextStore`
(ADR-0264); `crates/flui-interaction/docs/ARCHITECTURE.md` describes them. Every
call into code a store does not control goes through `OwnerCalls` (ADR-0142
item 8; its module doc lists the points).

A push preedit's cursor is the input method's target clause: the projection
selects it inside the composition, as AppKit's `setMarkedText:selectedRange:`
and `text-input-v3`'s cursor do, so a field marks the part being converted on
every path.

What a field asks of the input method and the on-screen keyboard is one value,
`TextStoreStatus::text_input` (`TextInputOptions`): the purpose (a number with
the sign and separator its `NumberFormat` allows), the Enter key's label
(`EnterAction`), capitalization, correction (`Correction::{Off, Suggestions,
Autocorrect}`, three states where two booleans would allow a fourth that no
platform has) and autofill beyond the purpose (`Autofill`). `TextInputOptions::new`
takes the purpose and picks the rest's defaults from it (a search's Enter says
Search; prose is corrected, an address or a number is not), so the purpose has no
builder whose order would matter. Each host passes on what its platform has a
word for and states the rest in `platform-parity.toml`. A secret wins over every
option: `TextStoreStatus::holds_secret` (protected, or asking for a password or a
one-time code) is the one test before an input method may suggest, correct or
learn and before the journal records content; `protected` alone decides what
hides the text. A host acts on `status_changed` only where what it passes on
differs from what it last told the platform, restarts an input method only
outside a composition, and never shows a keyboard for it. The Enter key's label
changes only the label: every host delivers the key as Enter (the Web always
states an `enterkeyhint` so the browser picks none of its own), and the field
acts on its own label (`EditableText` moves the focus on Next and Previous), so a
hardware Enter and every platform's action key behave alike. The Send action is
`EnterAction::SendMessage`: a variant named `Send` would make rustc print the
marker trait in full (`std::marker::Send`) in every user's diagnostics.

**Tests:** the `text_store` module's unit tests; the store tests that need a
store are `flui-interaction`'s (`queued_text_store_grants_respect_gate_changes`,
`ime_events_project_onto_the_store_as_the_text_input_contract_says`); each host's
mapping of the options is pinned beside it (`keyboard_traits`,
`editor_info`, `ContentType::of`, the Web's hints, `scope_for`), the field's in
`flui-widgets`' `the_store_reports_the_text_input_the_field_was_built_with` and
`enter_moves_the_focus_as_the_fields_enter_key_says`, and the recording's in
`flui-native`'s `a_store_focus_keeps_every_field_of_its_status`.

### The input vocabulary keeps what the device reported

`pointer` and `keyboard` follow the W3C Pointer Events and UI Events models
and keep what flattening loses (ADR-0143): a scroll keeps its unit (lines,
pixels or pages, resolved by the scrollable that receives it), its precision,
its gesture phase and whether the platform reversed the device's direction
(`ScrollInversion`, `Unknown` where it does not say); a trackpad gesture is one
pan/zoom stream whose transform is cumulative since its `Start`; the pen eraser
is a tool, not a button; a cancelled sequence says why. A key-down says its
kind in its state (`KeyState::Down(KeyPress::{First, AutoRepeat, Unpaired})`),
so a release carries no repeat, and a dead key is `Key::Dead` with the accent
it leaves pending, never a named key (ADR-0346). A variant has a producer: a
pointer revision or a pen squeeze joins the vocabulary with the host that
reports it (`docs/gaps.md`). A value is checked once, where it is built, so the
framework never re-checks finiteness downstream.
**Tests:** `input_vocabulary_contract` here (sanitization, "no sensor",
scroll units, button sets, generated key spellings, key-down kinds, the dead
key) and `flui-native`'s backend conformance table.

### Bounded measured history retains checked ownership

`PointerMove::retain_latest_coalesced` removes an oldest prefix from its
already-checked chronological readings without exposing a mutable vector or
revalidating copied data. `flui-interaction`'s resampler uses it after canonical
coalescing to enforce its queue's retained-history bound. Zero is a valid bound;
the current reading, predictions and complete pointer metadata stay unchanged.
`bounded_coalesced_history_keeps_latest_readings_and_predictions` in
`input_vocabulary_contract` pins zero, one and multiple retained readings,
including distinct sensor readings sharing a coarse timestamp.

`PointerMove::try_coalesce_from` transfers the retiring movement's checked
history storage to the receiving dispatch; only the older history becomes empty.
The borrowed `try_coalesce` operation keeps its source intact. Both refuse a
complete pointer-identity mismatch before any mutation. Checked history bounds
allow already chronological concatenation to skip filtering and sorting;
overlapping or out-of-order packets still use canonical filtering and stable
sorting. `coalescing_transfers_only_checked_history_ownership` pins ordered,
overlapping, equal-time sensor and future-reading cases through the public API.

### Data-transfer delivery shares the foundation claim slot

`TransferRequest::pending` makes the pair over one `ClaimSlot` for cancellation, executor
registration and delivery. A producer is consumed by completion, so no
owner registry remains to wake on abandonment. Producer disconnection maps to
`TransferError::SourceGone`; polling any resolved request again is a caller
contract violation. `TransferRequest::ready` keeps its concrete payload locally
and never clones the executor waker.

Executor clone, wake and retirement run outside shared locks. A caught delivery
failure still destroys the executor's waker under containment and preserves the
result; during an existing unwind it is discarded, so a secondary failure cannot
replace the incoming one (ADR-0281 §2–§3). The concrete transfer
payload has no caller-defined destructor. Ordinary successful retirement still
runs destructors, with Rust's normal abort semantics if aggregate destruction
panics twice before containment regains control.

**Tests:** `transfer_request_recovery` exercises public completion during
executor clone, clone failure with a previous registration, displaced executor
retirement, wake/capture failure competition, cancellation and producer loss
during an existing unwind, ready results and next-request progress. Each row
runs in a bounded subprocess so a deadlock or abort cannot hide later rows.

### A haptic pattern is validated once and frozen

`HapticPattern` is built only through its consuming builder, which keeps the
first broken rule (order, finiteness, the unit range, a closed envelope, the
ten-second cap) and freezes the events as an `Arc<[HapticEvent]>` (ADR-0210 §2).
`Intensity` and `Sharpness` admit a finite `f32` in `0.0..=1.0` and read negative
zero as zero. A backend therefore never sees an invalid pattern, and a play can
only degrade, never fail. `PlatformHaptics` gained `perform_with`, `play` and
`capabilities` with default bodies, so an implementor of the Stable `perform`
keeps compiling; `testing::FakeHaptics` (ADR-0264) records each request with the actuator that
played it and the outcome it returned.
**Tests:** `haptic_pattern_builder_rejects_invalid_patterns`.

### Each operating system's creation options are one typed value of it

What a window is created with on an operating system is a field of
`WindowOptions::os_options`, an `os_options::OsWindowOptions` with one optional
value per system that has options of its own, of that system's type:
`windows: Option<Win32WindowOptions>` today (ADR-0254 §8). Options of every
system as fields of the common struct (GPUI) would grow it per system; untyped
keys (SDL) would lose the types; a single slot holding one system's value would
make a cross-platform application choose one, so setting Windows options and
then macOS ones would drop the first. The types live here because the slot is
here; they are pure data, built on every target, so a cross-platform
application sets every system's options without a `cfg` of its own. Only that
system's host reads its field. Another host (the headless host, another
system's) opens the window without it, and the application sees that its
options were not taken when that system's extension in `flui::native` has no
view of the window. They sit in their own public module, `os_options`, not at
the root: they are Stable like every item of this crate (ADR-0089 §1 gives no
module a tier of its own), so they take the least committal shape, every
struct and enum `#[non_exhaustive]` and no conversion to the system's own
values, which stay with the host.
**Tests:** `a_win32_window_opens_with_its_options` (`flui-native`),
`config_to_options_keeps_every_field` (`flui-app`).
