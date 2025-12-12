# File Organization

**Complete file structure for FLUI rendering system (~150 files)**

---

## Overview

FLUI rendering system is organized into 8 major modules with clear separation of concerns. Total project contains approximately 150 implementation files plus module files.

---

## Project Root Structure

```
flui-rendering/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs                     # Main library entry point
│   ├── prelude.rs                 # Common imports
│   │
│   ├── protocol.rs                # Protocol trait (1 file)
│   ├── constraints/               # Constraints types (2 files)
│   ├── geometry/                  # Geometry types (2 files)
│   ├── parent_data/               # Parent data types (15 files)
│   ├── containers/                # Generic containers (5 files)
│   ├── traits/                    # Trait definitions (15 files)
│   ├── objects/                   # Render objects (85+ files)
│   ├── delegates/                 # Delegate traits (6 files)
│   ├── pipeline/                  # Pipeline system (8 files)
│   └── layer/                     # Layer system (15 files)
│
├── examples/                      # Usage examples
├── tests/                         # Integration tests
└── benches/                       # Performance benchmarks
```

---

## Module 1: Protocol (1 file)

```
src/
└── protocol.rs
    ├── trait Protocol
    ├── struct BoxProtocol
    └── struct SliverProtocol
```

**Purpose:** Core protocol abstraction with associated types

---

## Module 2: Constraints (2 files)

```
src/constraints/
├── mod.rs
├── box_constraints.rs
│   ├── struct BoxConstraints
│   ├── impl BoxConstraints
│   │   ├── fn tight()
│   │   ├── fn loose()
│   │   ├── fn expand()
│   │   └── fn constrain()
│   └── tests
│
└── sliver_constraints.rs
    ├── struct SliverConstraints
    ├── enum AxisDirection
    ├── enum GrowthDirection
    ├── enum ScrollDirection
    └── tests
```

**Total:** 2 implementation files + 1 mod.rs = 3 files

---

## Module 3: Geometry (2 files)

```
src/geometry/
├── mod.rs
├── size.rs
│   ├── struct Size
│   ├── impl Size
│   │   ├── const ZERO
│   │   ├── const INFINITE
│   │   └── fn contains()
│   └── tests
│
└── sliver_geometry.rs
    ├── struct SliverGeometry
    ├── impl SliverGeometry
    │   ├── fn zero()
    │   └── fn is_visible()
    └── tests
```

**Total:** 2 implementation files + 1 mod.rs = 3 files

---

## Module 4: Parent Data (15 files)

```
src/parent_data/
├── mod.rs
├── parent_data.rs                      # Base trait
│
├── box_parent_data.rs                  # 1. BoxParentData
├── flex_parent_data.rs                 # 2. FlexParentData
├── stack_parent_data.rs                # 3. StackParentData
├── wrap_parent_data.rs                 # 4. WrapParentData
├── flow_parent_data.rs                 # 5. FlowParentData
├── list_body_parent_data.rs            # 6. ListBodyParentData
├── table_cell_parent_data.rs           # 7. TableCellParentData
├── multi_child_layout_parent_data.rs   # 8. MultiChildLayoutParentData
├── list_wheel_parent_data.rs           # 9. ListWheelParentData
│
├── sliver_parent_data.rs               # 10. SliverParentData
├── sliver_logical_parent_data.rs       # 11. SliverLogicalParentData
├── sliver_physical_parent_data.rs      # 12. SliverPhysicalParentData
├── sliver_multi_box_adaptor_parent_data.rs  # 13. SliverMultiBoxAdaptorParentData
├── sliver_grid_parent_data.rs          # 14. SliverGridParentData
└── tree_sliver_node_parent_data.rs     # 15. TreeSliverNodeParentData
```

**Total:** 15 implementation files + 1 mod.rs + 1 trait = 17 files

---

## Module 5: Containers (5 files)

```
src/containers/
├── mod.rs
├── single.rs
│   ├── struct Single<P>
│   ├── type BoxChild
│   └── type SliverChild
│
├── children.rs
│   ├── struct Children<P, PD>
│   ├── type BoxChildren<PD>
│   └── type SliverChildren<PD>
│
├── proxy.rs
│   ├── struct Proxy<P>
│   ├── type ProxyBox
│   └── type SliverProxy
│
├── shifted.rs
│   ├── struct Shifted<P>
│   ├── type ShiftedBox
│   └── type ShiftedSliver
│
└── aligning.rs
    ├── struct Aligning<P>
    ├── type AligningBox
    └── type AligningSliver
```

