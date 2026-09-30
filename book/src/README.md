# FLUI

FLUI is a declarative UI framework for Rust, inspired by Flutter's widget-style composition. Its
pipeline is a three-tree design — immutable `View` configuration → mutable `Element` lifecycle →
layout/paint `RenderObject` → retained `Layer` tree → `flui-engine` compositor → `wgpu` GPU — with
structure and APIs designed for Rust. Cross-crate decisions are recorded as ADRs. See
[`AGENTS.md`](https://github.com/vanyastaff/flui/blob/main/AGENTS.md)'s Design stance for the
full policy.

This book is a skeleton (tracked as H2 in the beta roadmap): the structure is here, and every
section is either filled with real, verified content or an explicit stub pointing at the working
reference material that exists today (`docs/`, `examples/`, rustdoc). Nothing in this book invents
an API that isn't in the source tree — a code sample is either copied from a compiling example or
CLI template, or fenced as `rust,ignore` and labeled as illustrative.

## Where to start

- New to FLUI and want to build an application: [Getting Started](getting-started/installation.md).
- Coming from Flutter and want the vocabulary mapping first:
  [Flutter → FLUI mapping](mapping.md).
- Want to contribute to FLUI itself: [Contributing to FLUI](getting-started/contributing.md).
- Looking for the deep architectural reasoning, not just the how-to:
  [Architecture](architecture.md).
