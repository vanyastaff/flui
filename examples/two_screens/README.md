# Notes public consumer

Status: the shared application tree and all four public headless acceptance rows
compiled and executed through both ordinary `flui` and renamed `ui` consumers at
`d7e5d80ec`. The native Windows run at `cae2a4d74` built and executed through
resize, Error/Retry/Home, 48-pixel row spacing and pointer opening Note 0, then
failed at an incorrect driver text selector for the form's merged group name.
Native editing, remaining navigation, scroll retention and graceful close remain
unverified. The [central release evidence](../../docs/plans/2026-10-04-community-release.md)
is authoritative for candidate revisions, commands, results and limitations.
The source extends the existing two-screen Router example, rather than adding
another showcase. Root-held titles, selected draft, preferences and scroll
controller survive navigation in memory; restarting the process resets them.

The isolated consumer has one direct framework dependency, `flui`, with
`material`; `testing` is a development feature. Its launcher and the repository
example include the identical `tree.rs`. The facade consumer runner copies that
tree and `tests/fixtures/notes_flow.rs` into ordinary and renamed external consumers and
requires `notes_public_input_flow_matrix` to execute. A local path dependency
does not establish registry availability or package closure.

## Application behavior

Home loads a local offline readiness fixture: its first request fails, Retry
starts a new request and subsequent requests succeed. `NotesApp::with_loader`
accepts a readiness service for the same production loading/error/retry tree.
Reload notes replaces its keyed request, including while an older request is
pending. The default fixture is deterministic, not network IO; the injectable
service lets the public acceptance scenario hold completion and inspect
replacement/unmount cancellation without duplicating the application tree.

Ready displays a 10,000-row `ListView::builder` composed by
`Scrollable::controller` and `viewport_builder`, sharing the root controller.
Drag and wheel use the existing Scrollable input path; fling depends on the
host's animation binding. Those native capabilities still need execution.
Compact preference changes fixed row extent from 48 to 32 logical units; row
Material TextButtons admit that compact size. Buttons use existing Material
focus, shortcuts, activation and semantics rather than pointer-only RawButton.

Selecting a row opens Note. The focused title's standard editing action handles
Ctrl+A (Cmd+A on Apple targets) through the dispatched event, selecting its whole
text without changing the document.
Empty/whitespace Save rejects the edit; valid Save
changes the actual saved Home row. Settings and Back preserve the current draft,
saved data, compact preference and scroll pixels. Changing density can change
the first visible row at the same scroll offset. Selecting a different note
replaces an unsaved draft; there is no multi-draft or disk persistence promise.
Application-model retention does not establish element retention.

## Public headless acceptance family

`tests/fixtures/notes_flow.rs` replaces the former single scenario with one family table:

- Held service completion: submitted Loading → Error → pointer Retry → Loading;
  Reload retires the pending request; completing the old request cannot replace
  the latest state; latest success paints rows. Root replacement during another
  pending request retires it and late completion cannot change the replacement.
- Dispatched keyboard edits: invalid Save preserves the Home title; valid edit,
  Tab/Shift+Tab/Tab and Enter invoke Save and update the actual Home row.
- Unsaved draft survives Settings/Back; density changes measured row spacing.
- Pointer drag changes the lazy visible band and navigation preserves its row
  position. A distant row is absent from residency; total builder-admission
  counts remain a separate oracle, since absence alone cannot reject every eager
  construction strategy.

Assertions use onstage geometry, actual hit regions and the committed scene's
text commands, rather than finding labels in retained offstage pages. Timed
`pump_for` frames drive notifications without forcing root rebuilds and allow the
default 300ms page entrance to finish before asserting one active page. The drag
scenario holds its final position before release so ballistic progress cannot
confound its retained-offset assertion. The scene oracle requires
the public testing re-exports of `collect_commands` and `DrawKind`. It establishes recorded
paint content, not GPU pixels or native accessibility.

## Commands and evidence

The commands below are entry points, not claims that every exact invocation ran.
The headless consumer results above are recorded in the central release evidence.
Native launch and the Windows Notes driver remain unverified; execution requires
the shared compiler and native-driver slot.

```text
cargo xtask --help
cargo check --example two_screens --features material
cargo check --manifest-path examples/two_screens/consumer/Cargo.toml
cargo test -p flui --test facade_consumer external_notes_showcase_runs_through_the_facade --features material,testing -- --nocapture
cargo xtask facade-combos
cargo xtask check-changed
cargo run --manifest-path examples/two_screens/consumer/Cargo.toml
cargo xtask device windows-notes
```

After public consumer gates, use the same source on every advertised platform
for real input, resize, clipboard/IME, assistive names/actions, pixels and close/
quit. Record candidate SHA, OS/backend/GPU, features, command, artifacts and
passed/failed/skipped/unavailable separately. None are certified by this source.

The private Windows Notes driver uses this same production tree with `material,a11y`.
It combines real pointer input, physical Ctrl+A/Tab/Enter key events, UTF-16
Unicode character packets and normal UIA button actions, observing current editor
values, Home titles and physical bounds. Those observations must follow input;
successful injection alone cannot establish a committed edit. This is plain native
keyboard/UIA acceptance, not actual TSF/IME composition, Narrator announcements or
GPU pixel evidence. Deterministic Retry does not establish native held-loader
replacement or unmount cancellation; those remain the separate headless scenario.

Before accepting behavior repairs, isolate negative controls and restore original
bytes with no overlapping build: removing the default select-all action or shortcut must
break consumed-key/edit acceptance; pointer-only Save must break Tab/Enter acceptance;
constant request key must break replacement/retry; omitted validator must break
invalid Save; bypassed save must break active Home paint; route-local model must
break draft retention; removed shared scroll position must break navigation.
Final source must pass again after restoration. GPU/readback and performance
budgets belong to their owners and require independent evidence.