**Total:** 5 implementation files + 1 mod.rs = 6 files

---

## Module 6: Traits (15 files)

```
src/traits/
├── mod.rs
├── render_object.rs                # Base trait
│
├── box/
│   ├── mod.rs
│   ├── render_box.rs               # RenderBox trait
│   ├── single_child.rs             # SingleChildRenderBox
│   ├── proxy_box.rs                # RenderProxyBox
│   ├── hit_test_proxy.rs           # HitTestProxy
│   ├── clip_proxy.rs               # ClipProxy<T>
│   ├── physical_model_proxy.rs     # PhysicalModelProxy
│   ├── shifted_box.rs              # RenderShiftedBox
│   ├── aligning_shifted_box.rs     # RenderAligningShiftedBox
│   └── multi_child.rs              # MultiChildRenderBox
│
└── sliver/
    ├── mod.rs
    ├── render_sliver.rs            # RenderSliver trait
    ├── proxy_sliver.rs             # RenderProxySliver
    ├── single_box_adapter.rs       # RenderSliverSingleBoxAdapter
    ├── multi_box_adaptor.rs        # RenderSliverMultiBoxAdaptor
    └── persistent_header.rs        # RenderSliverPersistentHeader
```

**Total:** 15 implementation files + 3 mod.rs = 18 files

---

## Module 7: Objects (85+ files)

### Box Objects (60 files)

```
src/objects/box/
├── mod.rs
│
├── basic/                          # 6 objects
│   ├── mod.rs
│   ├── padding.rs                  # RenderPadding
│   ├── align.rs                    # RenderAlign
│   ├── constrained_box.rs          # RenderConstrainedBox
│   ├── sized_box.rs                # RenderSizedBox
│   ├── aspect_ratio.rs             # RenderAspectRatio
│   └── baseline.rs                 # RenderBaseline
│
├── layout/                         # 15 objects
│   ├── mod.rs
│   ├── flex.rs                     # RenderFlex
│   ├── stack.rs                    # RenderStack
│   ├── indexed_stack.rs            # RenderIndexedStack
│   ├── wrap.rs                     # RenderWrap
│   ├── flow.rs                     # RenderFlow
│   ├── list_body.rs                # RenderListBody
│   ├── table.rs                    # RenderTable
│   ├── custom_layout.rs            # RenderCustomLayout
│   ├── intrinsic_width.rs          # RenderIntrinsicWidth
│   ├── intrinsic_height.rs         # RenderIntrinsicHeight
│   ├── limited_box.rs              # RenderLimitedBox
│   ├── fractionally_sized_overflow_box.rs  # RenderFractionallySizedOverflowBox
│   ├── constrained_overflow_box.rs         # RenderConstrainedOverflowBox
│   ├── sized_overflow_box.rs       # RenderSizedOverflowBox
│   └── constraints_transform_box.rs # RenderConstraintsTransformBox
│
├── effects/                        # 11 objects
│   ├── mod.rs
│   ├── opacity.rs                  # RenderOpacity
│   ├── transform.rs                # RenderTransform
│   ├── fitted_box.rs               # RenderFittedBox
│   ├── fractional_translation.rs   # RenderFractionalTranslation
│   ├── rotated_box.rs              # RenderRotatedBox
│   ├── clip_rect.rs                # RenderClipRect
│   ├── clip_rrect.rs               # RenderClipRRect
│   ├── clip_oval.rs                # RenderClipOval
│   ├── clip_path.rs                # RenderClipPath
│   ├── decorated_box.rs            # RenderDecoratedBox
│   └── backdrop_filter.rs          # RenderBackdropFilter
│
├── animation/                      # 4 objects
│   ├── mod.rs
│   ├── animated_opacity.rs         # RenderAnimatedOpacity
│   ├── animated_size.rs            # RenderAnimatedSize
│   ├── physical_model.rs           # RenderPhysicalModel
│   └── physical_shape.rs           # RenderPhysicalShape
│
├── interaction/                    # 6 objects
│   ├── mod.rs
│   ├── pointer_listener.rs         # RenderPointerListener
│   ├── mouse_region.rs             # RenderMouseRegion
│   ├── absorb_pointer.rs           # RenderAbsorbPointer
│   ├── ignore_pointer.rs           # RenderIgnorePointer
│   ├── offstage.rs                 # RenderOffstage
│   └── ignore_baseline.rs          # RenderIgnoreBaseline
│
├── gestures/                       # 4 objects
│   ├── mod.rs
│   ├── tap_region.rs               # RenderTapRegion
│   ├── tap_region_surface.rs       # RenderTapRegionSurface
│   ├── semantics_gesture_handler.rs # RenderSemanticsGestureHandler
│   └── custom_paint.rs             # RenderCustomPaint
│
├── media/                          # 3 objects
│   ├── mod.rs
│   ├── image.rs                    # RenderImage
│   ├── texture.rs                  # RenderTexture
│   └── video.rs                    # RenderVideo
│
├── text/                           # 2 objects
│   ├── mod.rs
│   ├── paragraph.rs                # RenderParagraph
│   └── editable.rs                 # RenderEditable
│
├── accessibility/                  # 5 objects
│   ├── mod.rs
│   ├── semantics_annotations.rs    # RenderSemanticsAnnotations
│   ├── block_semantics.rs          # RenderBlockSemantics
│   ├── exclude_semantics.rs        # RenderExcludeSemantics
│   ├── indexed_semantics.rs        # RenderIndexedSemantics
│   └── merge_semantics.rs          # RenderMergeSemantics
│
├── platform/                       # 2 objects
│   ├── mod.rs
│   ├── platform_view.rs            # RenderPlatformView
│   └── annotated_region.rs         # RenderAnnotatedRegion
│
├── scroll/                         # 4 objects
│   ├── mod.rs
│   ├── view.rs                     # RenderView
│   ├── viewport.rs                 # RenderViewport
│   ├── shrink_wrapping_viewport.rs # RenderShrinkWrappingViewport
│   └── list_wheel_viewport.rs      # RenderListWheelViewport
│
└── debug/                          # 2 objects
    ├── mod.rs
    ├── error_box.rs                # RenderErrorBox
    └── performance_overlay.rs      # RenderPerformanceOverlay
```

