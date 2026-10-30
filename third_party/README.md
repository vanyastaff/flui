# Vendored crates

A crate here is a published crate with a change FLUI needs before upstream has it. Its dependent
names it by `path` beside the published `version`, never through a root `[patch.crates-io]`: a
patch binds only this workspace, so an app that depends on FLUI by path or git would build the
published crate without the change. A vendored crate is excluded from the workspace (its own
lints, not FLUI's) and keeps its sources as published except for the change its `.patch` file
holds. To drop one: delete its directory and its `.patch` file, remove the `path` key from the
dependency, bump the `version` to the upstream release that has the change, and run
`cargo update -p <crate>`.

## accesskit_ios 0.2.2

**Source:** crates.io `accesskit_ios` 0.2.2, AccessKit commit
`f3026222559c7f5f46d8f25cb7404de68eb149f0` (`adapters/ios`), with the licence files of that commit.

**Why:** VoiceOver's cursor reaches FLUI's `Semantics::on_assistive_focus_within` (gap 36 in
`docs/gaps.md`). The published adapter records where the cursor is in a private field and tells
nobody.

**Change** (`accesskit_ios.patch`, the whole difference from the published crate):

- `PlatformFocusHandler`, a public trait whose `platform_focus_changed(Option<(NodeId, TreeId)>)`
  is told when the cursor moves to a node of the tree or leaves it;
- `Adapter::with_platform_focus_handler`, `Adapter::new` with that handler as a fifth argument;
- `accessibilityElementDidBecomeFocused` and `accessibilityElementDidLoseFocus` tell the handler
  once the tree is no longer borrowed, and an update that removes the node under the cursor tells
  it `None` when its events are raised;
- a move made while the handler runs is told after it returns, in order.

**Drop it when** an `accesskit_ios` release reports the cursor to the application. The text for
that upstream pull request is below; it has not been sent.

### Upstream pull request (not sent)

> **ios: tell the application where VoiceOver's cursor is**
>
> An application sometimes needs to know that VoiceOver is reading part of its interface: a
> carousel that advances on a timer has to stop while VoiceOver is on it (WCAG 2.2.2). The
> adapter already tracks the cursor (`Context::platform_focus`, set by
> `accessibilityElementDidBecomeFocused` and `accessibilityElementDidLoseFocus`) but keeps it to
> itself.
>
> This adds `PlatformFocusHandler` and `Adapter::with_platform_focus_handler`. The handler is
> told `Some((node, tree))` when the cursor moves onto a node and `None` when it leaves the tree,
> either because VoiceOver moved off it or because an update removed the node. It runs on the main
> thread, outside every borrow of the adapter, so it may update the tree; a move made while it
> runs is told after it returns. `Adapter::new` is unchanged, and an adapter made with it behaves
> as before.
>
> This is not the tree's focus, which the application owns. Android's adapter keeps the same
> state (`ACTION_ACCESSIBILITY_FOCUS`); the trait could live in `accesskit` for both, which is a
> follow-up if this shape is accepted.
