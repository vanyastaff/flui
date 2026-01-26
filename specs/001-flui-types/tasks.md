# Tasks: flui-types Crate

**Input**: Design documents from `/specs/001-flui-types/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/README.md, quickstart.md

**Tests**: Test-first development is REQUIRED per Constitution Principle IV. All public API tests must be written BEFORE implementation and verified to FAIL before proceeding to GREEN state.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

Single Rust crate at `crates/flui_types/`:
- Source: `crates/flui_types/src/`
- Tests: `crates/flui_types/tests/`
- Benchmarks: `crates/flui_types/benches/`
- Examples: `crates/flui_types/examples/`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and basic structure

- [ ] T001 Create crate directory structure per plan.md (src/, tests/, benches/, examples/)
- [ ] T002 Initialize Cargo.toml with dependencies: proptest 1.5, criterion 0.5, trybuild 1.0, thiserror 1.0 (dev-deps only)
- [ ] T003 [P] Configure Clippy lints and rustfmt in Cargo.toml and .cargo/config.toml
- [ ] T004 [P] Add compile-time size assertions for memory layout contracts in crates/flui_types/src/lib.rs
- [ ] T005 Create module structure: src/units/, src/geometry/, src/styling/, src/prelude.rs

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T006 Define EPSILON constant (1e-6) in crates/flui_types/src/units/constants.rs
- [ ] T007 Define Unit trait with ZERO, to_f32(), from_f32(), approx_eq() in crates/flui_types/src/units/mod.rs
- [ ] T008 Write unit tests for Unit trait default implementations in crates/flui_types/tests/unit_tests/units_test.rs
- [ ] T009 [P] Setup property test infrastructure with arbitrary generators in crates/flui_types/tests/property_tests/mod.rs
- [ ] T010 [P] Setup criterion benchmark harness with black_box in crates/flui_types/benches/mod.rs

**Checkpoint**: Foundation ready - user story implementation can now begin in parallel

---

## Phase 3: User Story 1 - Device-Independent Layout (Priority: P1) 🎯 MVP

**Goal**: Enable widget developers to specify sizes in device-independent Pixels that work consistently across all screen DPI settings

**Independent Test**: Create a widget with `Pixels(100.0)` width, verify it maintains visual size across 1x, 2x, 3x displays

### Tests for User Story 1 (Test-First Required)

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T011 [P] [US1] Write failing unit test for Pixels::new() in crates/flui_types/tests/unit_tests/units_test.rs
- [ ] T012 [P] [US1] Write failing unit test for Pixels arithmetic (Add, Sub, Mul, Div) in crates/flui_types/tests/unit_tests/units_test.rs
- [ ] T013 [P] [US1] Write failing unit test for Point::new() and Point::distance_to() in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T014 [P] [US1] Write failing unit test for Size::new() and Size::area() in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T015 [P] [US1] Write failing unit test for Rect::from_ltwh() and Rect::contains() in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T016 [US1] Verify all tests FAIL with expected errors (run `cargo test` - should see 0 passed, N failed)

### Implementation for User Story 1

- [ ] T017 [US1] Implement Pixels newtype in crates/flui_types/src/units/pixels.rs (Copy, Clone, PartialEq, Debug, Default)
- [ ] T018 [US1] Implement Unit trait for Pixels in crates/flui_types/src/units/pixels.rs
- [ ] T019 [US1] Implement arithmetic operators (Add, Sub, Mul, Div, Neg) for Pixels in crates/flui_types/src/units/pixels.rs
- [ ] T020 [US1] Verify Pixels tests now PASS (run `cargo test units_test`)
- [ ] T021 [P] [US1] Implement Point\<T: Unit\> struct in crates/flui_types/src/geometry/point.rs
- [ ] T022 [P] [US1] Implement Size\<T: Unit\> struct in crates/flui_types/src/geometry/size.rs
- [ ] T023 [US1] Implement Point::new(), Point::distance_to() with #[inline] in crates/flui_types/src/geometry/point.rs
- [ ] T024 [US1] Implement Size::new(), Size::area(), Size::is_empty() in crates/flui_types/src/geometry/size.rs
- [ ] T025 [US1] Implement Rect\<T: Unit\> struct in crates/flui_types/src/geometry/rect.rs
- [ ] T026 [US1] Implement Rect::from_ltwh(), Rect::contains() in crates/flui_types/src/geometry/rect.rs
- [ ] T027 [US1] Verify all US1 tests now PASS (run `cargo test`)
- [ ] T028 [US1] Add module re-exports to crates/flui_types/src/lib.rs and crates/flui_types/src/prelude.rs

**Checkpoint**: At this point, User Story 1 should be fully functional - developers can use Pixels, Point, Size, Rect for device-independent layout

---

## Phase 4: User Story 2 - Unit Mixing Prevention (Priority: P1)

**Goal**: Make it impossible to accidentally mix incompatible unit types (e.g., Pixels + DevicePixels) at compile time

**Independent Test**: Attempt `Pixels(10.0) + DevicePixels(20.0)` - should fail compilation with clear error message

### Tests for User Story 2 (Test-First Required)

> **NOTE: Write these tests FIRST using trybuild to ensure compilation failures**

- [ ] T029 [P] [US2] Create compile-fail test for mixed Pixels + DevicePixels in crates/flui_types/tests/integration_tests/compile_fail/mixed_units.rs
- [ ] T030 [P] [US2] Create compile-fail test for mixed Point\<Pixels\> + Offset\<DevicePixels\> in crates/flui_types/tests/integration_tests/compile_fail/mixed_point_offset.rs
- [ ] T031 [P] [US2] Create compile-fail test for mixed Rect\<Pixels\>.intersect(Rect\<DevicePixels\>) in crates/flui_types/tests/integration_tests/compile_fail/mixed_rect_ops.rs
- [ ] T032 [US2] Setup trybuild test runner in crates/flui_types/tests/integration_tests/unit_mixing_test.rs
- [ ] T033 [US2] Verify compile-fail tests correctly detect expected compilation errors (run `cargo test unit_mixing_test`)

### Implementation for User Story 2

- [ ] T034 [US2] Implement strict trait bounds on Point operators (Point + Offset same unit) in crates/flui_types/src/geometry/point.rs
- [ ] T035 [US2] Implement strict trait bounds on Rect operators (Rect ops same unit) in crates/flui_types/src/geometry/rect.rs
- [ ] T036 [US2] Add compile-time size assertion: assert!(size_of::\<Point\<Pixels\>\>() <= 8) in crates/flui_types/src/lib.rs
- [ ] T037 [US2] Verify compile-fail tests still work with implementation (run `cargo test unit_mixing_test`)
- [ ] T038 [US2] Document error messages in crates/flui_types/README.md with examples

**Checkpoint**: At this point, type system prevents all unit mixing at compile time - impossible to mix Pixels and DevicePixels

---

## Phase 5: User Story 3 - Geometric Calculations (Priority: P1)

**Goal**: Provide complete Point, Size, Rect operations for hit testing, clipping, layout calculations

**Independent Test**: Calculate distance between points, rectangle intersection, bounding boxes - verify mathematical correctness

### Tests for User Story 3 (Test-First Required)

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T039 [P] [US3] Write failing property test for Point distance symmetry in crates/flui_types/tests/property_tests/geometry_properties.rs
- [ ] T040 [P] [US3] Write failing property test for Point triangle inequality in crates/flui_types/tests/property_tests/geometry_properties.rs
- [ ] T041 [P] [US3] Write failing property test for Rect intersection commutativity in crates/flui_types/tests/property_tests/geometry_properties.rs
- [ ] T042 [P] [US3] Write failing property test for Rect union contains both in crates/flui_types/tests/property_tests/geometry_properties.rs
- [ ] T043 [P] [US3] Write failing unit test for Offset magnitude and normalize in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T044 [US3] Verify all property tests FAIL initially (run `cargo test property_tests`)

### Implementation for User Story 3

- [ ] T045 [P] [US3] Implement Offset\<T: Unit\> struct in crates/flui_types/src/geometry/offset.rs
- [ ] T046 [US3] Implement Offset::magnitude(), Offset::normalized() in crates/flui_types/src/geometry/offset.rs
- [ ] T047 [US3] Implement Point::offset_by(), Point::approx_eq() in crates/flui_types/src/geometry/point.rs
- [ ] T048 [US3] Implement Point - Point = Offset operator in crates/flui_types/src/geometry/point.rs
- [ ] T049 [US3] Implement Rect::intersects(), Rect::intersect(), Rect::union() in crates/flui_types/src/geometry/rect.rs
- [ ] T050 [US3] Implement Rect::inflate(), Rect::deflate() in crates/flui_types/src/geometry/rect.rs
- [ ] T051 [US3] Implement Rect edge accessors: left(), top(), right(), bottom(), center() in crates/flui_types/src/geometry/rect.rs
- [ ] T052 [US3] Implement Size::approx_eq(), Size::scale() in crates/flui_types/src/geometry/size.rs
- [ ] T053 [US3] Verify all property tests now PASS (run `cargo test property_tests`)
- [ ] T054 [US3] Add #[inline] attributes to hot path methods (distance_to, contains, intersect) in geometry files

**Checkpoint**: At this point, full geometric calculation API available - ready for hit testing, layout, clipping

---

## Phase 6: User Story 4 - Font-Relative Sizing (Priority: P2)

**Goal**: Enable accessible layouts via Rems type that scales with user font preferences

**Independent Test**: Create padding with `Rems(1.5)`, verify it scales proportionally when base font size changes

### Tests for User Story 4 (Test-First Required)

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T055 [P] [US4] Write failing unit test for Rems::new() and arithmetic in crates/flui_types/tests/unit_tests/units_test.rs
- [ ] T056 [P] [US4] Write failing unit test for Rems::to_pixels(base_font_size) in crates/flui_types/tests/unit_tests/units_test.rs
- [ ] T057 [US4] Verify Rems tests FAIL (run `cargo test units_test`)

### Implementation for User Story 4

- [ ] T058 [US4] Implement Rems newtype in crates/flui_types/src/units/pixels.rs (Copy, Clone, PartialEq, Debug, Default)
- [ ] T059 [US4] Implement Unit trait for Rems in crates/flui_types/src/units/pixels.rs
- [ ] T060 [US4] Implement arithmetic operators for Rems in crates/flui_types/src/units/pixels.rs
- [ ] T061 [US4] Implement Rems::to_pixels(base_font_size) in crates/flui_types/src/units/pixels.rs
- [ ] T062 [US4] Verify Rems tests now PASS (run `cargo test units_test`)

**Checkpoint**: At this point, Rems type available for accessible font-relative spacing

---

## Phase 7: User Story 5 - Unit Conversions (Priority: P2)

**Goal**: Provide explicit conversion methods between unit types (Pixels ↔ DevicePixels, Pixels ↔ Rems)

**Independent Test**: Convert Pixels → DevicePixels → back to Pixels, verify round-trip preserves value within epsilon

### Tests for User Story 5 (Test-First Required)

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T063 [P] [US5] Write failing unit test for Pixels::to_device_pixels(scale) in crates/flui_types/tests/unit_tests/units_test.rs
- [ ] T064 [P] [US5] Write failing unit test for DevicePixels::to_logical_pixels(scale) in crates/flui_types/tests/unit_tests/units_test.rs
- [ ] T065 [P] [US5] Write failing unit test for Pixels::to_rems(base_font_size) in crates/flui_types/tests/unit_tests/units_test.rs
- [ ] T066 [P] [US5] Write failing property test for round-trip conversions in crates/flui_types/tests/property_tests/conversions_properties.rs
- [ ] T067 [US5] Verify conversion tests FAIL (run `cargo test conversions`)

### Implementation for User Story 5

- [ ] T068 [US5] Implement DevicePixels newtype in crates/flui_types/src/units/pixels.rs
- [ ] T069 [US5] Implement Unit trait for DevicePixels in crates/flui_types/src/units/pixels.rs
- [ ] T070 [US5] Implement Pixels::to_device_pixels(scale_factor) in crates/flui_types/src/units/conversions.rs
- [ ] T071 [US5] Implement DevicePixels::to_logical_pixels(scale_factor) in crates/flui_types/src/units/conversions.rs
- [ ] T072 [US5] Implement Pixels::to_rems(base_font_size) in crates/flui_types/src/units/conversions.rs
- [ ] T073 [US5] Implement Point::to_device_pixels(scale), Size::to_device_pixels(scale), Rect::to_device_pixels(scale) in geometry files
- [ ] T074 [US5] Verify conversion tests now PASS (run `cargo test conversions`)

**Checkpoint**: At this point, full unit conversion API available for layout-to-render pipeline

---

## Phase 8: User Story 6 - Padding and Margins (Priority: P2)

**Goal**: Provide EdgeInsets type for expressing padding, margins, safe areas

**Independent Test**: Create EdgeInsets, apply to Rect via inset_by(), verify resulting content area

### Tests for User Story 6 (Test-First Required)

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T075 [P] [US6] Write failing unit test for EdgeInsets::all(), EdgeInsets::symmetric() in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T076 [P] [US6] Write failing unit test for EdgeInsets::horizontal(), EdgeInsets::vertical() in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T077 [P] [US6] Write failing unit test for Rect::inset_by(EdgeInsets) in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T078 [US6] Verify EdgeInsets tests FAIL (run `cargo test geometry_test`)

### Implementation for User Story 6

- [ ] T079 [US6] Implement EdgeInsets\<T: Unit\> struct in crates/flui_types/src/geometry/edges.rs
- [ ] T080 [US6] Implement EdgeInsets::new(), EdgeInsets::all(), EdgeInsets::symmetric(), EdgeInsets::only() in crates/flui_types/src/geometry/edges.rs
- [ ] T081 [US6] Implement EdgeInsets::horizontal(), EdgeInsets::vertical() in crates/flui_types/src/geometry/edges.rs
- [ ] T082 [US6] Implement Rect::inset_by(EdgeInsets) in crates/flui_types/src/geometry/rect.rs
- [ ] T083 [US6] Add compile-time size assertion: assert!(size_of::\<EdgeInsets\<Pixels\>\>() <= 16) in crates/flui_types/src/lib.rs
- [ ] T084 [US6] Verify EdgeInsets tests now PASS (run `cargo test geometry_test`)

**Checkpoint**: At this point, EdgeInsets type available for padding/margins in layouts

---

## Phase 9: User Story 7 - Colors (Priority: P2)

**Goal**: Provide Color system with RGB/HSL support and multiple blending modes (mix, blend_over, scale)

**Independent Test**: Create colors from hex, blend two colors, verify perceptually correct results

### Tests for User Story 7 (Test-First Required)

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T085 [P] [US7] Write failing unit test for Color::from_rgb() and Color::from_rgba() in crates/flui_types/tests/unit_tests/color_test.rs
- [ ] T086 [P] [US7] Write failing unit test for Color::from_hex() valid formats in crates/flui_types/tests/unit_tests/color_test.rs
- [ ] T087 [P] [US7] Write failing unit test for Color::from_hex() invalid formats (debug panic, release fallback) in crates/flui_types/tests/unit_tests/color_test.rs
- [ ] T088 [P] [US7] Write failing unit test for Color::mix() boundaries (ratio 0.0 and 1.0) in crates/flui_types/tests/unit_tests/color_test.rs
- [ ] T089 [P] [US7] Write failing unit test for Color::blend_over() alpha compositing in crates/flui_types/tests/unit_tests/color_test.rs
- [ ] T090 [P] [US7] Write failing unit test for Color::scale() RGB multiplication in crates/flui_types/tests/unit_tests/color_test.rs
- [ ] T091 [P] [US7] Write failing unit test for Color::lighten() and Color::darken() HSL-based in crates/flui_types/tests/unit_tests/color_test.rs
- [ ] T092 [P] [US7] Write failing property test for Color mix commutativity in crates/flui_types/tests/property_tests/color_properties.rs
- [ ] T093 [US7] Verify all color tests FAIL (run `cargo test color_test`)

### Implementation for User Story 7

- [ ] T094 [US7] Implement Color struct (r, g, b, a as f32) in crates/flui_types/src/styling/color.rs
- [ ] T095 [US7] Implement Color::from_rgb(), Color::from_rgba() in crates/flui_types/src/styling/color.rs
- [ ] T096 [US7] Implement Color::from_hex() with thiserror for parsing errors in crates/flui_types/src/styling/color.rs
- [ ] T097 [US7] Add tracing::warn! for invalid hex in release mode in crates/flui_types/src/styling/color.rs
- [ ] T098 [US7] Implement HSL struct and RGB↔HSL conversions in crates/flui_types/src/styling/color.rs
- [ ] T099 [US7] Implement Color::mix(other, ratio) with linear interpolation in crates/flui_types/src/styling/color_blend.rs
- [ ] T100 [US7] Implement Color::blend_over(background) with Porter-Duff compositing in crates/flui_types/src/styling/color_blend.rs
- [ ] T101 [US7] Implement Color::scale(factor) with RGB multiplication in crates/flui_types/src/styling/color_blend.rs
- [ ] T102 [US7] Implement Color::lighten(amount) and Color::darken(amount) via HSL in crates/flui_types/src/styling/color.rs
- [ ] T103 [US7] Implement Color::with_opacity(opacity) in crates/flui_types/src/styling/color.rs
- [ ] T104 [US7] Add named color constants (RED, BLUE, WHITE, BLACK, TRANSPARENT) in crates/flui_types/src/styling/color_names.rs
- [ ] T105 [US7] Add compile-time size assertion: assert!(size_of::\<Color\>() <= 16) in crates/flui_types/src/lib.rs
- [ ] T106 [US7] Verify all color tests now PASS (run `cargo test color_test`)

**Checkpoint**: At this point, complete Color system available with multiple blending modes

---

## Phase 10: User Story 8 - Precise Rendering (Priority: P3)

**Goal**: Provide DevicePixels type for GPU rendering that maps 1:1 with framebuffer pixels

**Independent Test**: Convert layout Rect\<Pixels\> to render Rect\<DevicePixels\>, verify pixel-perfect alignment

### Tests for User Story 8 (Test-First Required)

> **NOTE: DevicePixels type was already created in Phase 7, now add comprehensive tests**

- [ ] T107 [P] [US8] Write failing unit test for Point\<DevicePixels\> operations in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T108 [P] [US8] Write failing unit test for Rect\<DevicePixels\> GPU alignment in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T109 [US8] Verify DevicePixels geometry tests FAIL (run `cargo test geometry_test`)

### Implementation for User Story 8

- [ ] T110 [US8] Add documentation for DevicePixels usage in GPU rendering in crates/flui_types/src/units/pixels.rs
- [ ] T111 [US8] Implement Point\<DevicePixels\>::to_logical_pixels(scale) in crates/flui_types/src/geometry/point.rs
- [ ] T112 [US8] Implement Rect\<DevicePixels\>::to_logical_pixels(scale) in crates/flui_types/src/geometry/rect.rs
- [ ] T113 [US8] Verify DevicePixels geometry tests now PASS (run `cargo test geometry_test`)

**Checkpoint**: At this point, DevicePixels type fully supported for pixel-perfect GPU rendering

---

## Phase 11: User Story 9 - Corner Radii (Priority: P3)

**Goal**: Provide Corners\<T\> type for per-corner values (e.g., rounded rectangle radii)

**Independent Test**: Create Corners with different radii per corner, verify geometric calculations

### Tests for User Story 9 (Test-First Required)

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T114 [P] [US9] Write failing unit test for Corners::all(), Corners::top(), Corners::bottom() in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T115 [P] [US9] Write failing unit test for Corners::only() selective corners in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T116 [US9] Verify Corners tests FAIL (run `cargo test geometry_test`)

### Implementation for User Story 9

- [ ] T117 [US9] Implement Corners\<T\> struct (generic over value type, not Unit) in crates/flui_types/src/geometry/corners.rs
- [ ] T118 [US9] Implement Corners::new(), Corners::all() in crates/flui_types/src/geometry/corners.rs
- [ ] T119 [US9] Implement Corners::top(), Corners::bottom(), Corners::only() in crates/flui_types/src/geometry/corners.rs
- [ ] T120 [US9] Verify Corners tests now PASS (run `cargo test geometry_test`)

**Checkpoint**: At this point, Corners type available for rounded rectangles and per-corner styling

---

## Phase 12: User Story 10 - RTL Support (Priority: P3)

**Goal**: Enhance EdgeInsets with RTL-aware start/end semantics for bidirectional layouts

**Independent Test**: Create EdgeInsets with start/end, verify automatic mirroring in RTL context

### Tests for User Story 10 (Test-First Required)

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T121 [P] [US10] Write failing unit test for EdgeInsets::with_start_end() in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T122 [P] [US10] Write failing unit test for EdgeInsets RTL mirroring in crates/flui_types/tests/unit_tests/geometry_test.rs
- [ ] T123 [US10] Verify RTL tests FAIL (run `cargo test geometry_test`)

### Implementation for User Story 10

- [ ] T124 [US10] Add LayoutDirection enum (LTR, RTL) in crates/flui_types/src/geometry/mod.rs
- [ ] T125 [US10] Implement EdgeInsets::with_start_end(direction, start, end) in crates/flui_types/src/geometry/edges.rs
- [ ] T126 [US10] Implement EdgeInsets::resolve(direction) to convert start/end to left/right in crates/flui_types/src/geometry/edges.rs
- [ ] T127 [US10] Verify RTL tests now PASS (run `cargo test geometry_test`)

**Checkpoint**: All user stories (US1-US10) now complete with RTL layout support

---

## Phase 13: Polish & Cross-Cutting Concerns

**Purpose**: Improvements that affect multiple user stories and final quality gates

### Performance Validation

- [ ] T128 [P] Implement Point::distance_to benchmark in crates/flui_types/benches/geometry_bench.rs (target: <10ns)
- [ ] T129 [P] Implement Rect::intersect benchmark in crates/flui_types/benches/geometry_bench.rs (target: <20ns)
- [ ] T130 [P] Implement Rect::union benchmark in crates/flui_types/benches/geometry_bench.rs (target: <20ns)
- [ ] T131 [P] Implement Color::mix benchmark in crates/flui_types/benches/color_bench.rs (target: <20ns)
- [ ] T132 [P] Implement Color::blend_over benchmark in crates/flui_types/benches/color_bench.rs (target: <20ns)
- [ ] T133 [P] Implement unit conversion benchmarks in crates/flui_types/benches/conversions_bench.rs
- [ ] T134 Run all benchmarks and verify performance targets met (run `cargo bench`)

### Examples & Documentation

- [ ] T135 [P] Create basic_usage.rs example demonstrating Pixels, Point, Rect in crates/flui_types/examples/basic_usage.rs
- [ ] T136 [P] Create unit_conversions.rs example demonstrating layout-to-render pipeline in crates/flui_types/examples/unit_conversions.rs
- [ ] T137 [P] Create color_blending.rs example demonstrating mix, blend_over, lighten in crates/flui_types/examples/color_blending.rs
- [ ] T138 [P] Add comprehensive doc comments to all public APIs in src/ files
- [ ] T139 [P] Create crate README.md with quickstart, features, installation in crates/flui_types/README.md
- [ ] T140 Run doc tests and verify all examples compile (run `cargo test --doc`)

### Final Quality Gates

- [ ] T141 Run full test suite with coverage report (run `cargo test --all-features`)
- [ ] T142 Verify coverage ≥80% per constitution requirement (run `cargo tarpaulin --out Html`)
- [ ] T143 Run Clippy with -D warnings (run `cargo clippy --all-features -- -D warnings`)
- [ ] T144 Run rustfmt check (run `cargo fmt --all -- --check`)
- [ ] T145 Verify clean build completes in <5 seconds per spec (run `cargo clean && cargo build --release --timings`)
- [ ] T146 [P] Run WASM compatibility test (run `cargo build --target wasm32-unknown-unknown`)
- [ ] T147 Validate quickstart.md examples compile and run correctly

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies - can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion - BLOCKS all user stories
- **User Stories (Phase 3-12)**: All depend on Foundational phase completion
  - US1 (P1): Can start after Foundational - No dependencies on other stories
  - US2 (P1): Depends on US1 (needs Pixels, Point, Rect to test type constraints)
  - US3 (P1): Depends on US1 (extends Point, Rect API)
  - US4 (P2): Can start after Foundational - Independent (Rems is separate type)
  - US5 (P2): Depends on US1 and US4 (needs Pixels, DevicePixels, Rems for conversions)
  - US6 (P2): Depends on US3 (Rect::inset_by needs full Rect API)
  - US7 (P2): Can start after Foundational - Independent (Color is separate system)
  - US8 (P3): Depends on US5 (DevicePixels created there, now add tests)
  - US9 (P3): Can start after Foundational - Independent (Corners is generic over T)
  - US10 (P3): Depends on US6 (extends EdgeInsets API)
- **Polish (Phase 13)**: Depends on all desired user stories being complete

### User Story Dependencies (Critical Path)

```
Foundation (Phase 2)
    ├─> US1 (Phase 3) Device-Independent Layout 🎯 MVP
    │   ├─> US2 (Phase 4) Unit Mixing Prevention
    │   │   └─> US3 (Phase 5) Geometric Calculations
    │   │       └─> US6 (Phase 8) Padding and Margins
    │   │           └─> US10 (Phase 12) RTL Support
    │   └─> US5 (Phase 7) Unit Conversions
    │       └─> US8 (Phase 10) Precise Rendering
    ├─> US4 (Phase 6) Font-Relative Sizing (independent)
    ├─> US7 (Phase 9) Colors (independent)
    └─> US9 (Phase 11) Corner Radii (independent)
