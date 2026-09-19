# Hot-reload plugin boundary

Hot reload is a development-only Rust payload ABI. Host and plugin builds must
agree on layout through the existing ABI-token handshake and keep the plugin
image mapped while plugin-backed values remain alive. This is not a stable C
representation of a scene.

## Mapping decisions

### Typed scene factories and explicit ownership transfer

The plugin macros export fixed C symbol families once per image. Factories return
the canonical `Scene`, publicly reexported from its owning layer crate. Macro
expansions resolve it through `$crate`, so application authors can invoke either
macro through `flui::hot_reload` or a renamed facade dependency. No consumer
layer dependency is necessary.

Every returned allocation has exactly two mutually exclusive consumption paths:

- Keep the original initialized scene in its box, then call that image's
  `flui_scene_drop` or `flui_app_drop` once.
- Move the scene out with `ptr::read` exactly once, then call that image's
  `flui_scene_free` or `flui_app_free` once to deallocate the emptied box.

The host never reconstructs a `Box` to free plugin memory. Free uses
`MaybeUninit<Scene>`, which preserves allocation size/alignment without dropping
the moved value. Both teardown functions are `unsafe extern "C"`: the caller
must supply the live allocation, preserve unique ownership, and choose the
matching consumption path. Null is a no-op. The C ABI and symbol names remain
unchanged; Rust callers now must acknowledge their obligations explicitly.

Keep the image loaded until both its allocation and every retained plugin-backed
payload have been destroyed, including payloads moved out or cloned from a
scene. Destroying the allocation alone does not discharge this obligation.

This follows Rust's ownership rules, rather than a Flutter lifecycle contract:
[`Box::from_raw`](https://doc.rust-lang.org/std/boxed/struct.Box.html#method.from_raw)
requires the original allocation layout and compatible allocator;
[`ptr::read`](https://doc.rust-lang.org/std/ptr/fn.read.html) moves ownership without
changing the allocation; [`MaybeUninit`](https://doc.rust-lang.org/std/mem/union.MaybeUninit.html)
has its payload's layout and omits its destructor. The
[Rust Edition Guide on unsafe attributes](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-attributes.html)
explains why symbol uniqueness is an explicit plugin-image obligation.

External consumer tests reject an incorrectly typed factory and each safe
teardown call separately, then compile both macros through ordinary and renamed
facade dependencies. `tests/scene_ownership.rs` executes both consumption paths
with a counted destructor and checks null handling under Miri. These tests do
not establish arbitrary cross-compiler layout compatibility or make unloading
live plugin-backed values safe.
