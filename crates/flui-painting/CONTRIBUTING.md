# Contributing to flui_painting

Thank you for your interest in contributing to `flui_painting`! This document provides guidelines and best practices for contributors.

## Table of Contents

- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Code Style](#code-style)
- [Testing](#testing)
- [Documentation](#documentation)
- [Pull Requests](#pull-requests)
- [Performance](#performance)

## Getting Started

### Prerequisites

- **Rust:** 1.70 or later
- **Cargo:** Latest stable
- **Git:** For version control

### Building from Source

```bash
# Clone the repository
git clone https://github.com/flui-org/flui.git
cd flui/crates/flui_painting

# Build the crate
cargo build

# Run tests
cargo test

# Run examples
cargo run --example basic_canvas
```

### Project Structure

```
flui_painting/
├── src/
│   ├── lib.rs           # Crate root with re-exports
│   ├── canvas.rs        # Canvas implementation
│   ├── display_list.rs  # DisplayList and DrawCommand
│   └── error.rs         # Error types
├── tests/
│   ├── canvas_composition.rs
│   ├── canvas_scoped.rs
│   ├── canvas_transform.rs
│   └── thread_safety.rs
├── docs/                # Documentation (this directory)
├── examples/            # Usage examples
└── benches/             # Benchmarks (planned)
```

## Development Setup

### Recommended Tools

```bash
# Formatter
rustfmt

# Linter
cargo install clippy

# Documentation
cargo doc --open

# Code coverage (optional)
cargo install cargo-tarpaulin

# Benchmarking (optional)
cargo install cargo-criterion
```

### Editor Setup

#### VS Code

Install extensions:
- `rust-analyzer` - Rust language server
- `crates` - Crate version management
- `Better TOML` - TOML syntax

Settings:

```json
{
    "rust-analyzer.checkOnSave.command": "clippy",
    "rust-analyzer.cargo.features": "all"
}
```

#### IntelliJ IDEA / RustRover

- Install Rust plugin
- Enable "Run clippy on save"
- Configure rustfmt as default formatter

## Code Style

### Formatting

We use `rustfmt` with default settings:

```bash
# Format all code
cargo fmt --all

# Check formatting
cargo fmt --all -- --check
```

### Linting

We enforce strict clippy lints:

```bash
# Run clippy
cargo clippy --all-targets --all-features -- -D warnings

# Auto-fix issues
cargo clippy --fix
```

### Naming Conventions

```rust
// Types: PascalCase
pub struct Canvas { }
pub enum DrawCommand { }
pub trait DisplayListCore { }

// Functions/methods: snake_case
pub fn draw_rect() { }
pub fn append_canvas() { }

// Constants: SCREAMING_SNAKE_CASE
pub const MAX_SAVE_DEPTH: usize = 256;

// Modules: snake_case
mod display_list;
mod error;
```

### Documentation Style

```rust
/// Brief one-line description.
///
/// More detailed explanation of the type/function.
///
/// # Examples
///
/// ```rust
/// use flui_painting::Canvas;
///
/// let mut canvas = Canvas::new();
/// canvas.draw_rect(rect, &paint);
/// ```
///
/// # Panics
///
/// This function panics if... (only if applicable)
///
/// # Errors
///
/// Returns an error if... (only for Result-returning functions)
///
/// # Safety
///
/// The caller must ensure... (only for unsafe code)
pub fn example() { }
```

### Code Organization

```rust
// Order of items in a file:
// 1. Module-level documentation
// 2. Imports (grouped: std, external crates, internal)
// 3. Type definitions
// 4. Trait implementations
// 5. impl blocks (inherent methods before trait impls)
// 6. Private helper functions
// 7. Tests module

//! Module documentation

use std::collections::HashMap;

use external_crate::Type;

use crate::internal::Module;

pub struct Example {
    // Public fields first
    pub field: i32,
    // Private fields after
    private: String,
}

impl Example {
    // Constructors first
    pub fn new() -> Self { }

    // Public methods
    pub fn public_method(&self) { }

    // Private methods
    fn private_method(&self) { }
}

// Trait implementations
impl Display for Example { }

#[cfg(test)]
mod tests { }
```

## Testing

### Writing Tests

We use three types of tests:

#### 1. Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canvas_creation() {
        let canvas = Canvas::new();
        assert_eq!(canvas.len(), 0);
        assert!(canvas.is_empty());
    }
}
```

#### 2. Integration Tests

Located in `tests/` directory:

```rust
// tests/canvas_features.rs
use flui_painting::prelude::*;

#[test]
fn test_transform_composition() {
    // Test across multiple modules
}
```

#### 3. Documentation Tests

```rust
/// # Examples
///
/// ```
/// use flui_painting::Canvas;
///
/// let mut canvas = Canvas::new();
/// assert!(canvas.is_empty());
/// ```
```

### Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test test_canvas_creation

# With output
cargo test -- --nocapture

# Documentation tests
cargo test --doc

# Integration tests only
cargo test --tests
```

### Test Coverage

We aim for:
- **80%+ line coverage** for core functionality
- **100% coverage** for public API
- **Edge cases** thoroughly tested

```bash
# Generate coverage report (requires tarpaulin)
cargo tarpaulin --out Html

# Open coverage report
open tarpaulin-report.html
```

### Testing Guidelines

**DO:**
- Test public API thoroughly
- Test edge cases (empty, zero, negative, max values)
- Test error conditions
- Use descriptive test names: `test_canvas_restore_without_save`

**DON'T:**
- Test private implementation details
- Write flaky tests (timing-dependent, random)
- Duplicate tests unnecessarily

## Documentation

### Documentation Requirements

All public items must be documented:

```rust
// ✅ Good - documented
/// Creates a new canvas.
pub fn new() -> Self { }

// ❌ Bad - missing documentation
pub fn new() -> Self { }
```

### Documentation Best Practices

1. **Start with a brief summary** (one sentence)
2. **Add examples** for non-trivial functions
3. **Document panics** if applicable
4. **Document errors** for Result-returning functions
5. **Link to related items** using backticks and square brackets

```rust
/// Draws a rectangle on the canvas.
///
/// The rectangle is filled or stroked according to the [`Paint`] style.
///
/// # Examples
///
/// ```
/// use flui_painting::{Canvas, Paint};
/// use flui_types::{Rect, Color};
///
/// let mut canvas = Canvas::new();
/// let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
/// let paint = Paint::fill(Color::RED);
/// canvas.draw_rect(rect, &paint);
/// ```
///
/// # See Also
///
/// - [`draw_rrect`](Self::draw_rrect) for rounded rectangles
/// - [`draw_path`](Self::draw_path) for custom shapes
pub fn draw_rect(&mut self, rect: Rect, paint: &Paint) { }
```

### Building Documentation

```bash
# Build and open docs
cargo doc --open

# Check for broken links
cargo doc --no-deps

# Build with private items
cargo doc --document-private-items
```

## Pull Requests

### Before Submitting

1. **Format code:** `cargo fmt --all`
2. **Run lints:** `cargo clippy --all-targets -- -D warnings`
3. **Run tests:** `cargo test --all`
4. **Update docs:** Add/update documentation as needed
5. **Add examples:** For new features

### PR Guidelines

**Title Format:**

```
<type>: <brief description>

Examples:
feat: Add batch drawing methods
fix: Correct bounds calculation in clip_path
docs: Update architecture guide
perf: Optimize canvas composition
test: Add thread safety tests
```

**Description Template:**

```markdown
## Summary

Brief description of changes.

## Changes

- Added X
- Fixed Y
- Updated Z

## Testing

- Added unit tests for X
- Verified Y manually
- Benchmarked Z (results: ...)

## Breaking Changes

None / List breaking changes

## Checklist

- [ ] Code formatted (`cargo fmt`)
- [ ] Lints pass (`cargo clippy`)
- [ ] Tests pass (`cargo test`)
- [ ] Documentation updated
- [ ] Examples added/updated
```

### Review Process

1. **Automated checks** run (CI)
2. **Code review** by maintainers
3. **Address feedback** if needed
4. **Merge** once approved

### Commit Messages

Follow conventional commits:

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

**Types:**
- `feat` - New feature
- `fix` - Bug fix
- `docs` - Documentation
- `style` - Formatting
- `refactor` - Code restructuring
- `perf` - Performance improvement
- `test` - Adding tests
- `chore` - Maintenance

**Examples:**

```
feat(canvas): add batch drawing methods

Add draw_rects(), draw_circles(), and draw_lines() for efficient
batch drawing of multiple primitives.

Closes #123
```

```
fix(display_list): correct bounds calculation in append

The bounds calculation was incorrect when appending an empty
display list. Now correctly handles this edge case.
```

## Performance

### Benchmarking

When making performance-related changes:

1. **Benchmark before** your changes
2. **Make changes**
3. **Benchmark after** your changes
4. **Include results** in PR description

```bash
# Run benchmarks
cargo bench

# Specific benchmark
cargo bench canvas_composition

# Save baseline
cargo bench -- --save-baseline before

# Compare to baseline
cargo bench -- --baseline before
```

### Performance Requirements

- **No regressions** without justification
- **Document performance characteristics** in comments
- **Profile** if adding complex algorithms
- **Consider memory** usage in addition to CPU time

### Performance Testing

```rust
#[test]
fn test_composition_performance() {
    use std::time::Instant;

    let mut parent = Canvas::new();
    let mut child = Canvas::new();

    // Fill child with commands
    for i in 0..1000 {
        child.draw_rect(rect, &paint);
    }

    // Measure composition
    let start = Instant::now();
    parent.append_canvas(child);
    let duration = start.elapsed();

    // Should be fast (< 10µs)
    assert!(duration.as_micros() < 10);
}
```

## Common Tasks

### Adding a New DrawCommand Variant

1. **Add variant** to `DrawCommand` enum
2. **Implement** required traits (Clone, Debug, etc.)
3. **Add** matching `Canvas::draw_*` method
4. **Update** `DrawCommand::kind()` if needed
5. **Add tests**
6. **Document** with examples

### Adding a Canvas Method

1. **Add method** to `Canvas` impl block
2. **Document** with examples
3. **Add tests**
4. **Consider** adding chaining variant
5. **Consider** adding scoped variant

### Adding a DisplayList Method

1. **Decide** if it belongs in `DisplayListCore` or `DisplayListExt`
2. **Implement** method
3. **Add tests**
4. **Document** with examples

## Questions?

- **API design questions:** Open a discussion on GitHub
- **Bug reports:** Open an issue with reproducible example
- **Feature requests:** Open an issue explaining use case
- **General questions:** Ask in discussions or Discord

## Code of Conduct

Be respectful, inclusive, and collaborative. We follow the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct).

## License

By contributing, you agree that your contributions will be licensed under the same terms as the project (MIT/Apache-2.0).