```

### Within Each User Story

1. **Tests FIRST** (all [P] test tasks run in parallel)
2. **Verify tests FAIL** (critical - ensures test validity)
3. **Implementation** (follow dependency order within story)
4. **Verify tests PASS** (validates implementation correctness)
5. **Story checkpoint** (test independently before next story)

### Parallel Opportunities

- **Phase 1 Setup**: T003, T004, T005 can run in parallel
- **Phase 2 Foundational**: T008, T009, T010 can run in parallel
- **Within each user story**:
  - All test writing tasks marked [P] can run in parallel
  - Multiple implementation tasks marked [P] can run in parallel (different files)
- **Phase 13 Polish**: T128-T133 (benchmarks), T135-T140 (examples/docs) can all run in parallel

### MVP Critical Path (User Story 1 Only)

```
T001 → T002 → T003-T005 (parallel) → T006 → T007 → T008-T010 (parallel) →
T011-T015 (parallel) → T016 → T017-T019 (sequential) → T020 →
T021-T022 (parallel) → T023-T024 (sequential) → T025-T026 (sequential) → T027 → T028
```

**Total MVP Tasks**: 28 tasks (Phase 1-3)
**Estimated MVP Time**: 2-3 days with test-first approach

---

## Parallel Example: User Story 1

```bash
# Step 1: Write all tests in parallel (T011-T015)
Parallel Tasks:
- T011: Write failing test for Pixels::new() in tests/unit_tests/units_test.rs
- T012: Write failing test for Pixels arithmetic in tests/unit_tests/units_test.rs
- T013: Write failing test for Point in tests/unit_tests/geometry_test.rs
- T014: Write failing test for Size in tests/unit_tests/geometry_test.rs
- T015: Write failing test for Rect in tests/unit_tests/geometry_test.rs