**Box Subtotal:** 60 implementation files + 13 mod.rs = 73 files

### Sliver Objects (25 files)

```
src/objects/sliver/
├── mod.rs
│
├── basic/                          # 5 objects
│   ├── mod.rs
│   ├── padding.rs                  # RenderSliverPadding
│   ├── to_box_adapter.rs           # RenderSliverToBoxAdapter
│   ├── fill_remaining.rs           # RenderSliverFillRemaining
│   ├── fill_remaining_and_overscroll.rs # RenderSliverFillRemainingAndOverscroll
│   └── constrained_cross_axis.rs   # RenderSliverConstrainedCrossAxis
│
├── layout/                         # 11 objects
│   ├── mod.rs
│   ├── list.rs                     # RenderSliverList
│   ├── fixed_extent_list.rs        # RenderSliverFixedExtentList
│   ├── grid.rs                     # RenderSliverGrid
│   ├── fill_viewport.rs            # RenderSliverFillViewport
│   ├── varied_extent_list.rs       # RenderSliverVariedExtentList
│   ├── persistent_header.rs        # RenderSliverPersistentHeader
│   ├── scrolling_persistent_header.rs    # RenderSliverScrollingPersistentHeader
│   ├── pinned_persistent_header.rs       # RenderSliverPinnedPersistentHeader
│   ├── floating_persistent_header.rs     # RenderSliverFloatingPersistentHeader
│   ├── floating_pinned_persistent_header.rs # RenderSliverFloatingPinnedPersistentHeader
│   └── tree_sliver.rs              # RenderTreeSliver
│
├── effects/                        # 3 objects
│   ├── mod.rs
│   ├── opacity.rs                  # RenderSliverOpacity
│   ├── animated_opacity.rs         # RenderSliverAnimatedOpacity
│   └── decorated_sliver.rs         # RenderDecoratedSliver
│
├── interaction/                    # 1 object
│   ├── mod.rs
│   └── ignore_pointer.rs           # RenderSliverIgnorePointer
│
└── scroll/                         # 5 objects
    ├── mod.rs
    ├── offstage.rs                 # RenderSliverOffstage
    ├── main_axis_group.rs          # RenderSliverMainAxisGroup
    ├── cross_axis_group.rs         # RenderSliverCrossAxisGroup
    ├── cross_axis_expanded.rs      # RenderSliverCrossAxisExpanded
    └── semantics_annotations.rs    # RenderSliverSemanticsAnnotations
```

