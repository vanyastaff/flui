# Notes showcase

Notes is a three-screen application (Home, Note, Settings) built on the
two-screen Router example. The application tree lives in [`tree.rs`](tree.rs);
[`../two_screens.rs`](../two_screens.rs) is the launcher that runs it.

## What it demonstrates

- **Typed navigation.** `WidgetsApp::router` over a `#[derive(Routable)]` route
  enum; screens push and pop routes through a `RouterHandle` acquired in
  `init_state`.
- **State above the router.** The root owns the note titles, the selected
  draft, the compact-rows preference and the list's scroll controller, so they
  survive navigation. Everything is in memory: restarting resets it.
- **Loading, error and retry.** Home loads through `FutureBuilder::keyed`. The
  default readiness fixture fails its first request and succeeds afterwards;
  Retry and "Reload notes" start a new keyed request, replacing a pending one.
  `NotesApp::with_loader` swaps in your own readiness service.
- **Lazy scrolling.** A 10,000-row `ListView::builder` inside a
  controller-backed `Scrollable`. Compact rows change the row extent from 48 to
  32 logical units.
- **Validated editing.** The Note screen's title field selects all with Ctrl+A
  (Cmd+A on Apple targets). Saving an empty or whitespace title is rejected; a
  valid save updates the Home row. The Material buttons take keyboard focus and
  activation, so Tab, Shift+Tab and Enter reach Save.

Opening a different note replaces an unsaved draft; there is one draft at a
time and no disk persistence.

## Run it

```bash
cargo run --example two_screens --features material
```

The `material` feature is required: Notes uses Material's `TextFormField` and
`TextButton`.

## Test it

The headless acceptance flow, [`tests/fixtures/notes_flow.rs`](../../tests/fixtures/notes_flow.rs),
drives this same tree through the public `flui` facade with dispatched pointer
and keyboard input. Its `notes_public_input_flow_matrix` table covers:

- loading, error, Retry and Reload, including a pending request replaced by a
  newer one or by unmounting, whose late completion must not change the UI;
- keyboard editing: an invalid Save keeps the old Home title, and a valid edit
  saved through Tab and Enter updates it;
- a draft retained across Settings and Back, and the row spacing that the
  density preference changes;
- a drag that moves the visible rows, and the scroll position kept across
  navigation.

Assertions read on-screen geometry, hit regions and the recorded scene's text.
The flow runs inside a generated external consumer project, once with the
dependency named `flui` and once renamed to `ui`:

```bash
cargo test -p flui --test facade_consumer external_notes_showcase_runs_through_the_facade
```

On Windows, a native driver launches a release build (`material,a11y`) and
works through the same flow with OS pointer and keyboard input and UI
Automation, then closes the window:

```bash
cargo xtask device windows-notes
```

It needs an interactive desktop and skips elsewhere.

## Current limits

- No IME/TSF composition: native typing is plain key and character input.
- No screen-reader announcement check: the driver reads UI Automation values
  and invokes actions, but does not listen to Narrator.
- No GPU pixel check: the headless flow asserts the recorded scene, not
  rendered pixels.