# Step 2: Verify all fail (T016) - CRITICAL GATE
Sequential Task:
- T016: Run `cargo test` - verify 0 passed, N failed

# Step 3: Implement models in parallel (T021-T022)
Parallel Tasks:
- T021: Implement Point<T> in src/geometry/point.rs
- T022: Implement Size<T> in src/geometry/size.rs

# Step 4: Verify tests pass (T027) - VALIDATION GATE
Sequential Task:
- T027: Run `cargo test` - verify all US1 tests pass
```

---

## Parallel Example: User Story 7 (Colors)

```bash
# Step 1: Write all tests in parallel (T085-T092)
Parallel Tasks:
- T085: Color::from_rgb test in tests/unit_tests/color_test.rs
- T086: Color::from_hex valid test in tests/unit_tests/color_test.rs
- T087: Color::from_hex invalid test in tests/unit_tests/color_test.rs
- T088: Color::mix boundaries test in tests/unit_tests/color_test.rs
- T089: Color::blend_over test in tests/unit_tests/color_test.rs
- T090: Color::scale test in tests/unit_tests/color_test.rs
- T091: Color::lighten/darken test in tests/unit_tests/color_test.rs
- T092: Color mix commutativity property test in tests/property_tests/color_properties.rs

# Step 2: Verify all fail (T093) - CRITICAL GATE
Sequential Task:
- T093: Run `cargo test color_test` - verify failures

