# Example, template and acceptance parity

- **Status:** In implementation
- **Date:** 2026-11-08
- **Source:** `packages-09`
- **Owner direction:** resolve the audit findings as one completed replacement, without keeping parallel examples that teach different trees

## Problem

The counter template, root example and agent acceptance test described the same application but built different trees. The hand-written hot-reload counter duplicated the generated workspace and depended on internal crates. Rendering, native and hot-reload fixtures lived at the repository root as tier-exempt tools instead of beside their owning crates, and the official Material and Cupertino packages had no examples.

## Requirements

- **R1. One counter source.** The CLI embeds one real Rust source with `include_str!`; the root example and agent acceptance compile that same file. Acceptance uses the template's `RawButton`, not a Material button.
- **R2. Real acceptance.** The agent workflow mounts the production `CounterApp` with required environment scopes, inspects structure and semantics, drives pointer input and observes the rendered count. A semantic change in the shared source changes the test result.
- **R3. One hot-reload source.** Remove the hand-written hot-reload counter. `flui create --hot-reload` and `cli_create` are the canonical project and compile coverage for all three generated crates. Reach checks use the real facade hot-reload root rather than the deleted project.
- **R4. Owning-crate examples.** Move engine/scene, painting, native web and hot-reload lifecycle fixtures beside their owning crates, preserving their build paths and documents. Manifest and dependency classification must enforce existing tier rules rather than bypass them after the move.
- **R5. Package examples.** Add executable Material and Cupertino examples that depend on `flui-sdk` alone in production, mount real themed controls through the SDK testing kit and demonstrate current typed theme APIs.
- **R6. Replacement coverage.** Remove stale paths, names and instructions from code, workspace metadata, tests and live documents. Record the change in `changelog.d` and validate affected targets plus repository gates.

## Non-goals

- Changing Android entry-point ownership or facade API shape (`packages-01`).
- Running browser or GPU demos as a substitute for their compile checks.
- Redesigning hot reload or adding a second generated project fixture.
- Adding host, facade or internal-crate production dependencies to official packages.
