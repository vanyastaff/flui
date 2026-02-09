# Performance Guide

Internal documentation for performance characteristics and optimization in `flui_interaction`.

## Performance Architecture

```
Input → Resampler → Predictor → Recognizers → Arena → Callbacks
         ↓            ↓            ↓           ↓
    Buffering    Extrapolation  Detection  Resolution
```

## Input Processing Pipeline

### VelocityTracker

Polynomial regression-based velocity estimation.

```rust
use flui_interaction::processing::VelocityTracker;

let mut tracker = VelocityTracker::new();

// Add samples (keep within 100ms window)
tracker.add_position(Instant::now(), position);

// Get velocity (px/s)
let velocity = tracker.velocity();
println!("Speed: {} px/s", velocity.magnitude());
```

**Estimation Strategies:**

| Strategy | Accuracy | Performance | Use Case |
|----------|----------|-------------|----------|
| `LeastSquaresPolynomial` | High | Moderate | Default, gesture detection |
| `LinearRegression` | Medium | Fast | Simple tracking |
| `TwoSample` | Low | Fastest | High-frequency updates |

```rust
let tracker = VelocityTracker::with_strategy(
    VelocityEstimationStrategy::LinearRegression
);
```

**Algorithm Constants:**
- `HORIZON`: 100ms sample window
- `MIN_SAMPLES`: 2 samples minimum
- `MAX_SAMPLES`: 20 samples maximum
- `POLYNOMIAL_DEGREE`: 2 (quadratic fit)

**Performance Characteristics:**
- Sample storage: O(MAX_SAMPLES) = O(20)
- Add sample: O(n) for old sample removal
- Velocity calculation: O(n²) for polynomial fit, O(n) for linear

### PointerEventResampler

Event buffering and temporal alignment.

```rust
use flui_interaction::processing::PointerEventResampler;

let resampler = PointerEventResampler::new(pointer_id);

// Buffer incoming events
resampler.add_event(event);

// Sample at frame rate
resampler.sample(now, next_frame, |resampled| {
    process_event(resampled);
});
```

**Use Cases:**
- Low-frequency touch sensors
- Mismatched input/display rates (120Hz input → 90Hz display)
- Stylus input smoothing

**Constants:**
- `MAX_BUFFERED_EVENTS`: 100 events
- `MIN_SAMPLE_INTERVAL`: 1ms

**Thread Safety:**
Uses `Arc<Mutex<_>>` internally for safe concurrent access.

### InputPredictor

Future position extrapolation for latency reduction.

```rust
use flui_interaction::processing::{InputPredictor, PredictionConfig};

// For games (low latency)
let mut predictor = InputPredictor::for_games();

// For UI (smooth, conservative)
let mut predictor = InputPredictor::for_ui();

// Custom config
let predictor = InputPredictor::with_config(PredictionConfig {
    max_prediction_time: Duration::from_millis(32),
    use_acceleration: true,
    smoothing: 0.2,
    velocity_strategy: VelocityEstimationStrategy::LeastSquaresPolynomial,
});
```

**Prediction Accuracy by Time:**

| Prediction Time | Accuracy | Recommended |
|-----------------|----------|-------------|
| 8-16ms | High | Interactive UI |
| 16-32ms | Medium | Games, drawing |
| 32-50ms | Low | With caution |

```rust
// Add samples
predictor.add_sample(Instant::now(), position);

// Predict for next frame
let predicted = predictor.predict_next_frame(60);
if predicted.is_confident() {
    render_at(predicted.position);
} else {
    render_at(last_known_position);
}
```

**Prediction Algorithm:**
1. Linear extrapolation: `pos + velocity * dt`
2. Quadratic extrapolation (optional): `+ 0.5 * acceleration * dt²`
3. Exponential smoothing to reduce jitter

## Gesture Arena Performance

### Lock-Free Concurrent Access

Uses `DashMap` for concurrent arena entries:

```rust
// Multiple threads can add/resolve simultaneously
let arena = GestureArena::new();
let arena_clone = arena.clone();

thread::spawn(move || {
    arena_clone.add(pointer, recognizer);
});
```

**Performance Characteristics:**
- Add member: O(1) amortized
- Resolve: O(n) where n = members per pointer (typically 2-4)
- Sweep: O(1)

### SmallVec Optimization

Arena entries use `SmallVec<[_; 4]>` to avoid heap allocation:

```rust
// 4 members stored inline (tap, drag, long-press, scale)
// Most gestures have 2-3 competing recognizers
members: SmallVec<[Arc<dyn GestureArenaMember>; 4]>,
```

### Timeout-Based Disambiguation

```rust
// Per-frame check for stuck arenas
arena.resolve_timed_out_arenas(DEFAULT_DISAMBIGUATION_TIMEOUT);

// Default timeout: 100ms
pub const DEFAULT_DISAMBIGUATION_TIMEOUT: Duration = Duration::from_millis(100);
```

## Memory Characteristics

### Per-Recognizer Overhead

| Type | Stack | Heap (typical) |
|------|-------|----------------|
| `TapGestureRecognizer` | 96 bytes | ~256 bytes (callbacks) |
| `DragGestureRecognizer` | 160 bytes | ~512 bytes (VelocityTracker + callbacks) |
| `ScaleGestureRecognizer` | 256 bytes | ~1KB (HashMap + callbacks) |
| `LongPressGestureRecognizer` | 128 bytes | ~256 bytes |

