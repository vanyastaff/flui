# ADR-0349: The window contract states each fact in one shape, and every window says what it does

- **Status:** Accepted (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0254](ADR-0254-window-contract.md) §2 only for its paragraph on
  `reveal_after_first_frame` (the reveal stays a method the embedder calls)
- **Related:** [ADR-0254](ADR-0254-window-contract.md) §1 (one source per window fact) and §4 (no
  inert default bodies), [ADR-0222](ADR-0222-window-appearance-is-a-presentation-observation.md)
  (the theme a window reports is the user's), [ADR-0237](ADR-0237-window-state-and-host-notices.md)
  (the snapshot and its commands), [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md)
  (the native recording mirrors the snapshot)

## Context and problem statement

ADR-0254 gave every window fact one source, the `WindowState` snapshot, and every obligation one
required method. The contract (`flui-platform`) still said some facts two ways, and let a window
inherit an answer it never chose. Each was read on `origin/main` before this record:

| Shape | What was wrong |
|-------|----------------|
| `WindowState::appearance: WindowAppearance` (`Light`, `Dark`, `VibrantLight`, `VibrantDark`) | A theme and a material in one enum. No host produced a vibrant value; `Brightness` already named the theme and `WindowMaterial` the material, and `flui-app` converted every report to `Brightness`. |
| `WindowState::{focused, active}: bool` | `focused && !active` was representable and published: Win32 sends `WM_ACTIVATE(WA_INACTIVE)` before `WM_KILLFOCUS`, and an X11 window manager writes `_NET_ACTIVE_WINDOW` after `XGetInputFocus` already names the window, so a read between the two published a window holding the keyboard that was not the active one. A hidden window could be published focused. |
| `CursorError::Backend(String)`, `PreferenceQueryError::Native { message }` | Two more shapes of the refusal `ApplyError::Platform(PlatformFailure)` already states. |
| `PlatformWindow::{gesture_geometry, window_at, reveal_after_first_frame}` | Default bodies (`Ok(None)`, `WindowAt::Unknown`, nothing) on a trait whose other facets say "every implementation says which; there is no default". A host that defers its reveal and forgets the override never shows its window. |
| ADR-0254 §2 | Decided that `reveal_after_first_frame` is not a method, the reveal policy being data in `WindowOptions`. The code kept the method because the host cannot know when a frame was presented: the engine presents, and the embedder also reveals at a bounded fallback after a frame that ran and presented nothing (`docs/gaps.md` recorded the disagreement). |

The question: what shape does each of these take, so the type says the fact once and a window
cannot inherit an answer?

## Decision drivers

- An invalid state is unrepresentable; a fact has one type.
- Accepted work is never dropped because a hook is missing (AGENTS.md, "Recurring defects").
- A host whose window system reports two facts out of step gets one rule at the seam, the same on
  every host, headless included.
- A recording and the agent protocol keep reading what was written before (ADR-0342: removing a
  value from a closed union is a break).

## Considered options

For focus and activation:

1. Keep two booleans and have `successor` force `active` when `focused`.
2. `WindowFocus::{Inactive, Active, Focused}`, with one constructor every host reads its two
   reports through.

For the reveal:

- (a) Delete the method, as ADR-0254 §2 decided, and have each host reveal on its own first
  present or after a deadline.
- (b) Keep the method and make it required.

## Decision

Chosen: option 2 and (b), because the type then carries the rule, and only the embedder knows that
a present succeeded.

### 1. The theme is a `Brightness`

`WindowState::brightness: Brightness` replaces `appearance: WindowAppearance`, which is deleted
with its `From` impl. What the window system draws behind the content stays
`WindowCommand::SetMaterial(WindowMaterial)`. The native recording keeps its `appearance` field
and its four values, so recordings written before read as before: a reader takes `vibrant_light`
and `vibrant_dark` as their theme, and no host writes them.

### 2. Focus and activation are one `WindowFocus`

```rust
pub enum WindowFocus { Inactive, Active, Focused }   // closed: there is no fourth standing

impl WindowFocus {
    pub const fn reported(focused: bool, active: bool) -> Self; // the focus wins
    pub const fn is_focused(self) -> bool;
    pub const fn is_active(self) -> bool;                       // Active | Focused
}
```

`WindowState::focus` replaces `focused` and `active`. Every host builds it with
`WindowFocus::reported`: the window that receives the keys is the one the user works in, and a
window system with no activation apart from focus passes its focus twice. `WindowState::successor`
publishes a hidden window `Inactive`, as it publishes a hidden window unoccluded. The native
recording keeps its two booleans: the writer writes `is_focused` and `is_active`, the reader reads
them through `reported`.

### 3. The window system's refusal is a `PlatformFailure`

`CursorError::Platform(PlatformFailure)` and `PreferenceQueryError::Platform(PlatformFailure)`
replace `Backend(String)` and `Native { message }`; each error has a `failed(message)` constructor,
as `ApplyError` has.

### 4. No method of `PlatformWindow` has a default body

`gesture_geometry`, `window_at` and `reveal_after_first_frame` are required. `gesture_geometry`
stays the one query beside `state()`: it is a system preference read at the window's scale, which
can fail and is retried, not a fact of the window.

### 5. The reveal is a method the embedder calls

`reveal_after_first_frame` stays on `PlatformWindow`. The embedder calls it after the first frame
whose present succeeded, and at a bounded fallback after a frame that ran and presented nothing; a
host that defers the reveal (`WindowReveal::AfterFirstFrame`) performs it then, and one that
reveals at open says so in its body.

### Consequences

- Good, because a window that holds the keyboard and is not active, or a vibrant theme, cannot be
  published, and the out-of-step reads of Win32 and X11 resolve the same way on every host.
- Good, because a host that forgets a facet does not compile.
- Bad, because every window double writes three more methods.
- Neutral, because the native recording and the protocol schema do not change.

### Confirmation

- The compiler: a `compile_fail` doctest on `PlatformWindow::reveal_after_first_frame` (a window
  without it does not compile).
- `flui-platform`'s `focus_implies_activation_and_a_hidden_window_has_neither` (the rule of
  `reported` and the hidden window's standing) and `each_window_fact_advances_the_revision`.
- `flui-testing`'s window conformance row `the_snapshot_agrees_with_the_window_system`, which
  compares the snapshot's `focus` with the window system's two reports read through `reported`.

## Pros and cons of the options

### 1. Two booleans, normalized

- Good, because consumers keep their fields.
- Bad, because the invalid pair stays representable in every value built outside `successor`.

### 2. `WindowFocus`

- Good, because the pair cannot be built.
- Bad, because every reader of `focused` changes to `focus.is_focused()`.

### (a) The host reveals by itself

- Good, because the trait loses a method.
- Bad, because a host sees `pre_present_notify` before a present, not its outcome, so it would
  reveal a window whose surface never presents, or never reveal one whose frames present nothing.

### (b) A required method

- Good, because the embedder that knows the outcome decides, and no host can forget it.
- Bad, because the contract keeps a call the reveal policy alone cannot replace.