# Step 3: Implement in parallel where possible
Note: Some tasks depend on Color struct existing (T094), so not all parallel
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

**Goal**: Deliver minimal viable type-safe unit system in 2-3 days

1. Complete Phase 1: Setup (5 tasks)
2. Complete Phase 2: Foundational (5 tasks)
3. Complete Phase 3: User Story 1 - Device-Independent Layout (18 tasks)
4. **STOP and VALIDATE**:
   - Run full test suite: `cargo test`
   - Build clean: `cargo clean && cargo build --release`
   - Verify: Pixels, Point, Size, Rect fully functional
5. Create MVP demo example
6. Tag release: `v0.1.0-mvp`

**MVP Deliverable**: Developers can write device-independent layouts with Pixels, safe from unit mixing bugs

### Incremental Delivery (MVP + P1 Stories)

**Goal**: Complete all P1 user stories for solid foundation

1. MVP (US1) → **Test independently** → Demo/Deploy
2. Add US2 (Unit Mixing Prevention) → **Test independently** → Verify compile-time safety
3. Add US3 (Geometric Calculations) → **Test independently** → Verify geometric operations
4. **STOP and VALIDATE**: All P1 stories complete
5. Tag release: `v0.2.0-p1-complete`

**P1 Deliverable**: Full geometric primitives with compile-time type safety

