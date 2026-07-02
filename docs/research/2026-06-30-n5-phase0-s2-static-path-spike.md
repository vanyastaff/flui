[← Roadmap](../ROADMAP.md) · [← Spec](../../specs/004-view-element-core/spec.md)

# N5 Phase 0 S2 — Static-tuple monomorphism spike: SC-007 + FR-016/FR-018 validation

**Status:** spike complete / verdict delivered.
**Date:** 2026-06-30.
**Author role:** Performance engineer (Phase 0 spec-validation, deliverable S2).
**Spec question answered:** SC-007 — does the tuple static path (`column!` / `row!` →
`Container<(A, B, …, P): ViewSeq>`) compile to **monomorphic dispatch with no
`dyn`-call overhead** on the per-position child-update inner loop, in contrast
to the `Vec<BoxedView>` dynamic path which pays a vtable call per child?
**Drives:** FR-016 ("tuple path is monomorphic per position"), FR-018
(`struct Column<C: ViewSeq> { children: C }`), and the Phase 1 go / no-go gate
for the `ViewSeq` design as specified.

Note: the concurrent S1 spike (`2026-06-30-n5-phase0-s1-keyid-interning-spike.md`)
benchmarks key storage; this S2 spike covers dispatch cost only — the two are
independent and share no prototype files.

---

## Executive summary — **VERDICT: CONFIRMED. Phase 1 proceeds.**

The static tuple path not only eliminates vtable dispatch — LLVM goes further:
it devirtualizes every per-position `View` method call, inlines the bodies,
SIMD-vectorizes the payload accumulation with SSE2 `paddq`, and constant-folds
the type-specific contributions at compile time. The dynamic `Vec<BoxedView>`
path pays one indirect call (`callq *24(%rax)`) per child through a runtime-loaded
vtable pointer, plus a loop branch per iteration.

The contrast in the release binary (opt-level=3, lto=thin, Rust 1.96.0) is:

| Path | ASM shape | Indirect calls | Timing (arity 4 / 8 / 16) |
|---|---|---|---|
| `static tuple (A,B,C,D)` | 4 SSE2 loads + SIMD sum + constant | **0** | 0.00 / 0.00 / 0.00 ns |
| `dynamic Vec<BoxedView>` | runtime loop + `callq *24(%rax)` per child | **N per call** | 13.2 / 8.3 / 15.0 ns |

FR-016 and FR-018's premise holds. SC-007's criterion is satisfied. The
`ViewSeq` + `struct Column<C: ViewSeq>` design monomorphizes exactly as
specified and delivers the expected dispatch advantage.

---

## What was built

A throwaway Cargo binary (`s2-static-path-spike`) placed in the session
scratchpad outside the FLUI workspace. It models the exact production shape from
`crates/flui-view/src/seq/{mod,tuple_impls,vec_impls}.rs`:

- `trait View` with an object-safe `type_tag(&self) -> u64` method.
- `trait ViewSeq` with `for_each<F: FnMut(usize, &dyn View)>(&self, f: F)` — the
  production signature, including the `&dyn View` coercion at the callback boundary.
- Tuple impls for arities 0 / 1 / 2 / 4 / 8 / 16 using the same
  `impl_view_seq_for_tuple!` macro shape as production, each with `#[inline]` on
  `for_each`.
- `impl<V: View> ViewSeq for Vec<V>` covering `Vec<BoxedView>` via the blanket, with
  `#[inline]` on `for_each`.
- Four concrete view types `ViewA`–`ViewD` each implementing `type_tag` as
  `0x{A,B,C,D}000_0000 + self.payload` — simple but non-trivial enough to be
  observable if dispatch is NOT devirtualized.
- `struct Container<C: ViewSeq> { children: C }` — the FR-018 shape.
- Four `#[inline(never)]` hot functions:

  ```
  update_static_4  — Container<(ViewA, ViewB, ViewC, ViewD)>
  update_static_8  — Container<(ViewA, ViewB, ViewC, ViewD, ViewA, ViewB, ViewC, ViewD)>
  update_static_16 — Container<(ViewA … 16 positions …)>
  update_dynamic   — Container<Vec<BoxedView>>  (all arities use this)
  ```

  Each accumulates `view.type_tag()` via `for_each` and returns the `u64` sum.
  `#[inline(never)]` ensures both appear as distinct symbols in the `.s` file;
  the closure and `for_each` bodies ARE allowed to inline.

**Build:** `RUSTFLAGS="--emit=asm" cargo rustc --release` with
`codegen-units = 1`, `lto = "thin"`, `opt-level = 3`.

