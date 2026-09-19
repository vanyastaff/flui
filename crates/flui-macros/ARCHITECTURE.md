# FLUI macro architecture

The proc-macro crate owns derive expansion. Runtime traits remain in their
owning crates; generated code resolves them from the consuming manifest.

## Mapping decisions

### Resolve runtime paths through Cargo dependencies

All four derives use `proc-macro-crate` to resolve names in the consumer's
manifest. Direct runtime dependencies take precedence over the facade so
framework crates and advanced consumers keep their explicit owner/version
selection. Otherwise, a single `flui` dependency supplies the runtime modules.
Both paths honor Cargo renames without per-derive configuration attributes.

When Cargo reports the consuming package itself, expansions use its absolute
crate name. Runtime libraries provide a private `extern crate self` alias;
the same generated path also works in the package's separate integration-test
targets. Using `crate::` would refer to those test crates instead of the library.

The resolver follows [proc-macro-crate 3.5.0](https://docs.rs/proc-macro-crate/3.5.0/proc_macro_crate/)
and [Cargo dependency renaming](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#renaming-dependencies-in-cargotoml).
Owner unit tests cover expansion inside libraries; consumer integration tests
cover facade-only manifests, renamed dependencies, and generic view derives.