### Full Feature Set (All Priorities)

**Goal**: Complete comprehensive type-safe unit library

1. P1 Complete (US1-3)
2. Add P2 Stories:
   - US4 (Rems) → Independent
   - US5 (Conversions) → Independent test
   - US6 (EdgeInsets) → Independent test
   - US7 (Colors) → Independent test
3. **STOP and VALIDATE**: All P2 stories work together
4. Add P3 Stories:
   - US8 (DevicePixels tests) → Independent test
   - US9 (Corners) → Independent test
   - US10 (RTL) → Independent test
5. Complete Phase 13: Polish
6. Tag release: `v1.0.0`

**Full Deliverable**: Production-ready flui-types crate with all features

### Parallel Team Strategy

With 3 developers after Foundational phase:

1. **Team completes Setup + Foundational together** (2-4 hours)
2. **Once Foundational done**:
   - **Developer A**: US1 → US2 → US3 (P1 critical path)
   - **Developer B**: US4 → US7 (P2 independent: Rems + Colors)
   - **Developer C**: US9 (P3 independent: Corners)
3. **After P1 critical path (Dev A) complete**:
   - **Developer A**: US5 → US6 → US10
   - **Developer B**: US8 (depends on US5)
   - **Developer C**: Start Phase 13 (benchmarks, examples)
