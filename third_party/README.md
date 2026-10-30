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

## accesskit_android 0.9.1

**Source:** crates.io `accesskit_android` 0.9.1, AccessKit commit
`f3026222559c7f5f46d8f25cb7404de68eb149f0` (`adapters/android`), with the licence files of that
commit.

**Why:** TalkBack's cursor reaches FLUI's `Semantics::on_assistive_focus_within` (gap 36 in
`docs/gaps.md`), as VoiceOver's does through `accesskit_ios` above. The published adapter records
where the cursor is (`ACTION_ACCESSIBILITY_FOCUS`) in a private field and tells nobody.

**Change** (`accesskit_android.patch`, the whole difference from the published crate):

- `Adapter::accessibility_focus`, a read accessor: the node and tree that hold accessibility
  focus, or `None` when no node of the tree holds it. FLUI reads it after each action, hover
  event and update it hands the adapter, and tells the application when the answer changed.

**Drop it when** an `accesskit_android` release reports the cursor to the application. The text
for that upstream pull request is below; it has not been sent.

### Upstream pull request (not sent)

> **android: let the application read where TalkBack's cursor is**
>
> An application sometimes needs to know that TalkBack is reading part of its interface: a
> carousel that advances on a timer has to stop while TalkBack is on it (WCAG 2.2.2). The adapter
> already tracks the cursor (`Adapter::accessibility_focus`, the virtual view that
> `ACTION_ACCESSIBILITY_FOCUS` moved to) but keeps it to itself.
>
> This adds `Adapter::accessibility_focus(&self) -> Option<(NodeId, TreeId)>`, the AccessKit node
> under the cursor, or `None` when there is none or the node has left the tree. It is a read
> only: the low-level adapter's caller already holds it around every call that can move the
> cursor (`perform_action`, `on_hover_event`, `update_if_active`), so it can compare before and
> after without a callback, and without running application code inside the adapter.
>
> This is not the tree's focus, which the application owns. The iOS adapter keeps the same state
> (`Context::platform_focus`); a shared trait in `accesskit` is a follow-up if the two shapes
> should converge.
