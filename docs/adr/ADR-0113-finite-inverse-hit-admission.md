# ADR-0113: Hit traversal requires an admitted finite inverse

- **Status:** Accepted
- **Date:** 2026-10-04
- **Related:** ADR-0098 (owned f64 geometry values)

## Context

An absolute determinant cutoff is unrelated to the visual size or conditioning
of a transform. A uniform two-dimensional scale of 1e-9 has a determinant of
1e-18 and a usable inverse of 1e9, yet the former `f64::EPSILON` cutoff refuses
it. A determinant-only probe also admits some non-finite matrices whose
computed inverse contains infinities or NaNs.

The shared hit-result helper formerly substituted the forward matrix when
inversion failed. That fallback changes coordinate direction. In a nested
transform chain it can produce an apparently invertible global-to-local map
with entirely wrong coordinates. Traversal must decide whether it can enter
the subtree before its callback publishes entries.

## Decision

`Matrix4::try_inverse` admits a matrix only when its input entries, computed
nonzero determinant and computed inverse entries are finite. It uses the
existing maintained `glam::DMat4::try_inverse` implementation; it does not apply
an absolute epsilon cutoff. `is_invertible` reports the same admission result,
and failed in-place inversion leaves the original matrix unchanged.

This is a contract for the inverse computed in f64 by that implementation. It
does not promise successful inversion of every mathematically invertible
matrix. A determinant or cofactor that underflows or overflows can still cause
refusal even when an exact inverse would be representable. There is no
additional conditioning estimate, equilibration algorithm or perspective
removal. Callers that need the inverse obtain it once rather than first using
the boolean probe.

`Transform::inverse` retains analytical translation, rotation and scale
variants. Their parameters must be finite; scales must be nonzero and their
reciprocals finite. Those analytical inverses do not use a determinant cutoff.
Complex variants continue through the computed `Matrix4` contract above. The
analytical and general matrix paths have different intermediate range limits;
neither returns a non-finite inverse as success.

`HitTestResult::with_paint_transform` returns `Option<R>`. Failure to obtain an
admitted inverse returns `None` before modifying the transform stack or invoking
the descendant callback. Success pushes the inverse, invokes the callback and
returns `Some(result)`. Its existing guard restores scope on normal return and
unwind. The helper never substitutes a forward matrix for an inverse.

The pipeline maps refusal to a subtree miss at both the node-transform and
context-transform boundaries. No entry from a refused subtree is published,
and a following healthy sibling uses the unmodified parent coordinate space.
Pure paint offsets retain their existing infallible scope API. The context
that records a forward transform also retains its generic return type; inverse
admission belongs at the shared traversal boundary.

## Alternatives

- Keep the epsilon cutoff and repair each small-transform caller. Rejected:
  every caller would carry a different numeric policy, and scale alone is not
  evidence that a finite inverse is unusable.
- Use a singular or forward fallback and reject it during event delivery.
  Rejected: descendants have already emitted hit entries, and a composed
  forward fallback may pass the later admission check with wrong coordinates.
- Invent a general full-range inversion algorithm. Rejected for this contract:
  the maintained fallible glam API covers the required finite small-transform
  case. Broader numerical guarantees would require a separately specified
  conditioning and error contract.

## Validation

Foundation cases compare known transformed points against independent expected
coordinates and cover singular, non-finite, inverse-overflow and computed
determinant range limits. Hit-result cases assert that refused closures do not
run and that a subsequent admitted scope still works. Pipeline cases use
actual tiny nested transforms and observe the leaf's local hit coordinates,
then exercise invalid node/context transforms followed by a healthy sibling.
Existing caught-descendant panic cases retain the unwind restoration contract.

Old-cutoff and forward-fallback source controls must fail the corresponding
coordinate and refusal cases while ordinary admitted transforms remain
healthy. Compilation on other targets is recorded separately from execution.
