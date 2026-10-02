# ADR-0102: Immutable geometric clips and effect coverage ownership

- **Status:** Proposed; implementation and local gates pass, pending PR acceptance.
- **Date:** 2026-10-01
- **Supersedes:** [ADR-0099](ADR-0099-save-layer-region-and-blend.md),
  [ADR-0057](ADR-0057-coverage-correct-blending-is-capability-gated.md)
- **Related:** [ADR-0057](ADR-0057-coverage-correct-blending-is-capability-gated.md),
  [ADR-0098](ADR-0098-owned-f64-geometry-values.md),
  [ADR-0100](ADR-0100-prepared-gpu-work-and-retained-frame-commit.md)

## Replaced capability decision

ADR-0057 previously allowed folded, incorrect destructive AA on a device without
DUAL_SOURCE_BLENDING. That fallback is superseded: independent clip coverage is
mandatory for accepted rendering. Direct unsupported Tess/Gradient draws under
an AA expression return `UnsupportedCoverageBlend` during ordered admission,
before that segment emits GPU work. The error is nonretryable; callers may change
the scene or choose an isolated group whose sampleable destination supports the
portable full-result mix. Group compositing uses the portable path; a general
portable direct-primitive coverage plane remains required follow-up work.
`featureless_direct_aa_refusal_recovers` exercises the public painter refusal,
unchanged target and next valid frame. It passes in the required native GPU run; the merge gate remains separate.

## Context

Canvas commands, layer effects and GPU replay must agree on which samples a clip
admits and where its coverage applies. A single rounded-clip slot loses an
ancestor when a child installs another shape. Bounds alone cannot express paths,
rotated rectangles or Difference. Multiplying resolved parent and child masks
also changes coverage when an identical clip is repeated.

A group rendered through its inherited clip and then composited through the same
clip attenuates its edge twice. Applying an inherited scissor before a blur also
removes source pixels the effect could have brought into the visible region.

## Decision

1. **Owned expression.** Every recorded run owns an immutable geometric clip
   expression. Each leaf captures its finite f64 2D affine mapping at installation.
   Changing the current transform does not move an earlier leaf. Save/restore
   restores the expression head. Identity includes strong ownership; allocation
   addresses without that ownership are not durable clip identifiers.
2. **Geometry.** Rectangles, elliptical rounded rectangles, exponent-four rounded
   superellipses and filled paths support Intersect and Difference. Rounded radii
   are fitted with one common factor, preserving each rx/ry ratio. Path contours
   close for filling and preserve winding or even-odd rules. Singular transforms
   have empty geometric membership: an intersection empties the expression and
   a difference leaves it unchanged. Non-finite, projective and non-representable
   2D inputs return typed errors; perspective and 3D require their own explicit
   scene/projection contract rather than implicit narrowing into this 2D pass.
3. **Coverage.** Boolean membership is evaluated before coverage resolve, on one
   common 8 by 8 sample lattice per attachment pixel. Hard leaves evaluate the
   pixel centre. Repeating the same geometric AA clip is idempotent. R8 is the
   final coverage result, never an intermediate operand in geometric clipping.
   Hardware scissors are conservative culling bounds. Scaled effect attachments
   reevaluate geometry on their own lattice through an explicit attachment-to-root
   mapping; sampling a previously resolved mask is not equivalent.
4. **Effect boundary.** A group owns its inherited clip prefix at its composite.
   Its isolated input starts without that prefix, including its coarse scissor,
   and newly installed child clips form the input's suffix. Restore reinstates
   the parent prefix. This applies to opacity and filter groups, including empty
   layers whose blend changes the destination under a transparent source.
5. **Blend.** The group result is `mix(destination, blend(source, destination),
   coverage)`. Destination-sensitive modes under fractional coverage use a
   backdrop read on the baseline GPU path; multiplying only source alpha is
   insufficient for Clear, Src and DstIn. Entirely hard coverage discards
   excluded fragments and uses exact fixed-function blending, including on
   view-only targets. Direct primitive draws still use ADR-0057's two-source
   path. If the
   device cannot carry the independent coverage channel, a direct destructive
   draw under an AA clip is rejected with `UnsupportedCoverageBlend`, rather than
   silently changing its edge. A portable direct-draw fallback needs independent
   geometric coverage, including for transparent paint, and remains required work.
6. **Region and damage.** A save layer's region is its local bounds mapped through
   the admitted transform and cut by its inherited clip; without bounds it is
   the clip. Its recorded blend operates over the entire region, including pixels
   its content left transparent. Rotated bounds constrain the geometric expression
   as well as its conservative region. Damage producers report at least this region
   for modes that change pixels under transparent source. Shader-mask blending
   remains between the shader and its child, with the finished result composited
   separately onto the parent.
7. **Admission.** CPU expression ownership is charged before allocation. Lowering,
   GPU masks and binding metadata are admitted before allocation. Membership work
   is cumulative across the frame and is not refunded by restore or dropping a
   mask. Exceeding a limit refuses the frame; it does not truncate a path, drop a
   leaf or substitute its bounds. Submission completion owns prepared charges.

## Implementation limits and follow-ups

These are current implementation limits, not permanent wire-format promises:
64 clip leaves, 512 path commands per leaf and 512 flattened edges per mask;
GPU payload magnitude at most 2^20, attachment dimensions at most 16384, and
1,000,000,000 sample-membership work units per frame. Bounded Lyon flattening
uses a transform-aware tolerance. Separate uniform bindings fit the portable
16 KiB uniform baseline without fragment storage buffers.

Mask cropping and per-segment strong-key deduplication bound avoidable work.
A frame-wide mask cache, an atlas and further fast paths need measured workload
and lifetime evidence. DrawItem container metadata is still outside the recording
arena allowance; this decision does not claim a complete CPU memory quota.

Public GPU readbacks in the existing clip family cover nested shapes, repeated
AA, elliptical radii, subpixel rectangles, path membership, Difference, transform
capture, group prefix ownership, destructive compositing and next-frame recovery
following invalid geometry or depth refusal. Existing transformed path/SSAA
readbacks must remain green. A green native run does not prove browser, mobile
or a platform event-translation path has executed.


## Capability and work-policy clarification

The [wgpu 30.0.1 feature documentation](https://docs.rs/wgpu/30.0.1/wgpu/struct.Features.html)
lists dual-source blending on both WebGPU and native backends. Backend name is
not sufficient admission evidence: use the requested device feature set. The
portable direct coverage path remains required for devices without that feature.

An entirely HardEdge expression evaluates its common pixel center once, instead
of redundantly evaluating the same membership 64 times. Mixed expressions retain
64 correlated samples. Admission charges the selected mode; a full-HD frame with
eight hard leaves would otherwise exceed the frame work ceiling despite having
no fractional coverage. The public `hard_clip_membership_scales_to_a_full_hd_frame`
row pins that ordinary frame's admission and corner/interior pixels.

Requested device limits are checked before creating the lazy clip layout: three
fragment uniform buffers, an 8192-byte uniform binding and an 8192-byte buffer
are required. Insufficient capabilities produce `PreparedResourceLimit`.
The public `limited_fragment_uniforms_clip_refusal_recovers` row requests two
fragment uniforms, renders normally, verifies a typed clip refusal without
changing the target, then renders the next ordinary frame on the same painter.