4. **Final integration**: All developers on Phase 13 polish

**Team Benefit**: 3x speedup on independent stories, coordinated integration

---

## Test-First Compliance Checklist

Per Constitution Principle IV, ALL public APIs require test-first development:

- [ ] Every user story phase has test tasks BEFORE implementation tasks
- [ ] Every test phase includes verification of FAIL state (Red)
- [ ] Every implementation phase includes verification of PASS state (Green)
- [ ] Property tests cover geometric invariants (commutativity, symmetry, triangle inequality)
- [ ] Compile-fail tests verify type safety (trybuild for unit mixing prevention)
- [ ] Performance benchmarks verify contracts (<10ns distance, <20ns intersect)
- [ ] Coverage target ≥80% verified in Phase 13 (T142)

**Red-Green-Refactor Cycle**:
1. **Red**: Write tests → Verify they FAIL
2. **Green**: Implement → Verify tests PASS
3. **Refactor**: Optimize (Phase 13 benchmarks) → Verify tests still PASS

---

## Notes

- **[P] tasks**: Different files, no dependencies - safe to parallelize
- **[Story] label**: Maps task to specific user story for independent delivery
- **Test-first MANDATORY**: Constitution requirement - all test tasks before implementation
- **Verify FAIL state**: Critical gate - ensures tests actually test the right thing
- **Checkpoints**: Each user story must be independently testable before next story
- **MVP focus**: User Story 1 is minimal viable product (28 tasks, 2-3 days)
- **Performance gates**: Benchmarks in Phase 13 verify <10ns, <20ns contracts
- **Memory gates**: Compile-time assertions verify size limits (Point≤8, Color≤16)
- **Independence**: Most user stories are independent after Foundational phase
- **Critical path**: US1 → US2 → US3 → US6 → US10 (for complete geometric API)

**Total Task Count**: 147 tasks
- Phase 1 (Setup): 5 tasks
- Phase 2 (Foundational): 5 tasks
- Phase 3 (US1 - P1): 18 tasks
- Phase 4 (US2 - P1): 10 tasks
- Phase 5 (US3 - P1): 16 tasks
- Phase 6 (US4 - P2): 8 tasks
- Phase 7 (US5 - P2): 12 tasks
- Phase 8 (US6 - P2): 10 tasks
- Phase 9 (US7 - P2): 22 tasks
- Phase 10 (US8 - P3): 7 tasks
- Phase 11 (US9 - P3): 7 tasks
- Phase 12 (US10 - P3): 7 tasks
- Phase 13 (Polish): 20 tasks

**Parallel Opportunities**: 89 tasks marked [P] can run in parallel (60% of total)
**MVP Scope**: Phase 1-3 (28 tasks) delivers working Pixels, Point, Size, Rect
**Suggested First PR**: Complete US1 (Phase 3) - demonstrates value, validates approach
