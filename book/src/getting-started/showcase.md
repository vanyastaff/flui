# Notes: from Todo to a showcase

[Counter → Todo](tutorial-todo.md) introduces state, input and a list in one screen.
Notes combines those ideas across Home, Note and Settings. It extends the existing
[two-screen example](https://github.com/vanyastaff/flui/blob/main/examples/two_screens.rs)
rather than adding another application to learn.

The Notes public headless consumer flow has passed. Native execution, GPU checks,
renamed-dependency verification and real distribution remain pending; this result does not
establish release readiness. Its shared `tree.rs` and
`README.md`, beside the two-screen launcher in its matching source directory, are the
references for the behavior described here. Those additions must be integrated before this
walkthrough can be followed from a main checkout. Release
readiness belongs in the [beta criteria](https://github.com/vanyastaff/flui/blob/main/docs/BETA.md),
not in this tutorial.

## Open the source and launchers

Use a checkout that contains the Notes draft. Check the two-screen launcher first: it must
include the shared Notes tree and call `flui::run_app(tree::NotesApp::default())`. The older two-screen
Router demo does not have the flow described below. Do not replace your working checkout
just to obtain the draft; use the candidate or separate checkout supplied for review.

From that checkout's root, these are the commands to verify:

```bash
cargo run --example two_screens --features material
```

Notes uses Material's `TextFormField` and `TextButton`, so the `material` feature is required. Counter and
Todo use the theme-free catalog and do not need it. See [Themes](../cookbook/themes.md)
for how a theme surrounds the tree.

There is also an independent manifest for the same application:

```bash
cargo run --manifest-path examples/two_screens/consumer/Cargo.toml
```

The draft's `consumer/Cargo.toml` manifest
has its own workspace boundary and depends directly on `flui` with `material`. Its launcher
includes the same tree as the repository example. It still resolves FLUI and the tree from
this checkout; it demonstrates a source arrangement to verify, not registry installation or
an application copied free of the repository. Follow [Installation](installation.md) for the
current first-application path.

## Follow the flow

Treat the following as a manual verification script for your platform. Record the
candidate revision, OS and failures rather than assuming a source-level callback proves the
interaction works.

1. Home should reach "Load failed" after its initial loading state. Press "Retry"; the Notes
   list should appear. The default fixture deliberately fails once and yields Pending only
   once per request, so loading may be too brief to observe manually. The controlled-service
   acceptance scenario below holds completion to check loading separately.
2. Drag the list to a later note and open it. The editor should show "Editing note" with its
   id and a Title field containing that note's title. The current tree connects the list to
   `Scrollable` with the root's controller. Its drag and wheel paths, and fling through the
   host animation binding, need execution on each advertised platform.
3. Focus Title, press Ctrl+A (Cmd+A on Apple targets), then Backspace. The standard
   `SelectAllTextIntent` editing action should select the whole text before deletion.
   Press "Save note". The form should
   show "Enter a title" and the status should say "Fix the title". Return Home and check that
   the saved title has not changed.
4. Open that note again, type a new title, then visit Settings before saving. Press "Toggle
   compact rows" and return with "Back". The unsaved draft should remain in the editor.
5. Use Tab to move from Title to Save, Shift+Tab to return, then Tab and Enter to save. The
   status should say "Saved note" with the id. Return Home and check the updated title,
   compact row height and retained scroll pixels. Density changes can change which row is
   first visible at the same offset. Reopen the same note.
6. Close and launch again. The initial generated titles return: this draft has no disk
   persistence. Saving changes application memory during the run.

A draft is retained for the currently selected note. Opening a different note replaces the
shared draft with that note's saved title; save before switching notes if you want to keep
an edit. Here "retained state" means state across navigation within the running app.

## Read the changes from Todo

| Todo idea | Notes extension | Where to look in the shared tree |
|---|---|---|
| One screen owns its item signal | A root owns the titles, selected draft, scroll and preferences, and passes handles into screens | `Shared`, `NotesRoot`, `NotesState` |
| Callbacks change a list | Save validates the form before updating the saved title | `editor` |
| Build all rows from a small list | Build row views on demand inside a controller-backed viewport | `home`, `Scrollable::viewport_builder`, `ListView::builder` |
| Enter or Add submits text | A title editor has a focus node; Material actions use `on_pressed` and support keyboard activation | `ScreenState`, `editor`, `TextButton` |
| No navigation | Typed routes select Home, Note and Settings; a lifecycle hook acquires the router handle | `Route`, `ScreenState::init_state` |
| Immediate in-memory data | A replaceable readiness service exposes loading, error, retry and reload states | `NotesApp::with_loader`, `home`, `FutureBuilder::keyed` |

The tree uses `StateHandle` bound in the root's lifecycle hook rather than Todo's `Signal`.
Read [State](../concepts/state.md) for those APIs; this change is not a prerequisite for adding
navigation to your own signal-based application. Keeping the owners above the router lets
screens share application data and controllers. It does not prove that a page's element
survives replacement. See [Lifecycle](../concepts/lifecycle.md) for the separate element lifetime.

The row builder is lazy, but the model still allocates 10,000 strings. Home clones a title
snapshot for displaying row labels, and the viewport builder clones that snapshot. Opening a
different selected note reads its title from the current shared model, rather than the label
snapshot; reopening the selected note preserves its draft. The headless consumer checks
the painted Home title after Save. Avoid interpreting a lazy view list as a memory or
frame-time result.

Retry and "Reload notes" increment the future's key. The default fixture yields Pending once,
deliberately fails its first attempt and succeeds on a later attempt. It performs no HTTP
request. `NotesApp::with_loader` accepts an `Rc` readiness service that receives the attempt
number and returns a future. The consumer flow uses it to hold requests pending,
inspect replacement and unmount retirement, complete an old request, and check that only
the latest request changes the current UI. Those passing headless cases exercise the same
application tree. Use
[Async builders](../cookbook/async.md) to learn the builder pattern, and replace the fixture
with your own IO only after deciding cancellation and error handling for that application.
For validation, continue with [Forms](../cookbook/forms.md).

## Verify before extending it

For application tests, enable `testing` on the `flui` dev dependency and start with the
[testing cookbook](../cookbook/testing.md) and
[facade testing guide](https://github.com/vanyastaff/flui/blob/main/docs/testing.md#application-tests-through-the-facade).
The source README owns the detailed acceptance scenarios; keep that checklist with the
application rather than copying it into several guides.

The public headless `notes_public_input_flow_matrix` in `tests/fixtures/notes_flow.rs` passed all four
scenarios: controlled loading/retry/replacement/unmount, dispatched edits and keyboard Save,
draft/density retention, and drag/navigation retention. Removing the default Select All
shortcut made both editing scenarios fail at the dispatched key; restoring it made the
whole matrix pass again. This confirms that those scenarios distinguish missing shortcut
wiring. The [beta criteria](https://github.com/vanyastaff/flui/blob/main/docs/BETA.md) remain
the authority for candidate limitations and verification. Renamed-dependency consumers,
GPU pixels, native behavior and real distribution still need their own checks.

Rendered loading/error/retry, dispatched editing input, distant-row scrolling, validation
and navigation retention need observable assertions. Directly setting the controller's text
can prepare a fixture but cannot prove typing works. Closing or unmounting during pending
work must also be checked for stale completions and clean teardown. Native input, IME,
clipboard, assistive technology and pixels need real-platform verification; compilation or
a headless semantics tree alone cannot establish those results.
