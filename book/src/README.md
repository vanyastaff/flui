# FLUI

FLUI is a declarative UI framework for Rust, inspired by Flutter's widget-style composition. Its
pipeline is a tree design — immutable `View` configuration → mutable `Element` lifecycle →
layout/paint `RenderObject` → a `Layer` tree built each frame, with a `Semantics` tree alongside
for accessibility → `flui-engine` compositor → `wgpu` GPU — with structure and APIs designed for
Rust. Cross-crate decisions are recorded as ADRs. See
[`AGENTS.md`](https://github.com/vanyastaff/flui/blob/main/AGENTS.md)'s Design stance for the
full policy.

This book teaches the current development version of FLUI. Start with a checkout and the
[installation guide](getting-started/installation.md); the framework's release and platform
acceptance evidence lives in the repository's
[beta criteria](https://github.com/vanyastaff/flui/blob/main/docs/BETA.md).
API and feature choices can change before the beta release.

The book is incomplete. Chapters link to the working examples and reference material where
more detail is needed. A code sample is either copied from an example or CLI template, or
fenced as `rust,ignore`. Building this site does not compile those snippets: check the linked
source for the exact API and required features.

## Where to start

- New to FLUI and want to build an application: [Getting Started](getting-started/installation.md).
- Ready to change a working example: [Counter → Todo](getting-started/tutorial-todo.md), then
  the [Cookbook](cookbook/overview.md) for forms, async work, themes, animation, and testing.
- Following the combined application work: [Notes showcase source draft](getting-started/showcase.md).
  The chapter records the source and verification still needed before its walkthrough can run.
- Choosing a state model or learning how a view becomes a frame:
  [Concepts](concepts/overview.md) and [State](concepts/state.md).
- Coming from Flutter and want the vocabulary mapping first:
  [Flutter → FLUI mapping](mapping.md).
- Want to contribute to FLUI itself: [Contributing to FLUI](getting-started/contributing.md).
- Looking for the deep architectural reasoning, not just the how-to:
  [Architecture](architecture.md).

## Find the right reference

| Need | Source |
|------|--------|
| Runnable applications and feature flags | [Example commands](https://github.com/vanyastaff/flui/blob/main/docs/getting-started.md#run-an-example) |
| Public types and method contracts for your checkout | Run `cargo doc -p flui --no-deps --open`; enable the same features as your application. |
| Widget discovery | [Widget catalog](widgets/catalog.md) |
| Testing an application without a native window | [Testing recipe](cookbook/testing.md) |
| Design decisions and crate boundaries | [Architecture](architecture.md) |
| Current release priorities and verification requirements | [Roadmap](https://github.com/vanyastaff/flui/blob/main/docs/ROADMAP.md) and [beta criteria](https://github.com/vanyastaff/flui/blob/main/docs/BETA.md) |

The [repository](https://github.com/vanyastaff/flui) contains this book's source. Use the edit
button on a chapter to propose a documentation correction.
