### Changed

- Owner-local event callbacks in the gesture, pointer, focus, text, form, Material and Cupertino catalogs receive `&mut EventCx` and accept signal-write results. Query callbacks retain their decision-only signatures. Form save/reset and field change/reset methods take the caller's event context.
- `FloatingActionButton` is constructed with `new(child)` and enabled with `.on_pressed(callback)`, matching other button builders without optional-closure lifetime annotations.
- Render views obtain their presentation's writer source from `RenderObjectContext`; `callback_with` and `callback_ref` support let-bound event closures with owned and borrowed payloads.
- Event contexts support current-value signal reads without subscriptions. Form save/reset and field edits return typed errors for detached targets, foreign presentation contexts and writes during build, before changing field state.
- Post-frame dispatch reports structured shared, owner-local and total callback batch depths without changing delivery order or admission.

### Fixed

- Queued assistive gesture activation uses the current handler and is cancelled after handler removal or widget disposal.
- Accepted assistive gesture actions preserve their multiplicity and FIFO order across tap and long-press commands, including the queued tail after an earlier callback panics.
- Simultaneously reusing a `FormHandle` or `FormFieldHandle` is diagnosed through a typed drain and isolated before the duplicate can replace or detach the mounted owner's state.
- Layout/build-observed animation and dismissal callbacks are delivered through the owner-local post-frame lane instead of attempting signal writes during build.
- Disposing a ScaffoldMessenger cancels pending completion callbacks; missing post-frame support no longer invokes completion callbacks inside build. Form reset and messenger completion guards recover after callback panics.
- Queued assistive gesture delivery holds only a weak mounted target, so a pending post-frame entry cannot retain the detector's callbacks or presentation writer after teardown.
- A form-field reset dirties its committed state before controller and user callbacks run, keeping the rendered value/error honest when either callback unwinds.
