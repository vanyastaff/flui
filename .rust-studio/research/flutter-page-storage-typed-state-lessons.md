# Flutter PageStorage typed-state lesson

QUESTION: Does Flutter #97583 reveal a PageStorage/restoration class of bug FLUI already has, or a future design constraint FLUI should preserve?

ANSWER: FLUI does not currently ship Flutter-style `PageStorage` / `keepScrollOffset`, so there is no executing FLUI bug to file from this issue. The lesson is architectural: any future FLUI persistence bucket must be namespaced and typed at the storage boundary. Flutter's untyped `Map<Object, dynamic>` plus implicit identifier lets `ExpansionTile`'s `bool`, scroll offset `double`, and table row `int` collide under the same page-storage path.

VERSIONS:
- Flutter reference: local `.flutter` clone verified at tag `3.44.0`.
- FLUI revision: workspace current on 2026-09-13.

SOURCES:
- Flutter issue https://github.com/flutter/flutter/issues/97583 is open, labeled `P2`, `framework`, `f: scrolling`, `c: crash`, and comments identify shared `PageStorageKey` state as the root cause.
- `.flutter/packages/flutter/lib/src/widgets/page_storage.dart:94` stores page state in `Map<Object, dynamic>? _storage`; `writeState` stores arbitrary `dynamic` at lines `104` through `113`, and `readState` returns `dynamic` at lines `124` through `132`.
- `.flutter/packages/flutter/lib/src/widgets/page_storage.dart:25` through `:30` define the implicit path of ancestor `PageStorageKey`s used to compute the storage identifier.
- `.flutter/packages/flutter/lib/src/widgets/page_storage.dart:150` through `:156` documents that scrollables use `PageStorageKey` for scroll position and require unique keys when several scrollables appear in the same bucket.
- `.flutter/packages/flutter/lib/src/material/paginated_data_table.dart:350` through `:352` reads page storage as `int?`; Flutter issue comments cite the analogous `ExpansionTile` `bool?` and `ScrollPosition` `double?` casts.
- `docs/ROADMAP.md:220` explicitly lists `PageStorage` / `keepScrollOffset` as v1-deferred in FLUI scroll parity.
- `crates/flui-widgets/tests/parity/scroll_controller_test.rs:35` repeats that no `PageStorage` equivalent exists in FLUI.
- `crates/flui-foundation/src/key.rs:487` through `:499` implements `ValueKey<T>` equality/hash using the concrete key type and `TypeId::of::<T>()`, so `ValueKey<u64>(42)` and another key family carrying `42` do not alias by value alone.
- `crates/flui-foundation/src/key.rs:1280` through `:1291` tests that `SaltedKey` creates a distinct namespace from its inner key while preserving equality between equal salted keys.

VERIFICATION:
- `cargo nextest run -p flui-foundation -E 'test(test_key_view_key_eq_rejects_cross_type) or test(salt_equals_only_another_salt_of_an_equal_inner_key)' --no-fail-fast` passed 2/2 selected tests, 229 skipped.
- `gh issue list --repo vanyastaff/flui --state all --limit 200 --search 'PageStorage storage key scroll offset expansion tile restoration typed bucket state persistence key collision'` returned no direct duplicate issue.

OPEN:
- No FLUI issue filed. `PageStorage` / `keepScrollOffset` is not implemented, and opening an issue solely to say "when we build it, build it typed" would be speculative.
- Future restoration/page-storage design should make the stored value type part of the key space or storage API, and should avoid a public `Box<dyn Any>` / `dynamic` bucket where unrelated subsystems share one identifier path. A Rust-native shape could use typed slot descriptors such as `(RestorationScopeId, StorageNamespace, ValueTypeId, local key)` or per-subsystem newtyped tokens.

ANSWERED