**Sliver Subtotal:** 25 implementation files + 6 mod.rs = 31 files

**Objects Total:** 85 implementation files + 20 mod.rs = 105 files

---

## Module 8: Delegates (6 files)

```
src/delegates/
├── mod.rs
├── custom_painter.rs               # CustomPainter trait
├── custom_clipper.rs               # CustomClipper<T> trait
├── single_child_layout_delegate.rs # SingleChildLayoutDelegate trait
├── multi_child_layout_delegate.rs  # MultiChildLayoutDelegate trait
├── flow_delegate.rs                # FlowDelegate trait
└── sliver_grid_delegate.rs         # SliverGridDelegate trait
```

**Total:** 6 implementation files + 1 mod.rs = 7 files

---

## Module 9: Pipeline (8 files)

```
src/pipeline/
├── mod.rs
├── pipeline_owner.rs               # PipelineOwner struct
├── painting_context.rs             # PaintingContext
├── render_view.rs                  # RenderView (root node)
├── semantics_owner.rs              # SemanticsOwner
├── dirty_tracking.rs               # Dirty node management
├── frame_production.rs             # Frame production logic
└── pipeline_manifold.rs            # PipelineManifold trait
```

**Total:** 8 implementation files + 1 mod.rs = 9 files

---

## Module 10: Layer (15 files)

```
src/layer/
├── mod.rs
├── layer.rs                        # Base Layer trait
│
├── leaf/
│   ├── mod.rs
│   ├── picture_layer.rs            # PictureLayer
│   ├── texture_layer.rs            # TextureLayer
│   ├── platform_view_layer.rs      # PlatformViewLayer
│   └── performance_overlay_layer.rs # PerformanceOverlayLayer
│
└── container/
    ├── mod.rs
    ├── container_layer.rs          # ContainerLayer (base)
    ├── clip_rect_layer.rs          # ClipRectLayer
    ├── clip_rrect_layer.rs         # ClipRRectLayer
    ├── clip_path_layer.rs          # ClipPathLayer
    ├── color_filter_layer.rs       # ColorFilterLayer
    ├── backdrop_filter_layer.rs    # BackdropFilterLayer
    ├── shader_mask_layer.rs        # ShaderMaskLayer
    ├── opacity_layer.rs            # OpacityLayer
    ├── transform_layer.rs          # TransformLayer
    ├── offset_layer.rs             # OffsetLayer
    ├── leader_layer.rs             # LeaderLayer
    └── follower_layer.rs           # FollowerLayer
```

**Total:** 15 implementation files + 3 mod.rs = 18 files

---

## Supporting Files

### Library Root

```
src/
├── lib.rs                          # Main entry point, re-exports
├── prelude.rs                      # Common imports
└── error.rs                        # Error types
```

**Total:** 3 files

### Utilities

```
src/utils/
├── mod.rs
├── alignment.rs                    # Alignment enum
├── axis.rs                         # Axis enum
├── edge_insets.rs                  # EdgeInsets
├── offset.rs                       # Offset
├── rect.rs                         # Rect
├── matrix4.rs                      # Matrix4
└── color.rs                        # Color
```

**Total:** 8 files

---

## Complete File Count

| Module | Implementation Files | Module Files | Total |
|--------|---------------------|--------------|-------|
| **Protocol** | 1 | 0 | 1 |
| **Constraints** | 2 | 1 | 3 |
| **Geometry** | 2 | 1 | 3 |
| **Parent Data** | 16 | 1 | 17 |
| **Containers** | 5 | 1 | 6 |
| **Traits** | 15 | 3 | 18 |
| **Objects/Box** | 60 | 13 | 73 |
| **Objects/Sliver** | 25 | 6 | 31 |
| **Delegates** | 6 | 1 | 7 |
| **Pipeline** | 8 | 1 | 9 |
| **Layer** | 15 | 3 | 18 |
| **Library Root** | 3 | 0 | 3 |
| **Utilities** | 8 | 0 | 8 |
| **TOTAL** | **166** | **31** | **197** |

---

## File Naming Conventions

### Snake Case

