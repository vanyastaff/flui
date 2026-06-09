# Foundation Diagnosticable Derive Specification

## Purpose

Pin the requirements for a `#[derive(Diagnosticable)]` procedural derive macro
in `flui-macros`.  The macro auto-generates `Diagnosticable::debug_fill_properties`
implementations from struct field declarations, eliminating ~15 LOC of hand-rolled
boilerplate per render object.

Owner crates: `crates/flui-macros` (`src/lib.rs`).

---

## Requirements

### Requirement: flui-macros MUST provide a #[derive(Diagnosticable)] proc-macro (F15)

`crates/flui-macros/src/lib.rs` MUST export a `#[derive(Diagnosticable)]`
procedural derive macro.

**The macro MUST:**

1. Accept `struct` items with named fields.  Tuple structs and enums are out of
   scope for this change.
2. Auto-generate `impl Diagnosticable for StructName` with a
   `debug_fill_properties(&self, builder: &mut DiagnosticsBuilder)` body that
   iterates the struct's named fields and emits:
   ```rust
   builder.add(stringify!(field_name), &self.field_name);
   ```
   for each field that is NOT marked `#[diagnostic(skip)]`.
3. Support the `#[diagnostic(skip)]` field attribute to exclude internal-only
   fields from diagnostic output.

**The macro MUST NOT:**
- Require fields to implement any trait beyond `std::fmt::Display` (which
  `DiagnosticsBuilder::add` already requires).
- Emit code that references types or paths outside the `flui_foundation` crate
  unless they are derived from the struct definition itself.

**Motivation:** `flui-rendering` ships 10+ hand-rolled `Diagnosticable` impls,
each 10–15 LOC.  Future render objects add ~15 LOC each.  The derive macro
reduces each to a one-line annotation, scales to 50+ future render objects, and
ensures consistency.

**Acceptance criterion:** SC12 — `cargo test -p flui-macros diagnosticable_derive_basic`
exits 0.

#### Scenario: Derive generates debug_fill_properties for a named-field struct (SC12)

- GIVEN:
  ```rust
  #[derive(Diagnosticable)]
  pub struct RenderFlex {
      pub direction: Axis,
      pub main_axis_alignment: MainAxisAlignment,
      pub cross_axis_alignment: CrossAxisAlignment,
  }
  ```
- WHEN the derive macro is applied and the crate is compiled
- THEN the generated `debug_fill_properties` calls `builder.add` exactly three
  times: once for `"direction"`, once for `"main_axis_alignment"`, once for
  `"cross_axis_alignment"`

#### Scenario: #[diagnostic(skip)] excludes a field

- GIVEN:
  ```rust
  #[derive(Diagnosticable)]
  pub struct RenderPadding {
      pub padding: EdgeInsets,
      #[diagnostic(skip)]
      _cache_key: u64,
  }
  ```
- WHEN the derive macro is applied
- THEN the generated `debug_fill_properties` calls `builder.add("padding",
  &self.padding)` only
- AND `_cache_key` does NOT appear in any `builder.add` call

#### Scenario: Derive does not compile on a tuple struct (unsupported — graceful error)

- GIVEN:
  ```rust
  #[derive(Diagnosticable)]
  pub struct Opaque(u32);
  ```
- WHEN the derive macro is applied
- THEN the compiler emits a clear error such as "derive(Diagnosticable) requires
  named fields; tuple structs are not supported"

#### Scenario: Basic smoke test passes (SC12)

- GIVEN `cargo test -p flui-macros diagnosticable_derive_basic` is run
- THEN it exits with code 0
- AND the test demonstrates that the derived impl produces at least one
  `builder.add` call for a struct with at least one named field
