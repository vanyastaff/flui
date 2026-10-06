# Notes: from Todo to a showcase

[Counter → Todo](tutorial-todo.md) introduces state, input and a list in one screen.
Notes combines those ideas across Home, Note and Settings. It extends the existing
two-screen Router example: the application tree is
[`examples/two_screens/tree.rs`](https://github.com/vanyastaff/flui/blob/main/examples/two_screens/tree.rs),
and its [README](https://github.com/vanyastaff/flui/blob/main/examples/two_screens/README.md)
summarizes what it demonstrates and how it is tested.

## Run it

From the checkout root:

```bash
cargo run --example two_screens --features material,persist
```

Notes uses Material's `TextFormField` and `TextButton`, so the `material` feature is required, with `persist` for the storage its saved state is written through. Counter and
Todo use the theme-free catalog and do not need it. See [Themes](../cookbook/themes.md)
for how a theme surrounds the tree.

## Follow the flow

Walk through these steps by hand; each says what you should see.

1. Home should reach "Load failed" after its initial loading state. Press "Retry"; the Notes
   list should appear. The default fixture deliberately fails once and yields Pending only
   once per request, so loading may be too brief to observe manually. The controlled-service
   acceptance scenario below holds completion to check loading separately.
2. Drag the list to a later note and open it. The editor should show "Editing note" with its
   id and a Title field containing that note's title. The list is a `Scrollable` driven by
   the root's controller: it scrolls by drag and wheel, and flings through the host's
   animation binding.
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
6. Close and launch again. The initial generated titles return: Notes has no disk
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
the latest request changes the current UI, against the same application tree. Use
[Async builders](../cookbook/async.md) to learn the builder pattern, and replace the fixture
with your own IO only after deciding cancellation and error handling for that application.
For validation, continue with [Forms](../cookbook/forms.md).

## Verify before extending it

For application tests, enable `testing` on the `flui` dev dependency and start with the
[testing cookbook](../cookbook/testing.md) and
[facade testing guide](https://github.com/vanyastaff/flui/blob/main/docs/testing.md#application-tests-through-the-facade).
The source README owns the detailed acceptance scenarios; keep that checklist with the
application rather than copying it into several guides.

The headless `notes_public_input_flow_matrix` in `tests/fixtures/notes_flow.rs` runs this
tree through the public facade with dispatched input. It checks controlled loading, retry,
request replacement and unmount; keyboard editing with an invalid and a valid Save; draft and
density retention across Settings; and drag and scroll retention across navigation. Its
assertions read on-screen geometry, hit regions and the recorded scene's text, so it does not
cover native input, IME, assistive technology or GPU pixels.

Rendered loading/error/retry, dispatched editing input, distant-row scrolling, validation
and navigation retention need observable assertions. Directly setting the controller's text
can prepare a fixture but cannot prove typing works. Closing or unmounting during pending
work must also be checked for stale completions and clean teardown. Native input, IME,
clipboard, assistive technology and pixels need real-platform verification; compilation or
a headless semantics tree alone cannot establish those results.
