### Changed

- Reject system text scales outside `1/64..=64` before publishing preferences,
  preventing extreme observations from breaking paragraph layout.

- Deliver host contrast preferences to inherited media data, Cupertino dynamic
  colors and optional Material contrast themes. Preserve explicit brightness,
  authored colors and nested media overrides during live changes.

- Runtime bootstrap seeds accepted host preferences before the first widget build
  and keeps that host through publication. Replacing the native owner retires
  the old preference source and isolates the next runtime incarnation.

- Windows observes system preferences independently of user windows, with owned
  native subscriptions and an internal setting-change receiver. Host recreation
  uses direct WinRT activation rather than a process-wide activation factory.
  Desktop owner wakes propagate observations to runtimes; failed Windows reads
  retry without user windows and successful reads stop idle retries. Remaining
  preference consumers and mobile live delivery are still under implementation.

- Windows owner delivery uses a kernel-event fallback when posting its wake
  message fails, preserving worker requests, reentrant continuation and quit
  through the native event loop without polling.

- `UiRuntime::set_device_pixel_ratio_for` publishes the new ratio to inherited
  window data as well as the renderer. The mutable `MediaQuerySource` and
  `UiRuntime::media_query_for` are internal; hosts no longer synchronize them
  separately.

- `EditableText` applies inherited text scaling when mounted and updated, keeping
  glyph geometry and caret position in the same text layout without changing the
  document or selection offsets.

- `Text` and `RichText` apply inherited text scaling on mount and after preference
  updates. `RichText` now composes a render view from a stateless build instead of
  implementing `RenderView` directly, so inherited dependencies are tracked.

- Import `MediaQuery` and `MediaQueryData` from `flui_widgets` directly; their
  former `flui_widgets::app` exports are removed. Inherited presentation data
  now lives below text and application composition in the widget module graph.

- Rename the independent UI execution owner from `UiRealm` to `UiRuntime`, its
  generational identity from `RealmId` to `UiRuntimeId`, and the identity field
  in `PresentationAddress` to `ui_runtime_id`. The runtime's identity accessor
  is `id()`. Runtime module paths use
  `ui_runtime`; associated error variants and test-support names use runtime
  vocabulary. No compatibility aliases retain the former names.

### Fixed

- Connect rebuild notifications before mounting each presentation, so inherited
  changes request a frame without relying on desktop-only runner setup.

- Preserve accepted appearance and safe-area changes when a batched native resize
  fails, retaining the first failure across redraw and owner completion.
- Notify development agents only after native window initialization remains live;
  closing a window during activation cannot hand over a failed installation.
- Deliver pending pointer motion before keyboard and IME callbacks without ending
  pointer contacts, preserving the first callback failure and the following input.
- Route surface resize through the installed frame driver's lifetime, preserving
  driver restoration after a resize failure and retirement after reentrant teardown.
- Keep newly arriving native input behind accepted window changes and earlier
  input when owner work carries across callbacks. Deferred keyboard delivery
  suppresses the native default when the framework cannot answer synchronously.
- Allow window identity and presentation assembly callbacks to reenter the host
  without a registry borrow panic. Refuse shared installation if its authorizing
  presentation closes during assembly.
- Tie frame resources and asynchronous renderer publication to the presentation
  that installed them. Native close and quit revoke publication before terminal
  lifecycle observers run.
- Preserve close-handler registrations made during another handler's retirement,
  and retire native owners outside app registry borrows while retaining the first
  failure across independent cleanup.

- Deliver font invalidation, host lifecycle and background work by UI runtime
  identity so closing its original window cannot discard work for surviving windows.