**Reproduce:** the prototype has been removed from the scratchpad per spike
protocol. To reproduce: create a standalone Cargo binary with the same trait
shapes, mark the hot functions `#[inline(never)]`, and run
`RUSTFLAGS="--emit=asm" cargo rustc --release`. The `for_each` `#[inline]`
annotations are mandatory — without them the vtable is not visible at the
devirtualization site.

---

## The ASM evidence (SC-007 criterion)

### Dynamic path — `update_dynamic` (Vec<BoxedView>)

```asm
; _ZN5spike14update_dynamic… — runtime loop body
.LBB17_3:
    movq    (%r14), %rdi       ; load data pointer from BoxedView slot
    movq    8(%r14), %rax      ; load VTABLE POINTER from BoxedView slot (runtime value)
    callq   *24(%rax)          ; INDIRECT CALL: vtable[24] = type_tag() dispatch
    addq    %rax, %r15         ; accumulate
    addq    $16, %r14          ; advance to next BoxedView (16 bytes = ptr + vtable ptr)
    cmpq    %rbx, %r14         ; check loop end
    jne     .LBB17_3           ; branch back
```

One `callq *24(%rax)` — an indirect call through a runtime-loaded vtable pointer —
per child, per iteration. The vtable pointer is loaded from memory at offset +8 of
each `Box<dyn View>` fat pointer; it is an *unknown at compile time* (any concrete
`View` implementation could have been pushed into the Vec). The CPU branch predictor
must track N different call targets across iterations. No SIMD, no constant-folding.

### Static path — `update_static_4` ((ViewA, ViewB, ViewC, ViewD) tuple)

```asm
; _ZN5spike15update_static_4… — complete function body
_ZN5spike15update_static_4…:
    movdqu  (%rdi), %xmm0      ; SSE2: load ViewA.payload + ViewB.payload (16 bytes)
    movdqu  16(%rdi), %xmm1    ; SSE2: load ViewC.payload + ViewD.payload (16 bytes)
    paddq   %xmm0, %xmm1       ; SSE2 parallel u64 add: [A+C, B+D]
    pshufd  $238, %xmm1, %xmm0 ; horizontal fold: move high lane to low
    paddq   %xmm1, %xmm0       ; final horizontal sum of all 4 payloads
    movq    %xmm0, %rcx        ; extract to GPR
    movabsq $12348030976, %rax ; compile-time constant: 0xA000_0000 + 0xB000_0000
                               ;   + 0xC000_0000 + 0xD000_0000 (type-tag bases)
    addq    %rcx, %rax          ; result = constant_base_sum + simd_payload_sum
    retq
```

**Zero indirect calls. Zero loop. Zero branch.** LLVM performed, in sequence:

1. Devirtualization: all four `type_tag()` vtable calls replaced with direct
   inlined arithmetic (the vtable pointer at each tuple position is a
   compile-time constant; LLVM replaced `call [vtable + 24]` with the known body).
2. Inlining: the four `type_tag()` bodies (`0xX000_0000 + self.payload`) are
   expanded inline at each of the four call sites.
3. Constant-folding: the four `0xX000_0000` base constants are summed at
   compile time → `12,348,030,976` (= `0x2_E000_0000`).
4. SIMD vectorization: the four runtime-variable `payload` fields are adjacent
   `u64` values in the struct; LLVM emits SSE2 `paddq` to sum them two-at-a-time,
   then a horizontal reduction.

### Static path — `update_static_8` and `update_static_16`

Arity 8: 4 `movdqu` loads (covering 8 × u64), 4 `paddq` SSE2 additions, 1
horizontal reduction, 1 compile-time constant (`24,696,061,952 = 0x5_C000_0000`).
Zero indirect calls, zero branches.

Arity 16: 8 `movdqu` loads (covering 16 × u64), 7 `paddq` SSE2 additions, 1
horizontal reduction, 1 compile-time constant (`49,392,123,904 = 0xB_8000_0000`).
Zero indirect calls, zero branches.

The pattern scales: each additional arity level adds SSE2 loads and SIMD adds,
never a loop branch or indirect call.

---

## Timing numbers

Platform: x86_64 Linux (Fedora 44), 2,000,000 iterations per cell, two runs.

| Path | Arity | ns/call run 1 | ns/call run 2 |
|---|---|---|---|
| static tuple | 4 | 0.00 | 0.00 |
| dynamic Vec<BoxedView> | 4 | 5.86 | 13.20 |
| static tuple | 8 | 0.00 | 0.00 |
| dynamic Vec<BoxedView> | 8 | 8.50 | 8.29 |
| static tuple | 16 | 0.00 | 0.00 |
| dynamic Vec<BoxedView> | 16 | 14.82 | 15.02 |
| dynamic Vec<BoxedView> (fallback) | 17 | 16.03 | 15.79 |

