# Web keyboard Unicode implementation

Status: Approved, implementation in progress.

## Task graph

- [x] T1: Confirm public DOM RED and agree the complete source-specific contract.
- [ ] T2: Implement private Web admission and extend the existing browser probe.
  Depends on T1; covers R1-R4 and R7-R8. Preserve a source-reverted producer RED.
- [ ] T3: Extend mounted shortcut and common owned-key acceptance tables.
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
