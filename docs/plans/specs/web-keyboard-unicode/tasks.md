# Web keyboard Unicode implementation

Status: Approved, implementation in progress.

## Task graph

- [x] T1: Confirm public DOM RED and agree the complete source-specific contract.
- [x] T2: Implement private Web admission and extend the existing browser probe.
  Depends on T1; covers R1-R4 and R7-R8. Preserve a source-reverted producer RED.
- [x] T3: Extend mounted shortcut and common owned-key acceptance tables.
  Depends on T1; covers R5-R6. Assert observable action and event-policy effects.
- [ ] T4: Integrate T2 and T3, document the mapping, record browser and wasm
  evidence, and run the changed-scope gate. Depends on T2 and T3; covers R9.
- [ ] T5: Review standards and spec independently, address findings and publish
  the single feature PR. Depends on T4. Merge requires separate authorization.

## Baseline evidence

On main `61ea713380a449777ffa791b61a3a01e3fd81c5a`, Chrome
154.0.8037.99 executed 38 DOM-constructor Down/Up cases through the registered
producer and public window callback. Ten assertions failed: both states lose
each of two NFC strings and admit each of three raw controls. The other 28
controls and following healthy event passed, with no browser page error.

This is translation evidence, not physical keyboard/layout conformance or
browser IME text-commit acceptance.

## Implementation evidence

The existing browser probe passes all 70 dispatched DOM events in Chrome
154.0.8037.99, including an ordinary pair after refused input. Its complete key
and metadata assertions fail with the former single-scalar predicate restored:
the normative NFC example becomes Unidentified. The independent 38-row
producer matrix also passes with the fix and reproduces its original ten
failures with that predicate restored. Source bytes were restored before the
fixed probe passed again.

The mounted focus/action/shortcut and public owned-key vocabulary tables pass.
They assert intentionally preserved consumer semantics; the Web producer is
the behavior-repair RED.

Independent review exposed four additional public DOM failures: lone high and
low UTF-16 surrogates were replaced with U+FFFD before validation. The final
producer refuses invalid UTF-16 before conversion, while preserving literal
U+FFFD and valid pairs. The extended probe passes all 76 dispatched events and
the independent matrix passes all 44 observations. Removing the UTF-16 guard
and restoring the former scalar predicate reproduces 14 intended failures;
after exact source restoration both checks pass again. Getter reentry and its
following healthy event also pass with no page errors.

These checks also pass on the optimized release artifact. With the same
release profile and wasm-bindgen 0.2.127, the net probe artifact sizes are:

| Artifact | Unchanged main | Implemented probe |
|---|---:|---:|
| wasm | 285,342 bytes | 384,428 bytes |
| wasm, gzip level 9 | 96,653 bytes | 131,649 bytes |
| wasm, Brotli quality 11 | 76,325 bytes | 104,490 bytes |
| JavaScript bindings | 33,601 bytes | 34,293 bytes |

This includes probe changes and coherent Unicode data; it is not isolated
library size or a latency benchmark. The data facilities are wasm-only.
Standards and spec reviews have no remaining findings after the UTF-16 repair.
Final changed-scope verification and PR publication remain pending.
