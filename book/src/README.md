# FLUI

FLUI is a Flutter-inspired declarative UI framework for Rust. It takes its shape from Flutter's
three-tree pipeline — immutable `View` configuration → mutable `Element` lifecycle → layout/paint
`RenderObject` → retained `Layer` tree → `flui-engine` compositor → `wgpu` GPU — but it is not a
port: where Flutter's contracts are good, FLUI starts from them and says so; where they are not,
or where Rust's ownership model asks for something different, FLUI diverges and records the
reasoning as an ADR. See [`AGENTS.md`](https://github.com/vanyastaff/flui/blob/main/AGENTS.md)'s
Design stance for the full policy.

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
