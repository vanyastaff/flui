# FLUI macro architecture

The proc-macro crate owns derive expansion. Runtime traits remain in their
owning crates; generated code resolves them from the consuming manifest.

## Mapping decisions

### Resolve runtime paths through Cargo dependencies

All the derives use `proc-macro-crate` to resolve names in the consumer's
manifest, in this order:

1. `flui-sdk`, giving `::flui_sdk::{view,foundation,animation}`: a package
   builds on the SDK alone (ADR-0088 §4).
2. The owning crate (`flui-view`, `flui-foundation`, `flui-animation`), so
   framework crates and advanced consumers keep their explicit owner/version
   selection.
3. The `flui` facade, whose runtime modules serve an application.

Every step honors Cargo renames without per-derive configuration attributes.

The SDK comes first because `proc-macro-crate` reads `[dependencies]` and
`[dev-dependencies]` alike: a package with the facade or an internal crate as a
dev-dependency (for its tests or examples) would otherwise expand to a crate its
library build does not link. The one shape this order cannot serve is an owner
as a normal dependency beside `flui-sdk` as a dev-dependency only; no crate has
it, and such a crate should name the owner through the SDK instead.

When Cargo reports the consuming package itself, expansions use its absolute
crate name. Runtime libraries provide a private `extern crate self` alias;
the same generated path also works in the package's separate integration-test
targets. Using `crate::` would refer to those test crates instead of the library.

The resolver follows [proc-macro-crate 3.5.0](https://docs.rs/proc-macro-crate/3.5.0/proc_macro_crate/)
and [Cargo dependency renaming](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#renaming-dependencies-in-cargotoml).
Owner unit tests cover expansion inside libraries; consumer integration tests
(`tests/facade_consumer.rs` at the root) cover facade-only and SDK-only
manifests, the SDK beside a facade dev-dependency, renamed dependencies, and
generic view derives.