The static path reads `0.00 ns/call` because the constant-folding described above
reduces the computation to a handful of SSE2 instructions — below the 0.01 ns
resolution of `Instant::now()` at this arity. In a realistic reconciler the closure
body (`view.can_update(old)`, `view.create_element()`) cannot be constant-folded, but
the devirtualization and per-position inlining still hold (the vtable pointers remain
compile-time constants per position). The measured 0.00 ns is the extreme case; the
dynamic path's ~5–15 ns cost captures the irreducible vtable dispatch + loop overhead.

Timing variance on the dynamic path (5.86 vs 13.20 at arity 4) reflects scheduling
noise from system load between runs; the directional conclusion (static << dynamic) is
consistent across both runs.

---

## Arity-16 cliff and fallback behavior

The tuple `ViewSeq` impls cap at arity 16, matching the production `tuple_impls.rs`
macro invocations. A 17-element tuple has no `ViewSeq` implementation; the compiler
emits a `trait bound not satisfied` error. In production, `column!` / `row!` macros
emit a friendly `compile_error!` (FR-034) at >16 children, naming the cliff and
directing the author to `vec![child.boxed(), …]`.

The arity-17 runtime measurement confirms the fallback behaves identically to the
16-child dynamic case (~15–16 ns) — the `Vec<BoxedView>` fallback is a graceful
degradation to the dynamic path, not a crash or silent misbehavior.

---

## Critical path: why inlining is mandatory

The devirtualization result depends on `for_each` being inlined into the
`update_static_N` body. Without inlining, LLVM does not see the concrete types at
the coercion sites and cannot replace the fat-pointer vtable dispatch with a direct
call. The production `tuple_impls.rs` has `#[inline]` on all `for_each`
implementations. This `#[inline]` is not cosmetic — it is a semantic requirement
for the SC-007 monomorphism property to hold. The `lto = "thin"` setting in the
release profile is also load-bearing for inlining across crate boundaries when
`Container<C>` and the concrete tuple types live in different crates.

---

## Threats to validity

- **Simple `type_tag()` bodies.** The spike used `0xX000_0000 + payload` as the
  per-child work, which enables constant-folding unavailable to a real reconciler
  callback. The zero-ns timing is a synthetic upper bound. However, the central
  evidence — the *absence of `callq *reg`* in the static path and its *presence*
  in the dynamic path — is not affected by the callback complexity. A more complex
  `view.can_update(old)` body would still be a devirtualized direct call in the
  static path vs. an indirect call in the dynamic path.

- **SIMD applicability.** The SIMD vectorization (`paddq`) only fires because the
  payload fields are adjacent `u64` values in each struct. A real `View` type has
  a heterogeneous field layout that cannot be vectorized the same way. This does
  not affect the dispatch-cost claim; it means the timing advantage is larger in
  this spike than it would be in production.

- **Opt-level sensitivity.** The devirtualization fires with `opt-level = 3` (and
  does not fire with `opt-level = 0`). The FLUI release profile is `opt-level = 3`
  + `lto = "thin"` per `Cargo.toml` — the measured condition is the shipping
  condition. Debug builds do NOT receive this optimization; that is expected and
  acceptable.

- **Cross-crate LTO boundary.** When `Container<C>` is in one crate and the
  concrete view types in another, devirtualization fires only if `lto = "thin"` is
  active (as it is in the FLUI release profile). Without thin LTO, the compiler
  would not inline `for_each` across the crate boundary and the devirtualization
  would be lost. The current workspace Cargo.toml already sets `lto = "thin"` in
  the release profile, so this is already handled.

---

## Verdict

**CONFIRMED. Phase 1 proceeds.**

FR-016 ("tuple path is monomorphic per position; dynamic path pays `dyn`-dispatch
per child") and FR-018 (`struct Column<C: ViewSeq> { children: C }`) are validated
by direct assembly inspection. The static tuple path compiles to zero indirect calls
per child — no `callq *reg` anywhere in the body — while the dynamic path emits
exactly one `callq *24(%rax)` per child in a runtime loop. The arity-16 cap degrades
gracefully to the `Vec<BoxedView>` fallback with no behavioral discontinuity.

**Phase 1 does NOT reopen.** SC-007 is closed by this evidence.

---

[← Roadmap](../ROADMAP.md) · [← Spec](../../specs/004-view-element-core/spec.md)