All files use snake_case:
- ✅ `render_opacity.rs`
- ✅ `custom_painter.rs`
- ✅ `box_constraints.rs`
- ❌ `RenderOpacity.rs`
- ❌ `CustomPainter.rs`

### Module Structure

Every directory has `mod.rs`:
```rust
// objects/box/effects/mod.rs
mod opacity;
mod transform;
mod clip_rect;

pub use opacity::RenderOpacity;
pub use transform::RenderTransform;
pub use clip_rect::RenderClipRect;
```

### One Type Per File

Each file contains one primary type:
- `opacity.rs` → `RenderOpacity`
- `flex_parent_data.rs` → `FlexParentData`
- `box_constraints.rs` → `BoxConstraints`

---

## Import Paths

### From External Crate

```rust
use flui_rendering::prelude::*;
use flui_rendering::{RenderOpacity, RenderFlex};
use flui_rendering::traits::{RenderBox, RenderProxyBox};
use flui_rendering::containers::ProxyBox;
```

### Internal (within crate)

```rust
// Use crate:: for absolute imports
use crate::protocol::Protocol;
use crate::traits::RenderBox;
use crate::containers::ProxyBox;

// Use super:: for relative imports
use super::RenderProxyBox;
```

---

## Prelude Pattern

```rust
// src/prelude.rs
pub use crate::protocol::{Protocol, BoxProtocol, SliverProtocol};
pub use crate::traits::{
    RenderObject, RenderBox, RenderSliver,
    SingleChildRenderBox, RenderProxyBox, RenderShiftedBox,
};
pub use crate::containers::{
    Single, Children, Proxy, Shifted, Aligning,
    BoxChild, SliverChild, ProxyBox, SliverProxy,
};
pub use crate::constraints::{BoxConstraints, SliverConstraints};
pub use crate::geometry::{Size, SliverGeometry};

// Usage in implementation files:
use crate::prelude::*;
```

---

## Test Organization

```
tests/
├── protocol_tests.rs               # Protocol system tests
├── container_tests.rs              # Container tests
├── box_objects_tests.rs            # Box object tests
├── sliver_objects_tests.rs         # Sliver object tests
├── layout_tests.rs                 # Layout algorithm tests
├── paint_tests.rs                  # Paint tests
└── pipeline_tests.rs               # Pipeline tests
```

---

## Example Organization

```
examples/
├── simple_box.rs                   # Basic box layout
├── flex_layout.rs                  # Flex layout demo
├── custom_painter.rs               # Custom painting
├── sliver_list.rs                  # Scrollable list
├── complex_layout.rs               # Complex nested layout
└── animation.rs                    # Animated effects
```

---

## Benchmark Organization

```
benches/
├── layout_bench.rs                 # Layout performance
├── paint_bench.rs                  # Paint performance
├── tree_bench.rs                   # Tree operations
└── protocol_bench.rs               # Protocol overhead
```

---

## Documentation

```
docs/
├── architecture.md                 # Architecture overview
├── protocol_system.md              # Protocol documentation
├── trait_system.md                 # Trait documentation
└── implementation_guide.md         # Implementation guide
```

---

## Cargo.toml

```toml
[package]
name = "flui-rendering"
version = "0.1.0"
edition = "2021"
authors = ["FLUI Team"]
license = "MIT OR Apache-2.0"
description = "Flutter-inspired rendering engine for Rust"
repository = "https://github.com/flui-rs/flui-rendering"

[dependencies]
# Core
ambassador = "0.3"          # Trait delegation
thiserror = "1.0"           # Error handling

# Graphics
skia-safe = "0.70"          # 2D graphics
wgpu = "0.18"               # GPU backend

# Utilities
tracing = "0.1"             # Logging
parking_lot = "0.12"        # Efficient locks
smallvec = "1.11"           # Stack-allocated vectors
ahash = "0.8"               # Fast hashing

[dev-dependencies]
criterion = "0.5"           # Benchmarking
proptest = "1.4"            # Property testing
pretty_assertions = "1.4"   # Better test output

[features]
default = ["skia"]
skia = ["skia-safe"]
wgpu = ["dep:wgpu"]
```

---

## Next Steps

- [[Protocol]] - Understanding the foundation
- [[Implementation Guide]] - Creating new files
- [[Object Catalog]] - Browse existing objects

---

**See Also:**
- [[Trait Hierarchy]] - Trait organization
- [[Containers]] - Container organization