### Callback Storage

Callbacks stored as `Arc<dyn Fn + Send + Sync>`:
- Single allocation per callback
- Shared reference across clones
- Freed on dispose or clear

### Arena Entry Size

```rust
// Per-pointer entry
struct ArenaEntryData {
    members: SmallVec<[Arc<dyn GestureArenaMember>; 4]>,  // 64 bytes inline
    is_open: bool,           // 1 byte
    is_held: bool,           // 1 byte  
    is_resolved: bool,       // 1 byte
    eager_winner: Option<_>, // 16 bytes
    has_pending_sweep: bool, // 1 byte
    winners: SmallVec<[_; 2]>, // 32 bytes inline
    created_at: Instant,     // 16 bytes
}
// Total: ~140 bytes per pointer (most inline)
```

## Hit Testing Performance

### Transform Stack

```rust
// Pre-allocated transform stack
pub struct HitTestResult {
    path: Vec<HitTestEntry>,      // Result entries
    transforms: Vec<Matrix4>,      // Transform stack
    local_transforms: Vec<Matrix4>, // Accumulated
}
```

**Optimizations:**
- Avoid allocation during hit test traversal
- Transform composition uses cached matrices
- Early exit on first hit (configurable)

### HitTestBehavior

```rust
pub enum HitTestBehavior {
    DeferToChild,   // Only if child hit (default)
    Opaque,         // Always hit, stop propagation
    Translucent,    // Hit but continue to children
}
```

**Opaque** provides early exit for performance-critical paths.

## Thread Safety Model

All public types are `Send + Sync`:

| Type | Thread Safety |
|------|---------------|
| `GestureArena` | `DashMap` (lock-free) |
| `*GestureRecognizer` | `parking_lot::Mutex` |
| `VelocityTracker` | Not `Sync` (mutable) |
| `PointerEventResampler` | `Arc<Mutex<_>>` |
| `InputPredictor` | Not `Sync` (mutable) |

### Recommended Patterns

**Single-threaded (main thread only):**
```rust
// Direct usage, no synchronization needed
let mut tracker = VelocityTracker::new();
tracker.add_position(now, pos);
```

**Multi-threaded (shared recognizers):**
```rust
// Arena handles internal synchronization
let arena = GestureArena::new();

// Recognizers are Arc'd and clonable
let recognizer = TapGestureRecognizer::new(arena.clone());
let recognizer_clone = recognizer.clone();
```

## Benchmarking Recommendations

### Velocity Estimation

```rust
#[bench]
fn bench_velocity_polynomial(b: &mut Bencher) {
    let mut tracker = VelocityTracker::new();
    let start = Instant::now();
    
    b.iter(|| {
        for i in 0..10 {
            tracker.add_position(
                start + Duration::from_millis(i * 10),
                Offset::new(i as f32 * 10.0, 0.0)
            );
        }
        tracker.velocity()
    });
}
```

### Hit Testing

```rust
#[bench]
fn bench_hit_test_deep_tree(b: &mut Bencher) {
    let tree = build_deep_tree(100); // 100 levels
    let result = HitTestResult::new();
    
    b.iter(|| {
        result.clear();
        tree.hit_test(&mut result, Offset::new(50.0, 50.0))
    });
}
```

## Optimization Checklist

### Input Processing
- [ ] Use appropriate velocity strategy for use case
- [ ] Consider resampling for high-frequency input
- [ ] Enable prediction only when needed
- [ ] Tune prediction time for accuracy/latency tradeoff

### Gesture Recognition
- [ ] Limit concurrent recognizers per widget (2-4 typical)
- [ ] Use `HitTestBehavior::Opaque` for leaf widgets
- [ ] Dispose unused recognizers promptly
- [ ] Avoid creating recognizers per-frame

### Arena Management
- [ ] Call `resolve_timed_out_arenas` per frame
- [ ] Use `sweep` on pointer up
- [ ] Pre-allocate arena with expected capacity

### Memory
- [ ] Prefer builder pattern callbacks (single allocation)
- [ ] Clone `Arc<Recognizer>` not recreate
- [ ] Clear event buffers on reset

## Profiling Points

Key functions to profile:

```rust
// Velocity calculation
VelocityTracker::polynomial_velocity
VelocityTracker::linear_velocity

// Hit testing
HitTestResult::add
transform_stack operations

// Arena resolution
GestureArena::resolve
GestureArena::close
ArenaEntryData::resolve

// Event processing
PointerEventResampler::sample
InputPredictor::predict
```

## Platform Considerations

### Touch vs Mouse

| Metric | Touch | Mouse |
|--------|-------|-------|
| Event frequency | 60-120 Hz | 125-1000 Hz |
| Slop tolerance | 18px | Lower recommended |
| Prediction benefit | High | Lower |
| Multi-pointer | Common | Rare |

### High-DPI Displays

Slop values are in logical pixels:
```rust
// Scale slop for physical pixel density if needed
let scaled_slop = base_slop * dpi_scale;
```

## See Also

- [ARCHITECTURE.md](ARCHITECTURE.md) - Core architecture
- [GESTURES.md](GESTURES.md) - Gesture recognizers
- [HIT_TESTING.md](HIT_TESTING.md) - Hit testing system
