# FLUI

FLUI is a Flutter-inspired declarative UI framework for Rust. It takes its shape from Flutter's
tree pipeline — immutable `View` configuration → mutable `Element` lifecycle → layout/paint
`RenderObject` → a `Layer` tree built each frame, with a `Semantics` tree alongside for accessibility →
`flui-engine` compositor → `wgpu` GPU — but it is not a port: where Flutter's contracts are good, FLUI starts from them and says so; where they are not,
or where Rust's ownership model asks for something different, FLUI diverges and records the
reasoning as an ADR. See [`AGENTS.md`](https://github.com/vanyastaff/flui/blob/main/AGENTS.md)'s
Design stance for the full policy.

This book is a work in progress, published at <https://vanyastaff.github.io/flui/>: the structure
is here; sections still being written are stubs that point at the working reference material
(`docs/`, `examples/`, rustdoc). A code sample is either copied from a compiling example or CLI
template, or fenced as `rust,ignore`. Nothing compiles an ignored fence, so read it as an
illustration and check the linked source for the exact API.

## Where to start

- New to FLUI and want to build an application: [Getting Started](getting-started/installation.md).
- Coming from Flutter and want the vocabulary mapping first:
  [Flutter → FLUI mapping](mapping.md).
- Want to contribute to FLUI itself: [Contributing to FLUI](getting-started/contributing.md).
- Looking for the deep architectural reasoning, not just the how-to:
  [Architecture](architecture.md).
