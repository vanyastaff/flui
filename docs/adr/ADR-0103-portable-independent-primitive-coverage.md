# ADR-0103: Portable independent primitive coverage

- **Status:** Proposed; implementation and local gates pass, pending PR acceptance.
- **Date:** 2026-10-02
- **Supersedes:** [ADR-0102](ADR-0102-immutable-geometric-clip-composition.md),
  its direct-primitive capability decision only; the geometric expression,
  sample lattice, group ownership and admission decisions remain in force.
- **Related:** [ADR-0100](ADR-0100-prepared-gpu-work-and-retained-frame-commit.md)

## Context

An optional GPU blend feature must not decide the meaning of a paint operation.
Folding fractional geometry coverage into paint alpha cannot implement transparent
Clear or source replacement: the destination still needs the uncovered fraction.
Gradients have intrinsic edge coverage even without an antialiased clip.
Saturating Plus is nonlinear: clamping after multiplying the source by coverage
does not equal applying coverage to the clamped full operation.

## Decision

The accepted pixel result is `D * (1 - C) + blend(S, D) * C`, where `S` and `D`
are premultiplied encoded SDR values and `C` is geometric coverage independent of
paint alpha. Plus clamps the full operation to the normalized attachment range
before this mix. This extends the existing group contract to direct primitives.

Destination-sensitive direct draws without dual-source blending isolate one
logical primitive into two unblended attachments: premultiplied `P = C * S`, and
R8 coverage `C`. A cropped destination copy supplies `D`. The composite evaluates
the operator without dividing by coverage. For example, Clear is `(1-C)*D`,
Src is `P+(1-C)*D`, and Plus is `min(vec4(C), P+C*D)+(1-C)*D`.
Optional dual-source blending retains equivalent linear fast paths; fractional
Plus uses the portable composite on either device. SSAA Plus resolves an
independent geometry plane through the same rasterization and downsample mapping
as its paint plane. Overlapping primitives remain separate ordered operations;
a shared blend mode does not authorize combining them into one coverage mask.

Scratch extents are conservative integer geometry bounds intersected with the
attachment and recorded scissor. A frozen crop mapping changes clip-space
coordinates but preserves world coordinates and addresses the clip mask in the
original attachment space. Small primitives do not allocate whole-window scratch
textures. Temporary allocations are admitted before creation and their charges
retire with submission completion. Lazy isolation layouts preserve ordinary
drawing on devices requesting fewer resources than the portable path requires.
Crop origins align with the attachment's 2×2 derivative grid; the original
scissor bounds both isolation and composite writes despite this alignment.
An SSAA tile with no attachment intersection is skipped before admission.

`WgpuPainter::render_to_texture` accepts the backing texture and creates its view
itself. It validates format, extent, dimension, layers, sample count and usages.
This avoids an unverifiable public texture/view pair. The view-only entry point
remains usable for operations whose accepted result does not require reading the
destination; otherwise it returns a typed capability error. Framework window,
headless, filter and SSAA destinations carry their actual backing textures.

The baseline path requires two color attachments, three bind groups, sufficient
uniform bindings and sampled textures, and attachment-byte limits computed from
wgpu's format costs. RGBA8 plus R8 costs nine attachment bytes per sample in wgpu,
although their physical texture storage totals five bytes per pixel. Insufficient
requested limits refuse the segment before isolation, without substituting an
incorrect blend. This is capability admission, not a backend-name exception.

## Evidence and remaining work

The existing `coverage_blend_reads_back_as_specified` family covers fractional
Porter-Duff results with opaque and translucent destinations, transparent Clear,
ordered overlap, varying gradient alpha, intrinsic gradient edges, attachment
crop rebasing and saturated Plus. The painter family covers view-only
refusal, reduced MRT limits, invalid texture targets and the next valid frame.
Native readbacks pass, including coverage on the software DX12 WARP adapter.
Production shader mutations that substitute paint alpha for coverage, remove
the Plus clamp or omit the crop origin each fail the coverage family; the
mutations are restored. Browser and mobile execution remain separate evidence,
not inferred from native wgpu success or cross-typechecks.

This decision does not change the primitive edge estimator, the encoded SDR color
contract, or supply HDR, 3D or an AI runtime. Those are independent planned
contracts. Scratch pooling, geometry/clip sample correlation and GPU timing need
measured workload evidence; they must preserve the independent coverage and
ordering contract rather than trading correctness for an optional feature.

## Sources

- [W3C Compositing and Blending Level 1, Porter-Duff operators](https://www.w3.org/TR/compositing-1/#porterduffcompositingoperators)
  supplies the full-operation factors. The independent geometric mix and
  normalized Plus clamp above are FLUI's explicit rendering contract.
- [Chrome 130 WebGPU: dual-source blending](https://developer.chrome.com/blog/new-in-webgpu-130)
  describes it as an optional feature that can replace additional render passes.
- [wgpu 30.0.1 TextureFormat](https://docs.rs/wgpu/30.0.1/wgpu/enum.TextureFormat.html)
  supplies attachment cost and format vocabulary; admission uses the pinned API.

Sources checked on 2026-10-02 with Keenable against primary documentation.
