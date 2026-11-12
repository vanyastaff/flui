# Where FLUI falls short

> [!NOTE]
> **Last reviewed:** 2026-10-21

The work list. One entry per shortfall, ranked by how much it stops someone from choosing FLUI for a
production app on that environment. Each entry says what is missing, the evidence, who it hurts and
the parity document that tracks it. Every entry blocks a ready platform
([`platform-parity.md`](platform-parity.md)). Pick from the top, check open pull requests first
(`gh pr list`), and when a gap closes, update this file and the parity source
(`crates/flui-native/platform-parity.toml`, which renders the parity document) in the same pull
request.

An entry is named by its heading, never by a number: the rank is only the order of the entries, and
moving an entry changes nothing that refers to it. Documents link the heading's anchor
(`gaps.md#no-system-back`), and code names an entry with `flui_native::testing::gap::GapId`, whose
slugs the test `every_gap_id_is_a_gaps_entry` holds to these headings. Renaming a heading renames
its anchor: update the links and the `GapId` slug in the same pull request.

## Entries

### No text input on mobile and the Web

> **Environments:** iOS, Android, Web · **Who it hurts:** Every app with a text field · **Parity:** [Text input](platform-parity.md#text-input)

- No IME connection, soft keyboard, keyboard insets or input hints. The window contract carries keyboard insets (ADR-0277); Android (from API 30) and iOS report them with the keyboard's motion, the Web at rest; none has run on a device.
- **Runs on iOS:** an invisible `UITextInput` responder (ADR-0255 decision 11) serves the focused field's store as the window's store host, the one AppKit's text input serves it through: it takes the system keyboard when a field takes focus, edits the field's text, composition and selection through the store, answers ranges and rects from the field's layout, and sends Return and Backspace as key presses (type-checked; the Simulator run below predates the store host); on the iOS 27.1 Simulator the Japanese Romaji and nine-key Pinyin keyboards composed `日本語` and `你好` into an `EditableText` (2026-10-28).
- **Missing on iOS:** `selectionRectsForRange:` (answered with no rects), a run of the store host on the Simulator and a device, `UITextInteraction`'s selection UI, input hints, keyboard insets on a device (type-checked, unrun), hiding the keyboard checked on screen and a run on a device. The field draws kana and kanji as missing-glyph boxes on the Simulator: its font fallback has no CJK face there.
- **Missing on every host:** a multiline field. Return reaches a field as the Enter key (UIKit's lone `"\n"`, AppKit's `insertNewline:`, a hardware key), which a single-line field answers with submit; a multiline field has to insert its line break through its store on Enter, on every host.
- **Runs on Android:** it serves fields through `FluiView`'s `InputConnection` (ADR-0247), run with Samsung Keyboard on a Galaxy S21 (composition, commit, Escape, the action key, the keyboard shown and hidden, and through a manual Android run a field submitted by Done while the keyboard still composed).
- **Missing on Android:** never run with Gboard, `CursorAnchorInfo` placement (the caret and composing character boxes, type-checked), the keyboard's insets and their animation (type-checked, unrun on a device), paste through `OnReceiveContentListener` or stylus handwriting.
- **Runs on the Web:** it now serves fields through `EditContext` or a hidden textarea, run only through Chrome's own input-method path in headless Chrome.
- **Missing on the Web:** no soft keyboard or input hints; the keyboard insets have run only against a fake page on Node, not in a mobile browser.
- **Missing everywhere:** client queries (selected range, surrounding text, first rect) for dictation, reconversion and handwriting, and a session that survives a window move, resize, DPI change and focus loss, are rows of their own under [Text input](platform-parity.md#text-input). Other frameworks show the failures: macOS dictation silently drops text when `selectedRange` returns `NSNotFound` ([rust-windowing/winit#4666](https://github.com/rust-windowing/winit/issues/4666), [#3617](https://github.com/rust-windowing/winit/issues/3617)), a Korean input method stops editing inline after the window moves ([DioxusLabs/dioxus#5246](https://github.com/DioxusLabs/dioxus/issues/5246)), and the input-method output of egui carries a position only ([emilk/egui#7941](https://github.com/emilk/egui/issues/7941)).
- **The IME event order is pinned by rows on every host:** the backend conformance table drives the Linux hosts' `text-input-v3` client (which X11's IBus, Fcitx5 and XIM contexts feed too), TSF's document on Windows, the AppKit and UIKit client, Android's `FluiView` mirror and the Web's `EditContext` and hidden textarea through one composition (a preedit that grows, a picked candidate, a commit, a cancel) and holds the field's text, composing range and caret after each step, each as one edit (`each_input_method_path_holds_the_ime_rows_or_names_its_gap`).

### No accessibility on mobile and the Web

> **Environments:** iOS, Android, Web; desktop · **Who it hurts:** Blind and low-vision users; anyone who must meet accessibility law · **Parity:** [Accessibility](platform-parity.md#accessibility)

- Desktop is always on but untested with a screen reader.
- **Runs on iOS:** every UIKit window answers UIAccessibility through `accesskit_ios`'s low-level adapter, and the semantics identifier reaches `accessibilityIdentifier`; on the iOS Simulator an XCUITest client read the tree, found a button by its identifier and pressed it (2026-10-28). No VoiceOver session ran, and no AccessKit action was sent.
- **Runs on Windows:** the semantics identifier reaches UI Automation's `AutomationId`: a `UIAutomationClient` read a counter's button by its identifier, invoked it twice and read the count (Windows 11, 2026-10-29). No Narrator session ran.
- **Android, type-checked:** every Activity window answers TalkBack through `accesskit_android`'s low-level adapter, which `FluiView`'s node provider forwards to on the Java main thread; a publish is applied there in order. No TalkBack session ran. Explore by touch comes from the native input queue, where NativeActivity takes it: a finger hovering over the touchscreen goes to the adapter, not to the app as input (ADR-0255 decision 3). Bounds are placed from the view's screen location, which a device run must check against the window surface the tree is drawn in.
- **Web, run in headless Chrome:** every canvas window mirrors its tree as a parallel ARIA DOM (ADR-0255 decision 18): one `flui-node` element per node, nested as the tree is, in a host `position: fixed` over the canvas with `pointer-events: none`, so the pointer still reaches the canvas. Until a screen reader activates a placeholder button only it finds, the page holds nothing else, since the browser never says one is running; nothing deactivates it. A click on an element reaches the composition root as the action its node supports, focus moves the tree's focus, and keys on an element reach the window as on the canvas. A link with an address is an `<a href>` whose click is the node's action, a slider a user can move is an `<input type="range">` the browser adjusts for a screen reader, a node that takes a scroll offset is a scroller over content as long as its extent, whose scroll asks for that offset, and a focused text field is a native `<input>` or `<textarea>` the window's text input edits its store through, so a screen reader reads and edits the field the input method edits; the store stays the source of truth, and the field gone or unfocused, the store is served on the canvas again. Static text in a `SelectionArea`, which publishes runs (ADR-0214), is selected as the page's selection both ways: a screen reader's selection there asks `SetTextSelection` in run positions, and the tree's selection is shown in the page unless the page holds a selection of its own text. A live region, an announcement's included, enters the page empty and is given its text once the page has rendered it, since a browser reads out no region inserted with its text; a background tab gives it its text when shown. `flui-native`'s `web_browser` tests read the tree back from the DOM, click a node, move a slider's input, scroll a scroller, type into a served field, select static text, give a live region its text after it renders and keep the host over a canvas in a scrolled or scaled container (a rotated container is not followed). A date, time or spin field is still served on the canvas alone, and no NVDA, VoiceOver or TalkBack session read it.
- The semantics identifier (ADR-0240) reaches `AutomationId`, `AXIdentifier`, AT-SPI's id, iOS's `accessibilityIdentifier` and the Web's `data-flui-id` attribute through `author_id`. It does not reach Android's `viewIdResourceName`: the published `accesskit_android` sets none, so UI Automator, Espresso and Appium cannot find a node by it until an AccessKit release maps `author_id` there (written, not yet sent upstream).
- The runs read one control, not a Narrator, VoiceOver, Orca or TalkBack session.
- No locked adapter routes custom actions or `ShowContextMenu`, so gesture alternatives (ADR-0216) keep a control on every path.

### No system back

> **Environments:** Android, iOS, Web · **Who it hurts:** Every multi-screen mobile or web app · **Parity:** [Input](platform-parity.md#input)

- From targetSdk 36 Android delivers neither `onBackPressed` nor `KEYCODE_BACK`, so only predictive back works.
- iOS needs its own edge swipe.
- The Web needs History or Navigation API routing.

### Most implemented rows never ran on their environment

> **Environments:** all · **Who it hurts:** Every claim of support · **Parity:** [Summary](platform-parity.md#summary)

- 🟡 dominates every column of the parity summary.
- Device checks exist for a handful of rows, and the macOS and iOS drivers are Python and Swift.
- Display runs still owed:
  - the macOS and iOS display sources (type-checked, never run)
  - the macOS window by hand: the AppKit window conformance kit passes on an M1, macOS 27.0,
    2026-10-27 (nextest flui-native 233/233; the fixture after review: appkit_window 3/3, nextest
    flui-native 237/237, 2026-10-27), and [#1736](https://github.com/vanyastaff/flui/issues/1736)
    still lists a drag, the y-axis and a second display; the green button, Cmd-M and the Dock;
    opening options; displays (UUID and bounds); sleep, lock and memory-pressure notices; session end
  - the iOS window lifecycle (the memory warning and `applicationWillTerminate:`)
  - a window moved between two monitors on every host but Windows
  - unplugging a monitor on Windows
  - in a browser, the page moved to another screen changing the window snapshot's `display`, and
    a `freeze`/`resume` and a canvas focus change each arriving as one `WindowHandler::state_changed`: the
    Node suite covers them against a fake page, no browser run yet
- Persisted documents (ADR-0262) run headless only: no native process has run the acceptance
  app's persistence rows through flui-driver's `os` lane (a kill in a write loop, a read-only
  file, a long and redirected profile path, two instances on one directory, a session end during
  a save), so the row stays 🟡 on every native column.
- The panic boundary's Apple entries (AppKit and UIKit methods, blocks, GCD work items) are
  type-checked only: no panic has been seen contained on a Mac or an iPhone
  ([#1840](https://github.com/vanyastaff/flui/issues/1840)).
- The macOS extension `flui::native::macos` is type-checked only: neither the backdrop material
  behind the content, nor Liquid Glass (`NSGlassEffectView`, macOS 26), nor the narrowing of an
  AppKit window to its view has run on a Mac
  ([#1840](https://github.com/vanyastaff/flui/issues/1840)).
- A macOS display whose current mode reports 0 Hz (some built-in panels) has no period and paces at the 60 Hz default, because the `CVDisplayLink` fallback is not written.

### Native Linux host (Wayland and X11)

> **Environments:** Wayland, X11 · **Who it hurts:** Linux users · **Parity:** [Summary](platform-parity.md#summary)

- It replaced the winit fallback: `current_platform` picks the native Wayland or X11 host by the session (ADR-0151 §11), and an application of `flui-app` runs on each under Weston and Xvfb (`native_linux`). winit itself is deleted from the workspace, and `deny.toml` bans it.
- **X11 has its own host now** (`X11Platform`): one libxcb connection read by a loop on the owner thread, which waits with no timeout but the application's wake deadline; windows that publish what the server and the window manager hold, take every window command, and honour what the window manager supports; the RandR displays, the Settings portal, logind and memory pressure, and an AT-SPI bridge per window. Ran on WSL Ubuntu 24.04 under Xvfb with openbox, 2026-10-27: the window conformance, a mode set before mapping, the window manager's close asked of the handler, a frame asked for on another thread waking an idle loop, the exit with the last window and a wake deadline delivered once. Its input is XInput 2 (pointers, touch, pens, unphased smooth scrolling), XKB (keys, compose, the layout and its changes) and the cursor theme; XTest's pointer, buttons, wheel and keys ran through it on the same server (2026-10-28) and replay from a native recording, while touch, pens and smooth scrolling are checked by the decoder alone, since Xvfb has no such device. Its clipboard owns and reads the `CLIPBOARD` selection on a connection and a thread of its own (ICCCM: `TARGETS`, `TIMESTAMP`, the text forms, `INCR` both ways); two of its clipboards exchanged text, large text included, through Xvfb (2026-10-30). Its primary selection is `PRIMARY` on the same connection: the application's selection is asked for its text at the loop's turn, only when another client reads it, and two of its clipboards exchanged it through Xvfb (2026-10-30). It takes XDND drops on its windows and starts drag sessions from them ([No native drag and drop](#no-native-drag-and-drop)). Its text input serves the focused field to IBus or Fcitx5 over D-Bus, one input context per window: each key of a composing field is asked of the input method and held, the keys behind it waiting in order, and a key the input method leaves unanswered within 200 ms, or the keys of a window losing the focus or closing, go as typed in their original order; commits, preedits and deletions around the caret reach the field's store, and the field's text, caret, purpose and caret rectangle reach the input method. It ran against a stand-in IBus on a private `dbus-daemon` with Xvfb (`text::linux::x11::bus_tests`, 2026-11-02), never with a real IBus or Fcitx5; a key the input method forwards back (`ForwardKeyEvent`) is logged and dropped With no daemon on D-Bus, the host speaks XIM to the server `XMODIFIERS` names, over the X transport: a key forwarded and sent back is typed, one the server keeps is taken, its on-the-spot preedit composes in the field, and a server that goes, or sends what no message reads as, leaves keys typed; it ran against a stand-in XIM server on Xvfb (`text::linux::x11::xim::tests`, 2026-11-02), never with a real XIM server. XIM has no surrounding text, purpose or deletion, and a server offering only the root-window style draws its preedit in its own window. **Still missing on it:** handing the clipboard to a clipboard manager on exit, which comes with the host's later parts; a run on a desktop X server with a touchscreen, a tablet and a touchpad.
- The X11 host's pen, touch, smooth scrolling, touchpad pinch (XInput 2.4, which Xvfb announces), a grab's cancel mid-press and the device hierarchy (a device unplugged and plugged back in) are tested only synthetically: their XInput 2 events are built by the decoder's tests and the conformance table, never produced by a server, since Xvfb has no touch screen, tablet or touchpad and XTest drives only the core pointer and keyboard. Swipes are not delivered, as on Wayland.
- The native Wayland host has its foundation (`WaylandPlatform`): the loop, `xdg-shell` windows with the snapshot and commands, displays, preferences, lifecycle and AT-SPI, run against a headless Weston. Its windows ask the compositor for their frame (`xdg-decoration`), draw at a fractional scale (`wp_fractional_scale_v1`), are given no frame while suspended, and are exported for a portal's parent (`xdg-foreign` v2); the AT-SPI bridge follows the compositor's activation. A window moved between two outputs of one scale names the display it moved to, run 2026-10-28 on Weston 13 with two outputs (X11 backend on Xvfb). Weston offers none of `xdg-decoration`, `wp_fractional_scale_v1` and `xdg-foreign`; the three ran on a headless sway 1.9 at scale 1.5, 2026-10-28 (WSL Ubuntu 24.04): sway honoured a floating window's frame request and overruled a tiled one's, the buffer was drawn at 1.5, and a second client imported the export and saw it revoked with the window. A tiled window takes no `SetSize`, as the tiling holds its size. Under sway the window conformance still fails on `SetMode(Maximized)`, which sway (`xdg_wm_base` v2, no `wm_capabilities`) ignores and the window cannot know in advance. Its windows ask to be activated with an `xdg-activation` token, which names the last key, button, touch or keyboard focus the seats report ([Activation ignores the OS focus rules](#activation-ignores-the-os-focus-rules)). Its seats deliver input: keyboard through XKB (layout, compose and client-side repeat, read through the keyboard the X11 host shares), pointer and wheel, touch, pens (`tablet-v2`), pinches (`pointer-gestures`) and cursors (`cursor-shape`); the cursor, a click, a wheel notch and a key ran from a Weston seat, while touch, pens and pinches have run through their translation tests only, with no device, and no cursor shape has run: Weston 13 has no `cursor-shape`, so the cursor ran from the cursor theme (`XCURSOR_THEME`, `XCURSOR_SIZE`), the fallback for such compositors. A tablet pad's buttons, rings and strips are not read. Where the compositor draws no frame, the window draws its own ([No custom title bar, popups or owned windows](#no-custom-title-bar-popups-or-owned-windows)). Its text input serves the focused field to the input method through each seat's `zwp_text_input_v3`: preedit, commit and deletion around the caret reached the field's store and the field's text reached the input method, run 2026-10-31 under sway 1.9 with an input method of the test's own (`wayland_text_input`), never with IBus, Fcitx or a desktop compositor; a compositor without `zwp_text_input_v3` gives its windows no text input and says so in a warning, with no fallback to another input method path. It takes drops on its windows and starts drag sessions from them ([No native drag and drop](#no-native-drag-and-drop)). Missing there: the portal parent reaching a capability crate (the contract's window reference does not carry it yet), and a run under a desktop compositor (GNOME, KDE, Hyprland).
- A Wayland window opened with `visible: false` cannot be hidden: Wayland has no hide for a toplevel, so it stays unshown only until it draws its first frame, and no command shows or hides it later (`hides`/`shows` are `Unavailable`).
- Under Hyprland (Omarchy's compositor) the host takes the decoration the compositor chooses through `zxdg_decoration_manager_v1` (no title bar of its own), presents an alpha surface so the compositor's blur applies, and scales through `wp_fractional_scale_v1` and `wp_viewporter`. The native Wayland host does the decoration and the scale (not run under Hyprland); the alpha surface does not exist.

### Clipboard is broken on two environments

> **Environments:** Android, Web; all for rich · **Who it hurts:** Every app with copy and paste · **Parity:** [Clipboard](platform-parity.md#clipboard-and-drag-and-drop)

- No rich formats: the clipboard reads and writes plain text until a widget needs more, and the shape of that read is reserved by ADR-0312 §7 ([Accepted ADR sections not built yet](#accepted-adr-sections-not-built-yet)).
- Android reads and writes the system `ClipboardManager` through JNI; type-checked, no device run yet.
- The native X11 host owns and reads the `CLIPBOARD` selection on a connection of its own, run between two of its clipboards on Xvfb (2026-10-30); what it owns is gone when the application exits, as it hands nothing to a clipboard manager yet.
- The native Wayland host reads and writes its seats' selection through `wl_data_device`, run between two clients of Weston 13 (2026-10-30), and its primary selection through `zwp_primary_selection_device_manager_v1`, run between two clients of sway 1.9 (2026-10-30); no widget claims or pastes the primary selection yet.
- The Web reads the system clipboard through `navigator.clipboard` asynchronously, falling back to its own writes where the browser refuses; no browser run yet.

### File dialogs, open URL, reveal, trash, message box and share exist on Windows only, or nowhere

> **Environments:** all · **Who it hurts:** Every document app; any app with a link · **Parity:** [System services](platform-parity.md#system-services)

- The cross-platform surface is `flui_platform::launch` (open a URI or a file, reveal, trash, with the scheme policy in `ExternalUri`) and `flui-system`'s file dialogs (modal to the widget's window, one at a time), behind the facade's `system` feature ([ADR-0154](adr/ADR-0154-capability-crates.md) §11).
- **Linux** provides both through the XDG desktop portal (`OpenURI`, `Trash`, `FileChooser`) on zbus's own runtime, run against a stand-in portal on a private bus, not on a desktop. A dialog is modal to an X11 window; on Wayland it is not modal until the host exports an `xdg-foreign` handle.
- **Still missing:** Windows, the Web, Apple and Android answer `Unsupported(NotYetOnThisHost)` through `flui-system`; the core `Platform`'s `open_url` (Windows and Web, nothing calls it) and file dialogs (Windows only, every other host answers "cancelled") go once each host has its provider.

### No native drag and drop

> **Environments:** all · **Who it hurts:** Document, media and file-manager apps · **Parity:** [Clipboard and drag and drop](platform-parity.md#clipboard-and-drag-and-drop)

- The core runs: in-window sessions, sessions that reach the other windows of their runtime on the source's capture where the host places its windows, escalation to an OS session, identity and delivery to receivers in `flui-runtime`, exercised headless (ADR-0211). The `Draggable` and `DropTarget` widgets run on it headless, with the cursor, Escape and the keyboard and assistive cut, copy and paste.
- Windows has OLE as both target and source (`IDropTarget` per window, `DoDragDrop` started on the owner window's next turn, frames for every window while its loop rests), run only within one process (`ole_drag_and_drop_on_real_windows`), never with Explorer, another app, a held mouse button, pen or touch.
- The Web takes and starts HTML drags (a drag starts only from a press the runtime lifted before the browser's threshold), run in headless Chrome with constructed events.
- Android takes and starts drags on `FluiView` (ADR-0247): on a Galaxy S21 a drag the probe started dropped its text on its own window, and one crossed into Chrome, which declined it; no drag from or into another app has completed. A drop is read lazily from the clip the view holds, under each request's limits; that read is type-checked and its model run on Linux, not yet run on a device. A request for a URI's bytes gets five seconds from when it is asked for, its wait for a place included, as on Wayland and X11: past them it answers `TimedOut` and a cancel worker of the transport's has the view cancel its open and close its stream, keeping its place until its thread ends; a content provider that ignores the cancellation while the URI is still being opened keeps that place for as long as it blocks. Whether the close unblocks a read on a device has not been run. Requests past the four places wait in a queue of 64 and past it are refused `Busy`; the Wayland and X11 targets bound their running fetches but not the requests waiting for them.
- The native X11 host takes XDND drops (version 5) on its windows: types become representations (text once, HTML, every media type), each position is answered from the feedback the target cached, a refused drop finishes unaccepted, and a fetch converts `XdndSelection` on the owner thread, `INCR` included, held to its limit and timed out. Run on Xvfb with a source on another connection (2026-10-30, `drag::x11::tests`), never from a file manager or a browser.
- The native X11 host starts XDND drag sessions from its windows when a drag leaves them with a button held: it owns `XdndSelection`, finds the `XdndAware` window under the pointer, sends one position at a time, drops on a target that accepted and leaves one that refused or on Escape, and serves the session's representations from its provider, a payload past one property in `INCR` chunks, at most four at once, each let go of when its requestor stalls for five seconds or is destroyed. Still owed: `XdndProxy` and `XdndActionAsk`. Run on Xvfb with synthetic pointer events and a target on another connection (2026-10-31; `INCR` 2026-11-10, `drag::x11::tests`), never into a file manager or a browser.
- The native Wayland host takes drops on its windows through each seat's `wl_data_device` (up to v3): types become representations (text once, HTML, every media type), each motion is answered from the feedback the target cached, with the action a v3 offer asks for, a refused drop cancels its source, and a fetch reads the offer's pipe off the owner thread, held to its limit and five seconds, the offer finished with the action once the target concludes. Run 2026-11-03 under sway 1.9 with a source of the test's own (`wayland_drag`), never from a file manager or a browser, nor under a desktop compositor.
- The native Wayland host starts drag sessions from its windows: a session starts from a press the window holds, under the serial of the pointer grab it began, with a data source of its own offering each representation under the type names it is read as and the session's actions; the source's `send` is served from the session's provider and written from a thread of its own, and the session ends dropped with the action the compositor chose once the target finishes, or cancelled. A session started under a grab whose release the compositor already had ends cancelled, by the compositor (sway, Weston) or by the release reaching the window. Run 2026-11-03 under sway 1.9 into a target of the test's own (`wayland_drag`) and on Weston 13 (`wayland_input`), never into a file manager or a browser, nor under a desktop compositor; no drag icon is drawn.
- **Still missing:** `NSDraggingDestination`, and every other native drag source and target.
- Planned in the [interaction spec](plans/specs/flui-interaction/completeness/tasks.md) (ADR-0211; T28–T31, T35, T37 and the per-OS tickets).

### Window control has run only on Windows, an X11 test server and the AppKit conformance kit

> **Environments:** desktop · **Who it hurts:** Every desktop app · **Parity:** [Windows](platform-parity.md#windows)

- **Runs:** title, mode, fullscreen, position, size and constraint commands with a state readback ship on every desktop host ([ADR-0237](adr/ADR-0237-window-state-and-host-notices.md)).
- **macOS:** the AppKit window conformance kit passes on an M1, macOS 27.0, 2026-10-27 (nextest flui-native 233/233); the fixture after review: appkit_window 3/3, nextest flui-native 237/237, 2026-10-27. A hand run on the same Mac, 2026-10-28, one display: every command and opening option, a title-bar drag (the position follows, y grows downward, the top stops under the menu bar at y = 30), the green button (full screen) and Option-click (zoom and back), the yellow button, a Dock click (`reopen`, then `Show` brings the window back), a corner resize held to the constraints, the title-bar close of one of two windows, a borderless window taking keys, a quit Apple Event ([#1736](https://github.com/vanyastaff/flui/issues/1736)). A minimized window read `visible: false` and `SetSize` went below the minimum; both are fixed. Still owed: a second display; sleep, lock and memory-pressure notices; session end.
  - **A window at its maximum size reads `Maximized`.** AppKit's `isZoomed` holds for any frame equal to the standard frame, which the window's maximum size bounds, so a window resized by hand or by `SetSize` to its constraints' maximum (900×700 on a 3440×1320 usable area) publishes `Maximized` with a `normal_geometry`, though it does not fill the usable area. Win32's `IsZoomed` stays false for a window dragged to its maximum. Telling a zoom from a frame that equals the standard one needs a decision on what the backend tracks.
  - **`normal_geometry` after a macOS tile is stale.** Dragging a window to the top edge fills the screen through the system's tiling, which no delegate method reports; the window reads `Maximized` with the `normal_geometry` recorded before its last move, while AppKit returns it to where it was dragged from.
  - **A transient `Normal` publish on leaving full screen.** `windowDidExitFullScreen:` publishes the window before AppKit puts back the frame it had before full screen; only the held mode waits for the owner lane's next turn. A window zoomed before full screen therefore publishes `Normal` for a moment before it publishes `Maximized`. The final state is correct, seen on an M1, macOS 27.0: without a subscriber command the window comes back maximized, and with a subscriber's `SetMode(Normal)` it ends normal and unzoomed. Proposed fix: defer the publish with the held mode. Reported on [#1736](https://github.com/vanyastaff/flui/issues/1736).
- The native X11 host's window passes the window conformance under openbox on Xvfb (2026-10-27), the snapshot compared with what the server reads back; not yet under a desktop's window manager or on a HiDPI server.
- Neither Linux protocol lets a client read the title shown back, so the snapshot holds the one last set.
- Wayland never reports minimized.
- **Occlusion.** Every host keeps the window's show state (`visible`), its minimized mode and its occlusion (`occluded`) apart, and a minimized window stays shown; the conformance checks that without a readback of the host's own. What no run covers: a covering by another window on Win32, which reports none (a cloak and a locked session it does), on a desktop X11 window manager (the native X11 host hears a covering as `VisibilityNotify`, run with a full-screen window of another client over it on Xvfb, `a_covered_window_reads_occluded_and_shown`, which the conformance cannot read back), and on AppKit by hand; Cmd-H on macOS takes `isVisible` from the application's windows, so a hidden application reads as hidden, not occluded.
- Placement restore ran only on Windows; live resize is not audited.
- **The snapshot against the window system** (`window_conformance`'s `the_snapshot_agrees_with_the_window_system`: metrics, focus, activation, visibility and capabilities read back by the fixture's own means) ran on headless, on Win32, on AppKit (an M1, macOS 27.0, 2026-10-27), and on the native Linux hosts in CI, the X11 host under openbox on Xvfb and the Wayland host on Weston.
  - UIKit: not run, type-checked only, queued in [#1840](https://github.com/vanyastaff/flui/issues/1840).
  - Web: not run; the browser suite has no fixture that reads the canvas back apart from the window.
  - Android: not run; the conformance needs a device fixture.
- **Android cannot close its window:** `WindowCommand::Close` is `Unavailable` there (one native window, which the application does not finish), so a programmatic close answers `CloseRequestError::Refused`.
- **No close request on the Web and Android:** neither host ever asks a window handler's `close_requested` (a page's unload and an Activity's finish are not requests the application can refuse), so an application's close veto never runs there.

### No custom title bar, popups or owned windows

> **Environments:** desktop · **Who it hurts:** Apps with modern chrome; every menu and dropdown · **Parity:** [Windows](platform-parity.md#windows)

- The contract names a title bar the application draws inside a frame the system keeps (`WindowFrame::AppTitleBar`, admitted where `WindowCapabilities::app_title_bar` is available). The headless host, Win32, macOS, the native X11 host (where the window manager lists `_NET_WM_MOVERESIZE`), the native Wayland host (where it can draw a frame of its own) take it.
- A window without the system's frame (`WindowFrame::None`) is dragged and resized from the regions it marks on Win32 alone.
- The contract has caption hit-testing: move, resize, caption-button and cut-out content regions, resolved in paint order by `chrome_hit` and honoured per kind (`InputCapabilities::input_region`, ADR-0286), and `flui-widgets` marks them (`WindowTitleBar`, `WindowCaptionButton`, `WindowResizeArea`, with buttons and fields in a bar cut out by themselves). Win32 answers `WM_NCHITTEST` from them (moving, resizing from a band along the top, `HTMAXBUTTON` for the snap layouts, the caption buttons' pointer messages delivered to the application's buttons) and drops the caption in `WM_NCCALCSIZE`; it is type-checked and not run yet, and whether the desktop window manager draws its own caption buttons through a transparent material over such a bar is unknown. On macOS the window keeps a transparent system title bar under full-size content: a left press on a move region drags the window (`performWindowDragWithEvent:`), a double-click does what `AppleActionOnDoubleClick` names, the system keeps the resize borders and the traffic lights, which the application hides or moves; it is type-checked and not run yet, and whether the system's title-bar view takes clicks meant for the content under it is unknown. The native X11 host has no decorations (`_MOTIF_WM_HINTS`) and answers each press itself: a move or a resize is a `_NET_WM_MOVERESIZE` request sent once the pointer grab is let go of, a double-click toggles `_NET_WM_STATE` maximized, and a secondary press stays the application's, since no request opens every window manager's menu; it ran under openbox on Xvfb with XTest (a drag on the bar moved the window, a drag on its right edge widened it, a double-click maximized it), and has not run under GNOME or KDE. The native Wayland host asks the compositor for no frame and keeps the resize margins of its own frame without its title bar: a press on the bar moves the window through `xdg_toplevel.move` with the press's serial once the pointer travels, a double-click maximizes it and a secondary press shows the compositor's window menu; it ran on Weston 13 with XTest (a drag on the bar moved the window, a drag on the corner margin resized it, a double-click maximized it), and has not run under GNOME, KDE or a compositor that insists on drawing the frame. No design package builds a title bar from the parts.
- Menus, tooltips and dropdowns can't leave the window.
- On Wayland, where the compositor draws no frame (GNOME, Weston) or leaves it to the client, the native host's window draws its own: a title bar with the title and the buttons `wm_capabilities` offers, margins it is resized from, moved and resized through `xdg_toplevel.move`/`resize` with the press's serial, and the compositor's window menu on a secondary press. It ran on Weston 13's desktop shell (X11 backend on Xvfb, XTest, 2026-10-28): the bar read back from the screen, a drag moved and a corner resized the window, the buttons maximized and closed it. **Still missing:** it is the host's own neutral frame, light or dark after the desktop (an application that wants its own title bar asks for `WindowFrame::AppTitleBar`, above); no touch or pen drags it; no shadow is drawn; the window menu, tiled edges and a fractional scale have not run (Weston 13 has no window menu or tiling, sway draws its own frames), nor has GNOME. Its title is shaped over the bundled faces and the host's, read by a scan of its own, which an application's font collection does not share yet.

### No native menus

> **Environments:** macOS, iPadOS; Windows · **Who it hurts:** Every macOS app · **Parity:** [Menus](platform-parity.md#menus)

- No macOS menu bar (no Quit or Edit menu), so the shortcuts its items carry do nothing: Ctrl-Cmd-F leaves a full-screen window full screen.
- No native context menus, no Dock menu or jump list.

### Lifecycle holes

> **Environments:** all · **Who it hurts:** Apps that save state or hold resources · **Parity:** [App lifecycle](platform-parity.md#app-lifecycle)

- Session end is delivered on Windows (checked with sent messages), macOS, iOS, Android, the Web and Linux (there as a notification).
- The Web reports page freeze, context loss and page hide only against a fake page, not yet from Chrome.
- Sleep, wake and lock reach Windows, macOS and Linux (Linux checked only against a stand-in for logind), and memory pressure Windows, macOS, iOS, Android and Linux.
- A GPU device whose rebuild keeps failing is given up after about twelve seconds (ADR-0267). On the desktop the application gets it back with `AppHandle::retry_render_device`; Android and iOS have no such entry, so a given-up window there stays blank until the process restarts. No device loss has run on real hardware: the rows use the stand-in renderer.

### No deep links, URL schemes, file associations or single instance

> **Environments:** all · **Who it hurts:** Apps opened from links, files or a second launch · **Parity:** [App lifecycle](platform-parity.md#app-lifecycle)

- No host reports an opened URL or file: the registration that would have heard one is gone, and `opened(urls, files)` joins the application handler with the activation feature.
- Packaging (app identity) not audited.

### System preferences missing or never run

> **Environments:** all · **Who it hurts:** Users who rely on accessibility settings or another locale · **Parity:** [System preferences](platform-parity.md#system-preferences)

- Missing: text scale on the Web and on Linux desktops other than GNOME, accent colour and gesture timings.
- Never run: regional formats ([ADR-0278](adr/ADR-0278-regional-formats-are-the-unicode-locale-extension-keys.md)). Every host reads them: Windows from Region settings, Linux from `LC_TIME`, `LC_MEASUREMENT` and GNOME's clock format, macOS and iOS from Foundation's locale and calendar, Android from its calendar, the 24-hour setting and Android 14's `fw` and `nu` keywords, and the Web from `Intl`. None has run on a device or in a browser. The Web reports only what its `Intl` defaults to, and Android reports no units, since Android 14's regional preferences offer a temperature unit rather than a measurement system. A date or time pattern the user typed (a custom Windows short date) is not carried, since its syntax is the host's own.
- Never run: every host reads contrast and the preferred languages, but macOS and iOS are type-checked only, the Web ran on Node rather than in a browser, and Linux ran against a stand-in portal rather than a desktop session.
- Windows text scale is applied linearly. WinUI scales larger text less by an undocumented curve, so large text grows more than in a WinUI control at the same setting.
- Gesture presets per platform and pointer kind, accessibility floors, per-display geometry and headless injection landed with the [interaction spec](plans/specs/flui-interaction/completeness/tasks.md) (ADR-0212).
- Layout direction, per-app language, and system fonts and fallback are separate rows, tracked by [Locale edge: layout direction, per-app language, system fonts and fallback](#locale-edge-layout-direction-per-app-language-system-fonts-and-fallback).

### Pointer and keyboard gaps

> **Environments:** various · **Who it hurts:** Pen, trackpad and iPad users · **Parity:** [Input](platform-parity.md#input)

- **Missing:** no pen on macOS or Linux; no iPad hardware keyboard; the iPad pointer (trackpad scroll, pinch, rotation, hover and the secondary button) is type-checked only, never run on an iPad; no cursors on mobile; keyboard layout read on Windows only.
- Windows: the precision-touchpad pan and pinch (DirectManipulation) have never run on a touchpad, and no injector reaches it, so its golden recording can only come from a `native-capture` run on one (injected touch, pen, mouse and wheel already record theirs through a manual Windows run).
- Android: touchpad pinch and two-finger scroll and its pen hover and tilt ran only from `uinput` devices through a manual Android run, with no hardware touchpad or stylus. Its widgets ran with `uinput` touches, `input` stylus and mouse events and hardware key events through a manual Android run, with no physical keyboard, key repeat or dead key.
- Android: a touch held while the app goes to the background makes the system report it not responding ten seconds later. The loop reads its input queue only while the Activity is resumed, so the cancel the system sends for the window it took away is never answered (seen on a Galaxy S21 by holding a touch and pressing Home).
- Web: the runner names the compile target (`TargetPlatform::Unknown`) as the tree's `PlatformConvention`, not the browser's OS, so a Safari tab on macOS follows the platform-neutral conventions: Control jumps by word and the clipboard and refresh chords use Control.
- Web: no precise-scroll phases, and its trackpad pinch (decoded from Control-held wheel and WebKit gesture events) ran only with constructed events in headless Chrome.
- The native Linux hosts' touch, pens, pinches and smooth scrolling ran only through their translation tests, never on a session with a touchscreen, tablet or touchpad.
- Pointer frames (`PlatformInput::PointerFrame`): only Windows touch and pen report them, read from the frame's own history and delivered once per frame id, and that path has run on synthetic data only (injected touches on a hosted Windows runner): no two-finger touch run on a Windows touch machine yet. The native Wayland host frames the touches each `wl_touch.frame` closes, as one pointer frame per surface (translation tests only, no touchscreen run). Android frames the fingers of one `ACTION_MOVE` (cross-checked in CI, no run on a device), and iOS (a touch set) still reports each contact on its own, as frames of one.
- Pointer, scroll and pinch work is planned per OS in the [interaction spec](plans/specs/flui-interaction/completeness/tasks.md) (T36 DirectManipulation is implemented and awaits a touchpad run; T40–T44).

### Mobile system UI

> **Environments:** iOS, Android, Web · **Who it hurts:** Every mobile app · **Parity:** [System UI](platform-parity.md#system-ui)

- Status-bar icon style ran on Android, iOS is type-checked only.
- **Missing:** no navigation-bar or immersive control (Android enforces edge-to-edge from targetSdk 35); no orientation lock; no safe area on the Web or for the macOS notch.
- Haptics has both tiers and the frame queue (ADR-0210), but:
  - its macOS and iOS paths never ran on a device
  - the Web's ran only in headless Chrome (activation refused, then `Submitted`, no motor)
  - Android ran only feedback and amplitude waveforms (on a Galaxy S21 without primitives; envelopes and compositions never ran)
  - Windows pen haptics ran only on a host with no haptic pen, while Windows touchpad and mouse haptics (`InputHapticsManager`) wait for a `windows` crate that ships it ([interaction spec](plans/specs/flui-interaction/completeness/tasks.md))
- System gesture exclusion is implemented on Android and iOS; it ran on an Android device, never on iOS.

### Desktop shell services

> **Environments:** desktop · **Who it hurts:** Background, messaging and utility apps · **Parity:** [System services](platform-parity.md#system-services)

- No tray, notifications, taskbar progress or badge, global shortcuts, launch at login, keep-awake or secure storage.

### Text services

> **Environments:** all · **Who it hurts:** Writing apps; sign-in forms; tablet users · **Parity:** [Text input](platform-parity.md#text-input)

- No spell check, system edit menu, autofill or handwriting.
- Handwriting, dictation and reconversion also need the text-input client queries and the session that survives a window move, resize, DPI change and focus loss; those rows sit under [Text input](platform-parity.md#text-input) and are tracked with [No text input on mobile and the Web](#no-text-input-on-mobile-and-the-web).

### No native views inside the UI (web view, map, video)

> **Environments:** all · **Who it hurts:** Apps that embed a map, browser or player · **Parity:** [Windows](platform-parity.md#windows)

- Not in the contract; the design shapes compositing, hit-testing and accessibility.
- Needs an owner decision.

### Window looks

> **Environments:** desktop; mobile for capture · **Who it hurts:** Apps with a native look; apps showing sensitive data · **Parity:** [Windows](platform-parity.md#windows)

- Backdrop materials, transparency, icon, capture exclusion.
- The contract's window material (`WindowCommand::SetMaterial`: `Opaque`, `System`) is drawn on Windows (Mica, build 22621 and later; a Win32 window still opens on it) and on macOS (`NSVisualEffectView`'s window-background material, type-checked only); Linux, Web and mobile report it `Unavailable`. The backdrop material and Liquid Glass of `flui::native::macos` ran on macOS 27 (an M1, 2026-10-27, `an_appkit_backdrop_goes_behind_the_content_and_comes_away`); the contract's material on macOS waits for its Mac run; no window icon on Windows or Linux.
- What the OS has and the host does not drive answers `NotYetOnThisHost`, not `NotOnThisPlatform`: the material on iOS (`UIVisualEffectView`), on Android from API 31 where the device blurs across windows, and on an AppKit window off the main lane; the macOS frame-rate hint from macOS 14 (`NSView.displayLink`); the pointer's shape on Android from API 24 (`PointerIcon`) and on an iPad from iOS 13.4 (`UIPointerStyle`). Below those versions, on an iPhone, or with the device's cross-window blur off, the answer is `NotOnThisDevice`. Only the rules' table tests ran (on Windows); none of the iOS, Android or macOS paths has run on a device.

### Unaudited rows

> **Environments:** desktop, Web · **Who it hurts:** Unknown · **Parity:** [Windows](platform-parity.md#windows)

- App identity and packaging, live resize.

### Version floors and the GPU fallback not run on devices

> **Environments:** all · **Who it hurts:** Users on older systems or GPUs without Vulkan or WebGPU, who get a failure in the middle of start instead of a clear refusal · **Parity:** [App lifecycle](platform-parity.md#app-lifecycle)

- The floors are declared in `platform-support.toml` ([version map](version-map.md)), the backends refuse a release below them at start, and `flui build` packages them, raised by an application's `[platforms.<environment>] min`: the APK's `minSdk` and the NDK level, the iOS and macOS deployment targets with `MinimumOSVersion` and `LSMinimumSystemVersion`. No bundle has run on its floor.
- Android packages API 23; below API 24, or on a device without a working Vulkan driver, the renderer falls back to GLES 3.0, which no device has run yet.
- The CLI writes no Windows application manifest (`supportedOS`), and an iOS app built through a hand-made Xcode project takes its deployment target from that project; only the start-up refusal holds the floor there.
- Windows links a 1607 API unconditionally; the Linux release glibc is the runner's.
- The GPU fallback (Vulkan to GLES 3.0, WebGPU to WebGL2, Direct3D 12 to WARP) is compiled and the engine's pixel suites pass on llvmpipe GL; only Linux has run it, and no browser has run WebGL2 or Windows WARP.

### No presentation feedback

> **Environments:** iOS, Android, virtual displays · **Who it hurts:** Every animated app; every latency claim · **Parity:** [Displays and metrics](platform-parity.md#displays-and-metrics)

- Windows (DXGI frame statistics, run) and the native Wayland host (`wp_presentation_time`, run under Weston 13) report when each frame reached the screen, on the event timeline; the frame clock keeps the report and bounds idle work by the next refresh. Headless injects one.
- macOS reports (run): the AppKit view is backed by its own `CAMetalLayer`, which wgpu draws into and whose `nextDrawable` adds `addPresentedHandler:` to each drawable; the display pass moved from `drawRect:` to `updateLayer`. On an M1, macOS 27.0, 100 Hz (2026-10-28): 29 reports for 30 presses of `counter`, the shown times a multiple of 9.999 ms apart, the handler 0.34–0.49 ms after the showing; the conformance row saw a report 13 ms before the next request. During a live resize Metal reports most drawables never shown (80 of 102); whether the window shows fewer frames there than with wgpu's own layer is not measured, and no one has looked at a resize by eye.
- A virtual display reports nothing (`NotOnThisDevice`): a Windows desktop driven only by the Microsoft Basic Render Driver (a Hyper-V machine such as a `windows-latest` runner) shows `SyncRefreshCount` 0 in every frame statistic, and a Mac that is a hypervisor's guest (a `macos-latest` runner, "Apple Virtual" display) gets `presentedTime` 0 for every drawable. Windows reads the adapters at startup; macOS reads `kern.hv_vmm_present`, a proxy, so a guest with a display that does report is still told it has none, and a virtual display on bare metal is not caught. The `Native` path is owed one run on real Windows and Mac hardware after this detection.
- iOS reports nothing (`NotYetOnThisHost`): a UIKit view whose layer class is a `CAMetalLayer` (FLUI's or the plain class) showed nothing on the iOS Simulator (iOS 27.1), while the same app with wgpu's sublayer draws; the simulator's drawables have no `addPresentedHandler:` either. A device run decides whether the layer class works there.
- Android reports nothing (`NotYetOnThisHost`): the time a frame was shown belongs to the Vulkan swap chain wgpu presents through, and wgpu reads none. The path left is wgpu reading `VK_GOOGLE_display_timing` (`vkGetPastPresentationTimingGOOGLE`) and handing it to the presenter as DXGI's statistics are handed over. The loop now runs its frames on the choreographer's callback (ADR-0269), and on API 33 its preferred frame timeline's `expectedPresentationTimeNanos` fills `FrameRequest::target_present`; that names when a frame is due on screen, never a report that it was shown. Type-checked only, not yet run on a device.
- The Web (a `requestAnimationFrame` timestamp is a frame's start) and X11 (the Present extension reports a completion to the GPU driver's connection) have no presentation time a toolkit can read, and their windows say so.
- Animations still tick on the vsync instant, never the presentation time (ADR-0257 decision 2); nothing yet attributes a presentation to the produced frame it showed, so the frame telemetry's input-to-present histogram still ends at the submit.

### No HDR, wide gamut or colour management

> **Environments:** all · **Who it hurts:** Photo, video and design apps; every app on an HDR display · **Parity:** [Displays and metrics](platform-parity.md#displays-and-metrics)

- The engine requests a surface colour capability per window (`AppConfig::surface_color`, [ADR-0256](adr/ADR-0256-surface-colour-is-requested-and-converted-at-presentation.md)) and converts to Display P3, scRGB, extended sRGB or HDR10 at presentation; no run on an HDR or P3 display backs it yet, and no Metal space has run.
- Every display carries its colour profile (`DisplayInfo::color`: `SurfaceColorSpace` and `Headroom`, or why there is none), read on Windows, macOS, iOS, Android, Wayland and the Web, and a profile change is a new snapshot revision; the engine sets a surface up for the colour of its window's display. Only an SDR Windows monitor has run; no HDR or P3 display, and the Apple, Android and Wayland reads are type-checked only.
- A profile change reconfigures the window's surface before its next frame (ADR-0256 §6, amended), but no host ran a real change: Windows, macOS and Wayland observe the change type-checked or synthetically only, iOS and Android re-read only on their other display events.
- X11 reads no colour (`_ICC_PROFILE` is not parsed). Windows has no broadcast for the SDR white level, so a moved slider is read at the next other display event. Android sets the window's `ColorMode` for the requested surface but does not listen to `Display.registerHdrSdrRatioChangedListener`: the listener needs a Java class in `FluiView`, so a moved HDR/SDR ratio is read only at the next configuration change or resume. The Web reports capability only and no headroom.
- Sources are still sRGB: images with a profile, video and HDR images enter the working space as sRGB values.

### Sandbox-aware file access is absent

> **Environments:** all desktop and mobile · **Who it hurts:** Every app distributed through a store or Flatpak · **Parity:** [System services](platform-parity.md#system-services)

- No security-scoped bookmarks, portals, AppContainer-aware paths, Storage Access Framework or document-scoped URLs.
- `data_dirs` and the dialogs assume an unsandboxed process, so an App Store, Flatpak or MSIX build gets paths it cannot open.
- The manifests are part of the same gap: nothing generates macOS entitlements, Flatpak `finish-args`, Snap interfaces or MSIX capabilities from declared capabilities (the parity row "Sandbox and entitlement manifests generated from declared capabilities"). Tauri's autostart plugin shows the cost: it writes a launcher path that cannot run outside the Flatpak sandbox ([tauri-apps/plugins-workspace#3166](https://github.com/tauri-apps/plugins-workspace/issues/3166)).

### Secure text entry and concealed clipboard entries are partial

> **Environments:** all · **Who it hurts:** Every sign-in form; password managers · **Parity:** [Text input](platform-parity.md#text-input), [Clipboard and drag and drop](platform-parity.md#clipboard-and-drag-and-drop)

- A protected field keeps its keystrokes from other processes and input-method learning on Windows (the TSF password input scope), Android (the password variation with `IME_FLAG_NO_PERSONALIZED_LEARNING`), the Web and macOS (secure event input held while it has focus in the key window, type-checked, not run on a Mac). iOS sets `isSecureTextEntry` from the focused field's store status (type-checked, not run on a device). On Linux the native Wayland host sends `text-input-v3` the content type of the store's status, a protected field as the password purpose with the hidden-text and sensitive-data hints, and shows it no surrounding text (`a_protected_field_shows_the_input_method_no_text`, unit-tested over the wire seam); the native X11 host sends IBus and Fcitx5 the same purpose and hints and no surrounding text for a protected field, unit-tested for the mapping (`a_field_states_its_purpose_in_each_protocol`).
- A clipboard write can be concealed (`ClipboardWrite::concealed`, ADR-0152 §6), and every host answers what it does with one as a `Concealment`. Windows, macOS and iOS keep it out of the system's history and sync (type-checked, not run on those hosts); X11, Wayland and Android only mark it, and only a clipboard manager or keyboard that reads the mark honours it (on Weston up to 14 a concealed write made under the same input as an ordinary one is not put on the clipboard, with a warning: that compositor keeps the earlier, unmarked source); the Web has no mark, so a concealed write there is an ordinary one, and the scenario `a_concealed_write_stays_out_of_history` fails on it as this gap.
- Android's `ClipDescription.EXTRA_IS_SENSITIVE` (API 33) is only a rendering hint, so the Android cell of the concealed-clipboard row can be at most partial by design; below API 24 a clip carries no extras at all.
- No text field writes concealed yet: an obscured `EditableText` copies nothing, and an application that copies a secret calls `ClipboardHandle::write` itself.
- Platform-edge diagnostics have no redaction check: protected-field text, clipboard payloads and URLs with tokens must never reach the `tracing` targets or the `native-capture` journal, which will carry text and URLs once more event families are recorded (the [OWASP Logging Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Logging_Cheat_Sheet.html) lists them under "Data to exclude").

### Input latency modes and high-rate sampling are partial

> **Environments:** all but Windows and the Web · **Who it hurts:** Drawing, games and anything that tracks a finger or pen · **Parity:** [Input](platform-parity.md#input)

- An application sets `InputLatency` for every window it opens with `AppConfig::with_input_latency`.
- Android asks the Activity's decor view for unbuffered pointer dispatch (`View.requestUnbufferedDispatch`, API 30+), which `ViewRootImpl` applies ahead of `NativeActivity`'s native input queue; type-checked, not run on a device. Below API 30 the system batches input once per frame.
- iOS attaches the touches UIKit coalesced into a move and the ones it predicts (type-checked, no run on a device). Pencil force, altitude and azimuth that UIKit later revises (`touchesEstimatedPropertiesUpdated:`) are not reported: the pointer vocabulary has no revision of a delivered sample, so a stroke keeps the first estimate. The planned shape is a `#[non_exhaustive]` `PointerEvent::SampleRevised { pointer, sample_index, sensors }`, which lands with the first stroke or ink consumer; a scenario that covers Pencil estimates fails on this gap until then. Holding a reading back until its estimate settles was rejected: it adds the latency this entry is about.
- The Linux and macOS paths are not audited.
- Timestamp conversion is a hazard of its own: Win32 `GetMessageTime` is 32-bit milliseconds that wrap every 49.7 days, Wayland event times have an undefined base, and Android's event time is uptime milliseconds. Windows pointer ticks are rebased across the wrap (`NativeTime::TickMillis`); the native Wayland host reads a stamp through the same rule only when it lies within a second of `CLOCK_MONOTONIC` and stamps the event on arrival otherwise (Weston's X11 backend stamps on the X server's clock); the X11, Android and Web conversions are not audited.

### Distribution is unaudited

> **Environments:** all · **Who it hurts:** Everyone who ships · **Parity:** [Distribution](platform-parity.md#distribution)

- Packages, signing, notarisation, identity.
- `flui build` stages unsigned `.app`s and a debug APK; no MSIX, Flatpak, AAB, notarisation or Authenticode.
- The identity that notifications, URL schemes, portals and jump lists need is not checked into the package.
- Entitlement and sandbox manifests (macOS and iOS entitlements, Flatpak `finish-args`, Snap interfaces, MSIX capabilities, Android permissions) are not generated from declared capabilities; the Android template declares `VIBRATE` itself.

### Activation ignores the OS focus rules

> **Environments:** Windows, Wayland · **Who it hurts:** Apps brought forward by a link, a second launch or a notification · **Parity:** [App lifecycle](platform-parity.md#app-lifecycle)

- Windows follows the foreground rules: an activation the rules refuse flashes the window's taskbar button and leaves the foreground with the application the user works with, a right handed over (`AllowSetForegroundWindow`) brings it forward (run with two processes, 2026-10-28). Opening a link, file or folder hands the right only to the handler process the shell started, by its id; a handler already running (a browser, Explorer) is handed nothing and comes forward under the OS's own rules, which may leave it behind (the PID hand-over not run against a real handler). No second launch hands the right yet: that comes with single instance, in the activation spec ([No deep links, URL schemes, file associations or single instance](#no-deep-links-url-schemes-file-associations-or-single-instance)).
- The Win32 snapshot says `focused: true` and `active: true` for a window of a process the user is not working with: they are its thread's focus and activation, not the system's. `WM_ACTIVATEAPP` does not tell them apart (Windows sends it for the thread's own activation of a background process, seen in a two-process run, 2026-10-28), so the fix needs the foreground's changes (`EVENT_SYSTEM_FOREGROUND`) to reach each window, which the host does not watch yet.
- The native Wayland host asks with an `xdg-activation` token and uses the launcher's `XDG_ACTIVATION_TOKEN` for the first window it holds; a token names the last key, button, touch or keyboard focus its seats report, the serial a compositor weighs before it moves the focus (with no input yet, an activation asks for attention: sway marks the window urgent). CI covers only the refusal: Weston 13, which CI runs, has no `xdg_activation_v1`, so the whole path runs only on sway, locally ([testing](testing.md)).
- macOS cooperative activation (`NSApplication.yieldActivation(to:)`, macOS 14) is documented by Apple, which also marks `activate(ignoringOtherApps:)` deprecated; neither is audited.

### Live resize is unaudited and unscheduled

> **Environments:** desktop, Web · **Who it hurts:** Every user who resizes a window by dragging its edge · **Parity:** [Windows](platform-parity.md#windows)

- No environment has been audited: the row "Live resize without jitter" is ❔ on Windows, macOS, Wayland, X11 and the Web, and [Window control has run only on Windows, an X11 test server and the AppKit conformance kit](#window-control-has-run-only-on-windows-an-x11-test-server-and-the-appkit-conformance-kit) and [Unaudited rows](#unaudited-rows) mention it only in passing.
- Windows has no `WM_ENTERSIZEMOVE` handling outside the DirectManipulation tests. That message opens a modal loop inside `DefWindowProc` that lasts until the drag ends ([Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-entersizemove)); winit reports that its `ControlFlow` is ignored there and that continuous redraws make a drag choppy ([rust-windowing/winit#3272](https://github.com/rust-windowing/winit/issues/3272), [#4708](https://github.com/rust-windowing/winit/issues/4708)).
- Metal presents asynchronously to Core Animation transactions unless `CAMetalLayer.presentsWithTransaction` is set ([Apple](https://developer.apple.com/documentation/quartzcore/cametallayer/presentswithtransaction)); Wayland needs an `ack_configure` before the committing frame and frame callbacks to pace the redraw; X11 has `_NET_WM_SYNC_REQUEST`. None of these is audited here.
- macOS sets `presentsWithTransaction` on the view's layer for each display pass of a live resize and clears it after, so wgpu presents that drawable in the transaction that carries the new window frame; the frame renders inline in `updateLayer`, so the wait wgpu adds holds the main thread only while an edge is dragged. Type-checked, not looked at by eye.
- The engine presents with `Fifo`, which blocks on vsync and ghosts a stale frame during the Windows modal resize loop; its own notes name `DXGI_SCALING_NONE` as the only real fix, which wgpu 30 does not expose. A resize-jitter probe saw no size-mismatched acquire in a 40-resize burst into the Metal swapchain; the compositor-side half is unobserved.
- It is among the most-reported defects of comparable frameworks ([emilk/egui#903](https://github.com/emilk/egui/issues/903) is a fast-resize flicker). A reference run should record its trade-off in the row note: Flutter's Windows embedder blocks the platform thread for up to 100 ms for a frame at the new size.
- Pass criteria: a scripted two-second border drag with every presented frame captured, content size equal to window size in every captured frame, callbacks counted during the modal loop, and the p99 frame time recorded.

### No present-mode, queue-depth, VRR or tearing control

> **Environments:** desktop, iOS, Android · **Who it hurts:** Games, drawing and latency-sensitive apps · **Parity:** [Displays and metrics](platform-parity.md#displays-and-metrics)

- The engine always presents with `Fifo` and a frame latency of 2; nothing lets the platform layer or the app choose a present mode, a queue depth, variable-refresh behaviour or tearing.
- Presentation feedback ([No presentation feedback](#no-presentation-feedback)) is read-only and "Refresh rate and frame pacing" covers rates only; control over how frames are presented is the other half.
- Windows tearing needs `DXGI_FEATURE_PRESENT_ALLOW_TEARING` and `DXGI_PRESENT_ALLOW_TEARING`, which can only be used with sync interval 0 ([Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/variable-refresh-rate-displays)); Wayland has `wp_tearing_control_v1`, `fifo-v1` and `commit-timing-v1`.
- Queue depth costs latency: the default DXGI queue of up to three frames adds about 16–32 ms of input latency in another framework ([libsdl-org/SDL#15656](https://github.com/libsdl-org/SDL/issues/15656)).
- The Web has nothing to control: `requestAnimationFrame` is its only cadence.

### Launch handoff on mobile and the Web

> **Environments:** iOS, Android, Web · **Who it hurts:** Every mobile and web app in its first second · **Parity:** [Windows](platform-parity.md#windows)

- The OS shows a splash before the first frame on these environments, and the first frame must match it: Android 12 and later through the `SplashScreen` API ([Android documentation](https://developer.android.com/develop/ui/views/launch/splash-screen)), the iOS launch screen, and the page background before a Web app's wasm loads.
- Nothing aligns them today: the generated Web page sets no background before the wasm loads and its manifest `theme_color` is `#FFFFFF`, the generated Android manifest sets no splash or window background, and iOS declares an empty `UILaunchScreen`.
- The parity row "Show without a blank first frame" covers the desktop only, where the window is created hidden and shown after the first presented frame; Electron does the same with a hidden window shown on `ready-to-show` and a `backgroundColor`.
- A dark-mode user should see the dark splash and the dark first frame, not a white flash between them.

### Quality-bar partials with no entry

> **Environments:** all · **Who it hurts:** Everyone who relies on a platform claim; maintainers who cannot replay a failure · **Parity:** [Quality bar](platform-parity.md#quality-bar)

- Honest absence: the nine spellings of "not here" are one now, `Unsupported` with its reason in every contract error (ADR-0151 §4). Other layers show what silent spellings cost: winit setters return `()` while their documentation lists platforms as unsupported, `tauri-plugin-geolocation` resolves to a zeroed position on the desktop, and `tauri-plugin-notification` reports a permission it did not check.
- Replay: pointer, key and wheel traces, system notices, session ends, reopens, input method events, reads of the system preferences and displays, and window snapshots replay on the headless host; close requests and closes do not.
- The `native-capture` journal records pointers, system notices, session ends, reopens, input method events, reads of the system preferences and displays, and window snapshots (titles redacted by default), not close requests or closes. An input method that edits the focused text store directly (TSF on Windows, the Android and web mirrors) delivers no event, so its edits are not journaled: the `ime` row family reserves a store-edit payload for them.
- Not every unsupported cell in the parity tables names its reason, and nothing checks the cells against the code's `Unsupported` reasons.

### Power-aware pacing and throttling

> **Environments:** all · **Who it hurts:** Every user on battery or a thermally limited device · **Parity:** [System preferences](platform-parity.md#system-preferences)

- Power state (battery, low-power mode, thermal) is read on no environment, and nothing acts on it: no row named a frame cap or a background pause before.
- The native signals exist: Apple's `ProcessInfo.thermalState` and `isLowPowerModeEnabled` with a change notification, Android's `PowerManager.getCurrentThermalStatus()` and `addThermalStatusListener`, Windows battery saver through `SYSTEM_POWER_STATUS.SystemStatusFlag` and `RegisterPowerSettingNotification`, and the Linux portal's `PowerProfileMonitor.power-saver-enabled`.
- Apple's `CADisplayLink.preferredFrameRateRange` is only a hint that Low Power Mode, a critical thermal state and accessibility settings can override, so an app that asks for 120 Hz still has to follow the state.
- iOS asks its display link for the screen's best rate only while a presentation animates or a pointer is down (the runtime's frame-rate demand, as on Android), and leaves a still screen to the system's rate. On a still screen the link pauses: it runs only while a frame is asked for, and a request from any thread resumes it on the main thread (type-checked, no device run). The Web's frame loop stands still the same way once the refresh estimate has its window of frames, and a request resumes it (run in the wasm32 suite on Node, not in a browser).
- The check forces each state and asserts a delivery within one event-loop turn and a frame-rate or work reduction.

### Locale edge: layout direction, per-app language, system fonts and fallback

> **Environments:** all · **Who it hurts:** Apps in right-to-left or non-Latin languages; users who set a language for one app · **Parity:** [System preferences](platform-parity.md#system-preferences)

- [System preferences missing or never run](#system-preferences-missing-or-never-run) names languages and regional formats only. Layout direction, per-app language, and system fonts and fallback have no reading in the platform layer.
- Android 13 adds per-app language preferences and `setApplicationLocales` ([Android documentation](https://developer.android.com/guide/topics/resources/app-languages)); iOS has a per-app language setting; a right-to-left layout follows from the locale.
- Text layout resolves fonts through `fontdb` in `flui-painting`. DirectWrite `IDWriteFontFallback`, Android `fonts.xml` and `AFontMatcher` (API 29), CoreText and fontconfig are the platform answers to which font draws a character the requested family lacks. The collection puts such an answer (`SystemFallback`) ahead of FLUI's tables ([ADR-0271](adr/ADR-0271-the-host-feed-asks-the-platform-which-faces-fall-back.md)). DirectWrite gives it on Windows, CoreText on macOS and iOS, fontconfig on Linux and the BSDs, `AFontMatcher` on Android from API 29 and `fonts.xml` below it; Windows, Apple and Android are type-checked, not run. When the first preferred locale changes, the feed's thread asks again and installs the new answer. The order follows the app's first locale only: a paragraph in another language falls back in that order, since the fallback key carries no per-paragraph locale yet.
- The check is a locale matrix (en-US, ar-EG for right-to-left, ja-JP, hi-IN with its numbering) on every OS: list order, first weekday, 12 or 24 hour clock, per-app override, and CJK and emoji text without missing-glyph boxes.

### Accessibility bridge fidelity (properties and announcements)

> **Environments:** desktop, iOS, Android, Web · **Who it hurts:** Screen-reader users: a control announced without its value, hint or state, or a change nobody hears · **Parity:** [Accessibility](platform-parity.md#accessibility)

- [No accessibility on mobile and the Web](#no-accessibility-on-mobile-and-the-web) is about missing adapters; a bridge that exists can still drop properties or announcements, and the two fail independently of the tree and actions.
- Another framework's Linux bridge drops a field's `value` and `hint` ([flutter/flutter#194638](https://github.com/flutter/flutter/issues/194638)) and gives no AT-SPI announcement for live regions, snack bars, alert dialogs or route changes, only for an explicit announcement call ([flutter/flutter#194045](https://github.com/flutter/flutter/issues/194045)).
- The parity rows "Properties (value, hint, description, state)" and "Announcements and live regions" are ❔ on Windows and macOS, run on X11 against an AT-SPI client (below), and ❌ where there is no adapter at all.
- Each needs a check with the real assistive technology on each OS that reads role, name, value, hint, state, actions, focus order and announcements; a harness that registers AT-SPI listeners and matches only strings carrying a nonce keeps any other source from producing a hit.
- Assistive-technology focus reaches a widget on no native platform. The model is there: `Semantics::on_assistive_focus_within` tells a subtree when the screen reader's cursor enters and leaves it, a `Carousel` pauses its autoplay on it, and `PresentationCapabilities::assistive_focus` says whether the platform reports the cursor (`each_native_adapter_says_whether_it_reports_the_assistive_cursor` in `flui-native`). UI Automation, NSAccessibility and AT-SPI give an application no signal for Narrator's, VoiceOver's or Orca's cursor, and the browser gives a page none for any screen reader's. The published `accesskit_ios` and `accesskit_android` do track VoiceOver's and TalkBack's cursor, in private fields, so iOS and Android say `Unreported` until an AccessKit release hands it to the application: a handler told when VoiceOver's cursor moves, and an accessor that reads where TalkBack's is. Both changes are written against AccessKit and not yet sent upstream. Where the cursor is not reported, a `Carousel` pauses on keyboard focus, pointer and the app leaving the front, and the pause control it draws is what meets WCAG 2.2.2.
- Windows scroll by position is missing: the published `accesskit_windows` implements no UI Automation `ScrollPattern`, so Narrator and NVDA read no scroll position and scroll only through a position slider's `RangeValue`, though FLUI publishes the position from the top or left edge and takes `SetScrollOffset` (ADR-0273). An upstream pull request adds the pattern ([AccessKit/accesskit#816](https://github.com/AccessKit/accesskit/pull/816), open); it closes with the `accesskit_windows` release that has it.
- **Linux, run against a real AT-SPI client:** `flui-native`'s `linux_atspi` starts a session bus and the accessibility bus of its own (at-spi2-core) beside an Xvfb, publishes a window's tree through `accesskit_unix` and reads it back as Orca would: role, name, hint as the description, a field's text through the Text interface (from its runs; a value without runs reaches no AT-SPI interface), a slider's value and range, states and actions; announcements at their urgency, live regions whose own or child's label changes, a status that appears, and focus once the window is active, each matched by a nonce of the run. A route change and a modal dialog appearing are told by name (ADR-0204 §5): heard on X11 as a polite announcement (`a_route_change_is_heard_by_its_name`); on iOS and Android as the platform's notice of a new screen, type-checked, with no VoiceOver or TalkBack run; on Windows, macOS and the Web as an announcement, not run there. No Orca session ran.

### Add-to-app: no FLUI view inside an existing native app

> **Environments:** all · **Who it hurts:** Teams adding FLUI to an app they already ship · **Parity:** [Windows](platform-parity.md#windows)

- FLUI owns the process: the host creates the window and runs the loop (`FluiActivity`, a `NativeActivity`, on Android, `UIApplicationMain` on iOS, the Win32 pump, `NSApplication`, the root element it creates on the Web). A view placed into a window the app already owns — a child `HWND`, an `NSView`, a `UIView` under a foreign view controller, an Android `View` in a foreign `Activity`, an element of a page the app renders — has no host mode, so the only way in is to hand FLUI the whole window.
- What exists: the renderer draws into a `SurfaceTarget` (`HasWindowHandle + HasDisplayHandle`, [ADR-0254](adr/ADR-0254-window-contract.md)), so the surface can be foreign; input, focus, IME, accessibility and lifecycle still assume the host's own window procedure, responder chain or `Activity`.
- Other frameworks ship it as their enterprise path: Flutter's add-to-app ([Flutter documentation](https://docs.flutter.dev/add-to-app)) with `FlutterEngine` and `FlutterViewController` or `FlutterFragment`, Compose's `ComposeView` in a View hierarchy, SwiftUI's `UIHostingController`, Qt's `QWidget::createWindowContainer`.
- The design question is who owns the loop: an embedded view is driven by the app's loop and receives events from the app's window, so the owner thread's one call point for user code ([ADR-0151 §10](adr/ADR-0151-platform-layer-boundary-and-names.md)) has to run inside a foreign callback, and ADR-0151 names an external embedder as the first reason to reopen the crate split.
- The check is one screen of FLUI inside a foreign app on each OS: keyboard focus moving into and out of the view, IME and the assistive technology reaching it, and the app's own lifecycle pausing and resuming it.

### No gamepad, and no D-pad run on a TV

> **Environments:** all · **Who it hurts:** TV, console-style and Steam Deck users; games · **Parity:** [Input](platform-parity.md#input)

- No gamepad in the contract or on any host: no buttons, sticks, triggers or rumble from XInput or GameInput on Windows, `GCController` on macOS and iOS (and tvOS's remote), Android's `SOURCE_GAMEPAD` and `SOURCE_JOYSTICK` axes (`AXIS_X`, `AXIS_HAT_X`, `KEYCODE_BUTTON_*`), the Web Gamepad API (polled through `navigator.getGamepads()`, with only connect and disconnect events) or evdev on Linux (Wayland has no gamepad protocol; a Steam Deck under gamescope has a controller as its only input).
- Android's D-pad arrives: the `ui_events` key table maps `KEYCODE_DPAD_UP`, `DOWN`, `LEFT` and `RIGHT` to the arrow keys and `KEYCODE_DPAD_CENTER` to Enter, and `DefaultFocusTraversal` binds unmodified arrows to `DirectionalFocusIntent`, so a remote can move focus; it never ran on Android TV or Fire OS, and a TV app also needs the `leanback` feature declaration and a banner that the Android packaging does not write.
- Without a pointer (Android TV, tvOS, a console) focus navigation is the whole input model: every widget must be reachable by direction with a visible focus ring, which no run has shown.
- The check is a remote on an Android TV device and a controller on Windows and a Steam Deck: every control of the interaction widgets reached by direction and activated by the centre button, and a controller's sticks and buttons recorded and replayed.

### No rotary input

> **Environments:** Windows, Android; Apple later · **Who it hurts:** Surface Dial and Wear OS users · **Parity:** [Input](platform-parity.md#input)

- No rotary vocabulary in the contract: a rotation in detents from a device of its own arrives nowhere — the Surface Dial through `RadialController` on Windows (per window, with the tool menu the OS draws), Wear OS's bezel or crown as a `MotionEvent` with `AXIS_SCROLL` from `SOURCE_ROTARY_ENCODER` ([Android documentation](https://developer.android.com/training/wearables/user-input/rotary-input)), the Digital Crown on watchOS and visionOS.
- Wear OS is a form factor of the Android host, not a column: its lists are expected to scroll by rotary input, so a watch app without it fails review on the first screen.
- The check is a Surface Dial on Windows and the rotary input of a Wear OS emulator turning a scroll view by detents, recorded and replayed.

### No compute lane off the UI thread on the Web

> **Environments:** Web · **Who it hurts:** Web apps that decode images or lay out large documents · **Parity:** [Quality bar](platform-parity.md#quality-bar)

- `Background::compute` (ADR-0272) runs its job inline on the UI thread on wasm32, because `ExecutionServices` is sequential there (ADR-0047): a long decode blocks the frame it was started in. The answer still settles at the next owner turn, so widget code is the same everywhere.
- The lane belongs on Web workers: a worker pool that runs a `ComputeJob` needs shared memory (`SharedArrayBuffer`, which needs cross-origin isolation headers) or a job shape that crosses `postMessage` by value.
- The check is a browser run of a large image decode started through `Background::compute` with the frame clock showing no frame longer than its budget.

### Images decode on the UI thread without background lanes

> **Environments:** hosts without `BackgroundCapability` · **Who it hurts:** Embedders whose host registers no background lanes · **Parity:** [Quality bar](platform-parity.md#quality-bar)

- `Image::memory`, `Image::file` and any provider with a `DecodeJob` decode on the background lanes (ADR-0272), and `Image::asset` and `Image::network` load through their registry on them (ADR-0279). A host that does not provide `BackgroundCapability` gets the job run on the UI thread instead, so the image still shows but its decode costs that frame. `flui-app` and the headless host provide the capability; an embedder's own host may not.
- Decoded images are not shared between widgets: two `Image::memory` widgets with equal bytes decode twice. A decoded-image cache owned by the UI runtime (never by the process) is the follow-up.
- The check is a host without the capability decoding a large image with the frame clock showing the cost, and a cache hit test once the runtime cache exists.

### The driver agent cannot reach a page in Safari

> **Environments:** Web (Safari) · **Who it hurts:** Agents and tests that drive a FLUI app in Safari · **Parity:** none yet (the driver agent has no row)

- A page's agent is reached through the browser's own remote protocol (ADR-0288 §6): `flui run --agent` speaks WebDriver BiDi to Firefox and the DevTools protocol to Chrome, Chromium, Edge, Brave and Opera, which need no driver of their own. Safari has neither: `safaridriver` speaks classic WebDriver, which has no channel or binding a page can report through, so `flui run --agent --device browser:safari` is refused, naming this entry.
- The likely shape is a third dialect of the CLI's `BrowserLink` once `safaridriver` serves WebDriver BiDi, or one over classic WebDriver that polls the page for its lines.

### The driver agent cannot reach an app in the iOS Simulator

> **Environments:** iOS · **Who it hurts:** Agents and tests that drive a FLUI app on iOS · **Parity:** none yet (the driver agent has no row)

- `flui run --device <simulator>` builds the app, installs it and launches it once through `simctl`, and hands it no agent endpoint or launch token, so `flui run --agent` on a Simulator is refused, naming this entry. The nightly iOS run of the counter scenario (`an_agent_presses_the_counter_on_a_live_device`) fails as this entry.
- The likely shape: the Simulator shares the Mac's file system, so the desktop's Unix socket and token can reach the app through `simctl launch`'s `SIMCTL_CHILD_` environment, and the iOS runner serves the agent as the desktop runners do. A device needs a transport of its own (a forwarded port, as on Android).
- The check is the counter scenario passing on a Simulator, then on a device.
- The check is `flui run --agent` in Safari on macOS, `flui mcp` reading the app and acting on it, and a reload reconnecting.

### An interactive widget costs eighteen elements

> **Environments:** all · **Who it hurts:** Every screen with many controls; every catalog built on the wrappers · **Parity:** [Quality bar](platform-parity.md#quality-bar)

- `RawButton` and Material's `InkWell` mount one element and one render object for their behaviours (the private node of [ADR-0322](adr/ADR-0322-a-control-mounts-one-element-for-its-behaviours.md) §1; `crates/flui-widgets/src/interaction/raw_button.rs`, `packages/flui-material/src/ink_well.rs`), and so do the Material controls built on `InkWell` for those behaviours. Still on the wrappers: the outer `Semantics` of `ButtonStyleButton`, `Checkbox`, `Switch`, `Radio`, `Chip`, `ListTile`, the tabs, `NavigationBar` and the floating action button, and every Cupertino control (`CupertinoButton` builds its own `Semantics` and `GestureDetector`). On the wrappers a button holding a `Text` was 18 elements, 8 render nodes, 421 allocations and 46 KB at mount; an `InkWell` over a `Text` was 15 elements, 9 render nodes, 200 allocations and 33 KB, and on the node is 3 elements and 2 render nodes, its ink included; its allocations and bytes are held by `packages/flui-material/perf/baseline/ink_well_over_text.toml` (`perf_ink_well_over_text`). The second render node is the ink's `CustomPaint`, the price of the ink: it stays mounted at rest, because inserting it over the child at the first hover would remount the child and lose its state.
- What a `RawButton` adds to the `Text` it holds is counted by `perf_raw_button_over_text` (`crates/flui-widgets/tests/perf.rs`) and recorded in `crates/flui-widgets/perf/baseline/raw_button_over_text.toml`: the elements, the render nodes and the allocations are at the entry's numbers, the bytes are not. What is left of them is the node's view, held twice, in its parent and in its element (it carries a role and an identifier in place of the description, which the node builds while the window collects semantics); the focus node with its notifier, its listener and its measured rectangle; and what any element and render node cost to build, lay out and paint the first time.
- The entry closes when a `RawButton` adds to its child at most 2 elements, 1 render node, 40 allocations and 3 KB, asserted by that scenario. The `Text` itself is recorded by the same scenario (`text_allocations`, `text_bytes`) and is not this entry's condition. Until then no new widget family starts on the wrappers (`AGENTS.md`, "Working here").
- The scenario's baseline holds the migration threshold of ADR-0322 (2 elements, 1 render node, at most 40 allocations and at most 5 KB), so the Material and Cupertino controls move to the node now. The entry stays open until they have moved and the bytes reach 3 KB.

### A label change allocates over a hundred times a frame

> **Environments:** all · **Who it hurts:** Every app that updates text while it animates or scrolls · **Parity:** [Quality bar](platform-parity.md#quality-bar)

- `perf_scaling_a_label_change_costs_the_same_on_any_tree_size` (`crates/flui-widgets/tests/perf.rs`) changes one label over 500 and 5 000 buttons. The cost no longer grows with the tree: its allocations are equal at both sizes, and `perf --instructions` holds the instruction counts within 5%. Each frame still allocates more than a hundred times. The `label_change_*.toml` files of `crates/flui-widgets/perf/baseline/` hold the count, four frames per row.
- From a backtrace per allocation of that scenario, where each part goes:
  - **Text** (shaping and the shaped paragraph, about half): a painter whose text changes shapes into the buffers of the layout it dropped, so Parley allocates a fresh `Layout` only for a painter shaping for the first time; each shaping still fills Parley's style and break lists, and FLUI copies the spans' text, styles and run colours into the shaped paragraph. This lane is `flui-painting`'s `TextContext` and `TextPainter`, and it outlives the retained scene.
  - **Paint and layers** (about a third): fragment op vectors, display-list commands, and the `LayerTree` and `FragmentComposer` rebuilt every frame. This lane is the retained scene, `docs/plans/specs/render-scaling/` (RS4, RS8).
  - **View rebuild** (about a sixth): each rebuilt level hands its child to the reconciler in `ChildViews`, a child it holds as a `BoxedView` shared and unboxed, and the element clones the offered view without boxing it. What is left is the clone itself: an element copies the view it is offered, and a view that owns heap data (a `Text`'s string) allocates in that copy. This lane is `flui-view`'s element update path.
- The entry closes when a label change allocates at most 40 times a frame, held by those rows. It is measured after the retained scene lands; until then `cargo xtask perf --bless` on `main` ratchets each row a change lowered.

### Accepted ADR sections not built yet

> **Environments:** all · **Who it hurts:** Anyone who reads an accepted ADR as the code's current shape · **Parity:** [Quality bar](platform-parity.md#quality-bar)

- An accepted ADR is the rule; whether the code follows it yet is not its status ([`adr/README.md`](adr/README.md)). Where a section is not built, a bullet here names it as `ADR-NNNN §k` and the record's Status stays `Accepted (date)`; the pull request that builds the section removes its bullet, and the record becomes `Implemented (date)` when none is left.
- **ADR-0083 §3.** Beside `APP_RUNTIME`, `flui-app` keeps three secondary-window thread-locals (`crates/flui-app/src/app/runner/secondary_window.rs`), each a `globals` entry whose exit is ADR-0083.
- **ADR-0083 §4.** `HeadlessBinding::pump_frame`, `run_pipeline`, `pump_presentation` and `pump_all` still exist in `crates/flui-testing/src/lib.rs`.
- **ADR-0085 §5.** A graph no element reads still pays the map lookups, since `begin_element_build` (`crates/flui-view/src/reactive/mod.rs`) looks the element up on every build.
- **ADR-0089 §5.** `flui::testing::a11y` still re-exports `accesskit` (`crates/flui-testing/src/a11y.rs`, behind the facade's `testing` feature), `flui::android_activity` is still a facade item on Android (`src/lib.rs`), and `flui-sdk` has no `gpu` module, so `pub use ::wgpu` stays in the internal `flui-engine`, which no Stable crate re-exports.
- **ADR-0089 §6.** `cargo xtask api-closure` builds each Stable package once, on the host with its default features (`tools/xtask/src/api_closure.rs`), so the `testing` re-export and the Android item above are outside what CI checks.
- **ADR-0091 §3–§5.** None of the Verification checks exist apart from the threaded protocol harness in `crates/flui-engine/src/raster_owner.rs`'s tests: a two-window test that asserts one device and one glyph atlas, the atlas types pinned free of `Mutex`/`RwLock`, §5's per-platform pacing budget in CI for Linux with a recorded run for Win32, and the "one window blocked, the other within budget" test with its atlas contention counter.
- **ADR-0093.** Built: §1 without `#[nest]`, §3's handle, its typed result (`RouterHandle::push_for_result` and `pop_with`, `RouteResult` without `Send`) and `WidgetsApp::router`, and the stack restoration of ADR-0262 §6. Not built: `RouterHandle::maybe_pop` and `PageHandle`; `MaterialApp::router` and `CupertinoApp::router` (both shells take only `.navigator(…)`); the named routes, which `crates/flui-widgets/src/navigator/named_route.rs` and the `*_named*` doors, `push_keyed`, `route`, `on_generate_route`, `RouteArguments` and `RouteKey<T>` still carry; the URL owner (`RouterScope`, `UrlOwner`, `UiRuntime::navigate`, `NavigationIntent`, `on_unknown`), so no inbound URL reaches a Router; dialogs, popup and context menus, the selection popup and `OpenContainer` on a modal surface outside routes; and then the `NavigatorHandle` mutation doors, `maybe_of`, `maybe_of_root`, `WidgetsApp::navigator` and `NavigatorForm`, which stay public until those move. Nesting, guards and web history come after, as features.
- **ADR-0098 §3.** Containment is inclusive: `Rect::contains`, `Rect<f64>::contains_offset`, `Size::contains` and `Bounds::contains` (`crates/flui-foundation/src/geometry/`), and hit testing reaches the first through the clip render objects (`crates/flui-objects/src/proxy/clip.rs`), `HitShape::contains` (`crates/flui-objects/src/interaction/content_shape.rs`) and `BoxHitTestCtx::is_hit` (`crates/flui-rendering/src/protocol/box_protocol.rs`). The change makes a point on a clip's far edge stop reaching the clipped child.
- **ADR-0098 §5.** There is no `DeviceF64` and no sealed `Unit`: `snap_edges` and `cover` take the logical `Rect` (`crates/flui-foundation/src/geometry/snap.rs`), and the engine's device bounds are `Rect<f64>` (`crates/flui-engine/src/state_stack.rs`). The native Wayland host turns the n/120 scale numerator into an `f64` ratio (`crates/flui-native/src/window/linux/wayland.rs`) instead of sizing the buffer in integers from it; the Windows and web rules are in place.
- **ADR-0098 §6.** Content snapping (solid, gradient and image quads; animated layers unsnapped), border and stroke widths resolved in layout (`resolve_stroke_width` has no caller outside its module) and text baseline snapping. They move every readback and need the engine to know which layers animate, so they land as one change with readbacks that tell a snapped edge from an antialiased one.
- **ADR-0098 §7 and §8.** `LinearGradient` carries no interpolation colour or alpha space (`crates/flui-painting/src/styling/gradient.rs`), and `Vec2`, `Bounds` and `Circle` are not pruned to their consumers.
- **ADR-0102 §5 and ADR-0103.** The clip and coverage readbacks (`hard_clip_membership_scales_to_a_full_hd_frame`, `coverage_blend_reads_back_as_specified`) have run on native adapters only, not on a browser or a mobile GPU.
- **ADR-0142 item 4.** Built: the host contract, the owner's `complete_composition` with the `Abandoned` path, the field's commit on blur, on a pointer-down on it and on a paste, and the runtime's commit on a pointer-down anywhere in the presentation. The commits on an accepted close request, at the end of the session, on undo and on unmount, and the outgoing client's commit when an attach replaces it, land with the widget's composition handling.
- **ADR-0153 items 3 and 5.** The `cargo-semver-checks` report runs on every pull request and push against its base commit, on `flui-platform` only (`.github/workflows/ci.yml`), not in a train's release against the previous published train, and not on the five `flui-foundation` items the contract names. `cargo xtask workspace`'s `capability` kind rule does not scan a capability crate for `PlatformWindow` or contract paths outside the seam.
- **ADR-0255.** Of Android, decisions 1, 4, 5 and 7 are built, decision 2's queue with `onTrimMemory` (the other entries come with their consumers) and decision 6's Activity, `hasCode`, `singleTop`, the back flag and `configChanges` (the dex is a declaration of `flui-native`, ADR-0307 version 2; the handshake is `owner::android::java_host`), and the Gradle path of the build table: the crates' sources through `flui.javaSources` and their R8 rules (`dev.flui.**` kept) through `flui.proguardFiles`. The iOS and Web decisions land as separate changes; text input and accessibility on each are [No text input on mobile and the Web](#no-text-input-on-mobile-and-the-web) and [No accessibility on mobile and the Web](#no-accessibility-on-mobile-and-the-web), and a platform's row in [`platform-parity.md`](platform-parity.md) moves only after a run on that platform.
- **ADR-0213.** The recording round trip of `PlatformInput::Capabilities` is not built, and presence changes from the native Win32, AppKit, UIKit, Android and web backends are not either: they report their initial table (the headless, Wayland and X11 backends report changes). Until DirectManipulation lands, Windows reports precise scrolling and trackpad gestures `Unavailable`.
- **ADR-0215 §5 and §8.** `--keep-text` is the API's `Privacy::KeepText`; the command-line flag arrives with a recording tool. Injection into a real window (§8) is built on Windows only (`cargo xtask device windows-input`): the precision touchpad is journaled but certified only by a hardware recording, and the other OSes are pending.
- **ADR-0240 §2.** On Android the identifier does not reach `viewIdResourceName`: the published `accesskit_android` sets no resource name from `author_id` ([No accessibility on mobile and the Web](#no-accessibility-on-mobile-and-the-web)). It is built when an AccessKit release maps it.
- **ADR-0243 D2.** `flui_view::testing` still holds `ElementBuildContext`, an owned context a test builds for a mounted element outside `BuildOwner::build_scope` (`crates/flui-view/src/testing.rs`). It records dependencies straight into the owner, which the production context never does, so it is `#[deprecated]`. Five tests still read it (`ancestor_finders.rs`, `inherited_dependency.rs` and `notifications.rs` under `crates/flui-view/tests/`); each moves to a view that reads its context inside a build, and the type is deleted with the last.
- **ADR-0254 §6–§8.** Decisions 6 to 8 are not built; they land as separate changes, in the order of the platform-layer feature's tasks.
- **ADR-0277 §1 and §4.** The Web safe area (§4) is not built, and `SYSTEM_KEYBOARD_BEZIER` is not checked against a device recording (§1).
- **ADR-0287 Migration.** Only the capability seam's `Completion` and `Persisted` use `flui_foundation::panic`; each other seam moves to it in its own change. Every hand-written `catch_unwind` left carries an `ADR-0287:` expectation of `clippy::disallowed_methods` (ADR-0303 §6), so those expectations are the migration's remaining count.
- **ADR-0304.** Paint writes the retained scene: each repainted boundary is published as its own node, a clean one keeps its content, and the patch is applied all or nothing (`crates/flui-rendering/src/pipeline/owner/paint.rs`, `crates/flui-layer/src/scene/`). Each presentation owns its `SceneOwner`, composes the root around the pipeline's frame node and the debug overlay, and publishes the version with the damage since the version the raster side acknowledged (`crates/flui-runtime/src/presentation_scene.rs`); `FrameSink::submit` takes that `ScenePublication`, and the raster lane applies its damage only to the image of its base (`RasterBase`, `crates/flui-layer/src/damage/base.rs`). Left: the engine still draws the version flattened into a `LayerTree` and records it whole every frame (`SceneSnapshot::of_version`, `crates/flui-engine/src/damage.rs`); `RasterCompletion` names no scene version (`crates/flui-engine/src/raster_owner.rs`); the buffers a completion releases have no recorder pool to go back to, so paint allocates per frame; `resolve_follower_transform` still takes a `LayerTree`; a refused submit repaints the root boundary instead of resubmitting the version (ADR-0068 §3); the web's `DirectSink` draws every frame in full; `LayerDiffer`, `ContentToken` and `BoundaryStamp` live on behind `flui-layer`'s `testing` feature as the oracle of the engine's damage readbacks.
- **ADR-0305.** `GpuContext::bootstrap` makes the context and its `SurfaceFactory` on the host's `DisplayConnection`, and an inline `RasterExecutor` drives offscreen and window slots with `Release` and `Restore` (`crates/flui-engine/src/gpu_context.rs`, `crates/flui-engine/src/surface_factory.rs`, `crates/flui-engine/src/window_slot.rs`, `crates/flui-engine/src/raster_executor/`), but no runner calls `bootstrap` yet, so no window presents through a slot; window slots are tested only on wgpu's no-op backend. A WebGL2 context presents to one canvas: a second canvas on the GL backend is refused at attach with `EngineError::IncompatibleSurface` (wgpu-hal's GLES adapter answers no capabilities for another canvas's context), and only WebGPU serves several canvases. No threaded placement, `render_lent` or `UiRuntime::settle_frame` yet; the executor rebuilds a lost device on the context's instance (§8), but `RetryDevice` rebuilds there too instead of running ADR-0244's probe in fresh instances with a new factory, the web cannot rebuild (the inline executor cannot wait for wgpu's request there, so a lost device is given up at once), the inline rebuild blocks the owner thread for the adapter and device request (hundreds of milliseconds on a real driver after a reset, paid again by each failed attempt) until the threaded placement, and until the runners move to the executor ADR-0267's backoff is held twice, by the executor's `DeviceRebuilds` and by `flui-app`'s `DeviceRecoveryBackoff` (`crates/flui-app/src/app/runner/device_recovery.rs`), whose numbers must agree; §7's `ContextEpoch` does not exist, since no slot holds a recording derived from the shared caches yet (a panicking frame drops the painter's caches and keeps its pipelines); an offscreen slot is cleared before its frame records, so a frame that panics leaves its target cleared instead of holding its last image, and the frame's `Retry` is what redraws it; the context has no GPU profiler, and the context's pipelines, glyph atlas and image cache live in the one painter it shares between slots instead of in fields of their own. The executor's seam is named `SlotBackend` until the window runners move to the executor, which deletes the deprecated `RasterBackend`, `RasterOwner` and `RasterHandle` and renames `SlotBackend` to `RasterBackend`. §6's `GpuResourceGeneration` axis stays until the runners rebuild their device through the context. Every window still builds its own `Renderer`, with its own instance, device, pipelines and glyph atlas (`crates/flui-app/src/app/runner/window_renderer.rs`), behind an `Arc<Mutex<RasterLane>>` (`crates/flui-app/src/app/runner/desktop.rs`), inline on every platform; the web renders through `DirectSink`. The Migration section gives the order, and a platform's placement becomes threaded only after a run on that platform (§10).
- **ADR-0306 §1–§3, §5.** Every command still stores its own `Matrix4` (`crates/flui-painting/src/display_list/command.rs`); `DRRect`, `ImageRegion` and `ImageFiltered` hold their payloads inline and `Path` its hint, bounds and finiteness flag (`crates/flui-painting/src/paint/path.rs`); no size assertion exists; the reader still exposes `commands()`, `Index` and `DrawCommand::untransformed`.
- **ADR-0311 §1–§6.** `DevReloadHook` is still `Send + 'static` and still carries `scene_frame` (`crates/flui-view/src/dev_reload.rs`); there is no `DevSceneHook` or `AppConfig::with_dev_scene`, so the Android runner calls `scene_frame` on the reload hook. `AppConfig::with_dev_reload` takes an instance, not a factory (`crates/flui-app/src/app/config.rs`): every clone of the configuration shares it, and a second loop's attach is refused instead of building an instance of its own. The host keeps the instance in `Arc<Mutex<_>>` and contains each call with a hand-written `catch_unwind`, not `FirstPanic` and `drop_contained` (`crates/flui-app/src/app/hot_reload.rs`), and the web runner does not log that it ignores an installed hook. `flui-hot-reload` is still under `crates/`, with four `edge-exceptions` (to `flui-foundation`, `flui-layer`, `flui-rendering` and `flui-view`) where the plugin ABI's items belong in `flui_sdk::hooks`; the facade still names it through its `hot-reload` feature, which `flui create --hot-reload` turns on.
- **ADR-0312 §7.** `ClipboardFormat`, `ClipboardAsk`, `ClipboardHandle::read` and `ClipboardWrite::with` do not exist; they land with the first widget that reads or writes more than text (a `SelectionArea` copied as HTML, a pasted image), with its headless tests.
- **ADR-0313 §3.** The render slab keeps its hand-rolled slot generations (`crates/flui-rendering/src/storage/node_slab.rs`, its `RETIRED` sentinel) instead of the ledger; the adoption lands with #2241.
- **ADR-0322 §1–§3, §7.** Built: §4 (`StatefulRenderView`, `RenderViewState`), §5 (`ProvidedValue`, `Provided`, `provide`), with `Focus`, `FocusScope` and `Actions` publishing from their own elements, and of §1 `RenderInteractive`, the `FocusHost` and `RecognizerHost` that the wrappers and the node run on, and the node itself with its pointer, tap, focus, focus-ring, action and semantics parts, which `RawButton` mounts; §6 holds for `RawButton` (`raw_button_a11y_tree_matches_wrappers`, `raw_button_in_scrollable_contests_like_gesture_detector`, `nested_raw_button_contests_like_gesture_detector`). The node is `#[doc(hidden)]` `pub` for the catalogs, with the tap family (`ControlTap`) and `InkWell` mounts it. Not built: the node has no cursor and no recognizer slots beyond the tap family (long press, double tap), and no catalog control but `InkWell` mounts it (§1, §7); the node's `enabled` switches its semantics, pointer, focus and action parts, its states and hover rows hold (`raw_button_writes_its_states_to_the_controller`) (§2); the node writes its five states to the controller it is given and a painter that returns the controller from `repaint` follows them with no build (`hover_repaints_without_rebuilding`); no catalog control reads its states at paint, and `InkWell` rebuilds on every state change (`packages/flui-material/src/ink_well.rs`) (§3). The order is §2 and §3 on the node, then the control families; the numbers that close it are in [An interactive widget costs eighteen elements](#an-interactive-widget-costs-eighteen-elements).
- **ADR-0323 §3–§5.** `cargo xtask send-positions` knows §2's seven `crosses` values but does not require one yet: the 106 of the 240 listed entries that name none are §4's removals (`tools/xtask/allowlists/send-positions.toml`). None of §4's removals is made: `Localizations::of` returns `Arc<R>` and asks `R: Send + Sync`, route arguments, `StreamBuilder`, the animated lists, `ObjectKey::new`, `ImageProvider` and the window capability traits of `flui-platform` keep their thread bounds. §5's owner-thread locks remain in `crates/flui-widgets/src/navigator/navigator.rs`, `crates/flui-view/src/element/future_builder.rs`, `crates/flui-scheduler/src/scheduler.rs`, `crates/flui-interaction/src/pointer/resampler.rs`, `crates/flui-view/src/owner/build_owner.rs` and `crates/flui-interaction/src/gesture/arena/mod.rs`, and no `disallowed-types` rule names the lock types.

### Two rest distances for logical pixels

> **Environments:** all · **Who it hurts:** Apps that mix a scroll fling with an animated offset, size or rect on a dense display, where the two settle at visibly different precision · **Parity:** none (animation has no row)

- A scroll fling rests within half a device pixel: `Tolerance::for_device_pixel_ratio` gives `0.5 / dpr` logical pixels (`crates/flui-animation/src/simulation.rs`, used by `crates/flui-widgets/src/scroll/scroll_physics.rs`; ADR-0155, ADR-0200). At a ratio of 3 that is about 0.17 logical pixels.
- Animated geometry rests within a fixed 0.01 logical pixels whatever the display: the converters' rest thresholds in `crates/flui-animation/src/value_conversion.rs` (ADR-0186), and the controller's fling tolerance `FLING_TOLERANCE` in `crates/flui-animation/src/controller.rs`. That is about 17 times finer than the fling at a ratio of 3.
- No record chooses one source for rest in logical pixels or says why the two differ. A candidate ADR after the render-scaling gates.
- The entry closes when one rule decides rest in logical pixels for both, or an accepted record states why they differ, with a test of each.

### No scroll notifications

> **Environments:** all · **Who it hurts:** Widgets that react to a scroll view below them (a pull-down header, an infinite-scroll loader, a scroll-linked fade) · **Parity:** none yet (scrolling widgets have no row)

- Flutter's `ScrollNotification`, `ScrollMetricsNotification` and `NotificationListener` have no FLUI counterpart. An ancestor reaches the scroll view nearest below it only through the crate-private scroll observer scope (`crates/flui-widgets/src/scroll/scroll_observer.rs`), whose one user is `Scrollbar`; other code shares a `ScrollController` by hand.
- The likely shape is a public listener that installs the observer scope and reads the attached position, so a scroll frame costs one listener call and nothing bubbles through the tree.
- The entry closes when a public listener reports a scroll start, update, end and a change of extents for the scroll view at depth zero below it, with headless tests over a nested list.

### Widget states still fill their handles in `init_state`

> **Environments:** all · **Who it hurts:** Contributors copying a catalog widget's state · **Parity:** none (API shape)

- `StatefulView::create_state` receives the `LifecycleContext`, so a state can be built with its signals and capability handles bound; `Signal::default()` is gone. Production states still hold an `Option` (a `WriterSource`, a focus or text-input handle, a rebuild handle) that `init_state` fills with `self.field = Some(ctx.…)` and that `build` unwraps: `rg 'self\.[a-z_]+ = Some\((ctx|cx)\.' crates packages` lists them.
- The check is that search finding no production state, each field a plain handle made in `create_state`.

### No widget hot reload on Android and the Web

> **Environments:** Android, Web · **Who it hurts:** Developers who iterate on widget code on a phone or in a browser · **Parity:** none yet (hot reload has no row)

- Desktop and iOS reload widget code in place through `DevReloadHook` (ADR-0311 §4): a worker edit reassembles every UI runtime with its `State` kept. The Android runner drives only the scene hook, so `flui run --scene` reloads a `Scene` plugin, not the widget tree. The web drives no reload hook: `flui run --device browser:<browser>` reloads the page on every rebuild, and the page's state with it ([`hot-reload.md`](hot-reload.md), "WASM / Web").
- The likely shape on Android is the desktop's worker path, pushed to the device as the scene plugin is; on the Web, which has no `dlopen`, a code patcher (ADR-0311 §7).
- The check is a widget edit on an Android device and in a browser that reloads with a stateful counter's value kept.
