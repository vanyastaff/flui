# Universal Crate Refactoring Prompt

Short, reusable prompt template for refactoring any FLUI crate to modern Rust standards.

---

## Template Prompt

```markdown
# Refactor: {CRATE_NAME}

## Task
Refactor `{CRATE_NAME}` crate to modern Rust 1.90+ standards with zero-cost generic abstractions and complete safety.

## Principles

### 1. Generic-First (No `dyn` in hot paths)
- ❌ BAD: `Box<dyn Trait>`, `&dyn Trait`
- ✅ GOOD: `T: Trait`, generic parameters

### 2. Compile-Time Safety
- ❌ BAD: `.unwrap()`, `.expect()`, `panic!()`
- ✅ GOOD: `?`, `Result<T, E>`, `Option<T>`

### 3. Rust 1.90+ Features
- ✅ Const generics: `struct Foo<const N: usize>`
- ✅ GATs: `type Item<'a> where Self: 'a`
- ✅ LazyLock: Replace `OnceCell`
- ✅ Const functions: `const fn new()`
- ✅ impl Trait in return: `fn foo() -> impl Iterator`

### 4. Zero Unsafe (Unless Necessary)
- Remove all unsafe code OR
- Add detailed SAFETY comments explaining invariants

### 5. Pattern Matching Over Conditionals
- ❌ BAD: `if x.is_some() { x.unwrap() }`
- ✅ GOOD: `if let Some(v) = x { v }`

## Steps

### Step 1: Analyze Current Code
```bash
cd crates/{CRATE_NAME}

# Find all bad patterns
rg "\.unwrap\(\)" src/
rg "\.expect\(" src/
rg "panic!" src/
rg "Box<dyn" src/
rg "&dyn" src/
rg "unsafe" src/
rg "OnceCell" src/
```

### Step 2: Replace Trait Objects with Generics

**Before:**
```rust
struct Container {
    items: Vec<Box<dyn Item>>,
}
```

**After:**
```rust
struct Container<T: Item> {
    items: Vec<T>,
}

// If heterogeneous collection needed, use enum:
enum ItemKind {
    TypeA(TypeA),
    TypeB(TypeB),
}
```

### Step 3: Remove unwrap/expect

**Before:**
```rust
fn get_value(&self) -> String {
    self.value.unwrap()  // ❌ BAD
}
```

**After:**
```rust
fn get_value(&self) -> Option<String> {
    self.value.clone()  // ✅ GOOD
}

// Or with Result:
fn get_value(&self) -> Result<String, Error> {
    self.value.ok_or(Error::NoValue)
}
```

### Step 4: Use Rust 1.90+ Features

**Replace OnceCell:**
```rust
// Before
use once_cell::sync::OnceCell;
static FOO: OnceCell<Config> = OnceCell::new();

// After (Rust 1.80+)
use std::sync::LazyLock;
static FOO: LazyLock<Config> = LazyLock::new(|| Config::load());
```

**Add const generics:**
```rust
// Before
struct Array {
    data: Vec<u8>,
    len: usize,
}

// After
struct Array<const N: usize> {
    data: [u8; N],
}
```

**Use const fn:**
```rust
// Before
fn new() -> Self { ... }

// After
const fn new() -> Self { ... }
```

### Step 5: Document Unsafe (If Any)

**Before:**
```rust
unsafe {
    *ptr = value;
}
```

**After:**
```rust
// SAFETY: `ptr` is guaranteed to be:
// 1. Valid - allocated via Box::into_raw
// 2. Aligned - Box ensures proper alignment
// 3. Initialized - value written before any read
// 4. Unique - no other references exist (single-threaded context)
// 5. Lifetime - pointer valid for 'static
unsafe {
    *ptr = value;
}
```

### Step 6: Improve Error Handling

**Before:**
```rust
fn parse(s: &str) -> Config {
    let val = s.parse().unwrap();
    Config { val }
}
```

**After:**
```rust
#[derive(Debug, thiserror::Error)]
enum ParseError {
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
}

fn parse(s: &str) -> Result<Config, ParseError> {
    let val = s.parse()
        .map_err(|e| ParseError::InvalidFormat(e.to_string()))?;
    Ok(Config { val })
}
```

### Step 7: Use Pattern Matching

**Before:**
```rust
if x.is_some() {
    let v = x.unwrap();
    process(v);
}
```

**After:**
```rust
if let Some(v) = x {
    process(v);
}

// Or with match:
match x {
    Some(v) => process(v),
    None => handle_none(),
}
```

## Acceptance Criteria

Run these commands and ALL must pass:

```bash
cd crates/{CRATE_NAME}

# Build
cargo build
cargo build --release

# Test
cargo test
cargo test --release

# No warnings
cargo clippy -- -D warnings

# Format check
cargo fmt -- --check

# Check for bad patterns (should return nothing)
rg "\.unwrap\(\)" src/ && echo "❌ Found unwrap!" || echo "✅ No unwrap"
rg "\.expect\(" src/ && echo "❌ Found expect!" || echo "✅ No expect"
rg "panic!\(" src/ && echo "❌ Found panic!" || echo "✅ No panic"

# Check dependencies
cargo tree -p {CRATE_NAME} --depth 1
# Verify minimal dependencies

# Check binary size (if applicable)
cargo bloat --release -n 20
```

## Anti-Patterns to Remove

### 1. String Cloning in Loops
```rust
// ❌ BAD
for item in items {
    let s = item.name.clone();  // Clone every iteration!
}

// ✅ GOOD
for item in &items {
    let s = &item.name;  // Borrow
}
```

### 2. Unnecessary Allocations
```rust
// ❌ BAD
fn get_items(&self) -> Vec<Item> {
    self.items.clone()  // Expensive clone!
}

// ✅ GOOD
fn get_items(&self) -> &[Item] {
    &self.items
}
```

### 3. Mutex Over-Use
```rust
// ❌ BAD (if not needed)
struct Cache {
    data: Mutex<HashMap<K, V>>,
}

// ✅ GOOD (if read-heavy)
struct Cache {
    data: RwLock<HashMap<K, V>>,
}

// ✅ EVEN BETTER (if appropriate)
use dashmap::DashMap;
struct Cache {
    data: DashMap<K, V>,  // Lock-free!
}
```

### 4. Stringly-Typed Code
```rust
// ❌ BAD
fn set_mode(&mut self, mode: &str) {
    self.mode = mode.to_string();
}

// ✅ GOOD
enum Mode { Read, Write, Append }

fn set_mode(&mut self, mode: Mode) {
    self.mode = mode;
}
```

### 5. God Structs
```rust
// ❌ BAD
struct Everything {
    config: Config,
    cache: Cache,
    db: Database,
    logger: Logger,
    // ... 20 more fields
}

// ✅ GOOD - Split into focused types
struct Config { ... }
struct Cache { ... }
struct Database { ... }

struct App {
    config: Config,
    cache: Cache,
    db: Database,
}
```

## Modern Rust Idioms

### Use std::mem::take
```rust
// ❌ BAD
let old = std::mem::replace(&mut self.value, Default::default());

// ✅ GOOD
let old = std::mem::take(&mut self.value);
```

### Use if let chains (Rust 1.90+)
```rust
// ❌ BAD
if let Some(x) = opt_x {
    if let Some(y) = opt_y {
        process(x, y);
    }
}

// ✅ GOOD (Rust 1.90+)
if let Some(x) = opt_x && let Some(y) = opt_y {
    process(x, y);
}
```

### Use let-else (Rust 1.65+)
```rust
// ❌ BAD
let Some(value) = self.value else {
    return Err(Error::NoValue);
};

// Already good! (Rust 1.65+)
```

### Use #[must_use]
```rust
#[must_use = "Builder does nothing until .build() is called"]
pub struct ConfigBuilder { ... }

#[must_use = "Iterator is lazy and does nothing unless consumed"]
fn iter(&self) -> impl Iterator<Item = &T> { ... }
```

## Output Format

After refactoring, provide:

1. **Summary of changes:**
   ```
   - Removed 15 .unwrap() calls → replaced with ? and Result
   - Replaced 3 Box<dyn Trait> → generic parameters
   - Added const generics for Array<N>
   - Replaced OnceCell with LazyLock
   - Added SAFETY comments to 2 unsafe blocks
   ```

2. **Performance impact:**
   ```
   Before: 150 KB binary, 250 µs benchmark
   After:  142 KB binary, 210 µs benchmark (16% faster!)
   ```

3. **Remaining issues (if any):**
   ```
   - 1 unsafe block in raw_ptr module (well-documented)
   - 1 expect() in panic handler (acceptable - already panicking)
   ```

## Example: Refactoring flui-tree

```bash
# Use this prompt
cat CRATE_REFACTOR_PROMPT.md | sed 's/{CRATE_NAME}/flui-tree/g'

# Give to AI agent
# Agent refactors crate

# Verify
cd crates/flui-tree
cargo build && cargo test && cargo clippy -- -D warnings
rg "\.unwrap\(\)" src/  # Should be empty

# Commit
git add -A && git commit -m "refactor(flui-tree): Modern Rust 1.90+ patterns"
```

## Example: Refactoring flui_rendering

```bash
cat CRATE_REFACTOR_PROMPT.md | sed 's/{CRATE_NAME}/flui_rendering/g'
# ... same process
```
```

---

## Usage

### For Single Crate:
```bash
# Copy template, replace {CRATE_NAME}
cat docs/CRATE_REFACTOR_PROMPT.md | sed 's/{CRATE_NAME}/flui-tree/g' > /tmp/refactor_flui-tree.md

# Give to AI agent
cat /tmp/refactor_flui-tree.md
```

### For All Crates:
```bash
for crate in flui-tree flui_rendering flui_core flui_widgets; do
    cat docs/CRATE_REFACTOR_PROMPT.md | sed "s/{CRATE_NAME}/$crate/g" > /tmp/refactor_$crate.md
    echo "Refactor prompt ready: /tmp/refactor_$crate.md"
done
```

### Automation Script:
```bash
#!/bin/bash
# refactor_all.sh

CRATES=(
    "flui-tree"
    "flui-foundation"
    "flui_rendering"
    "flui_core"
    "flui_widgets"
)

for crate in "${CRATES[@]}"; do
    echo "=== Refactoring $crate ==="

    # Generate prompt
    sed "s/{CRATE_NAME}/$crate/g" docs/CRATE_REFACTOR_PROMPT.md > /tmp/prompt.md

    # Give to AI agent (pseudo-code)
    ai-agent refactor --prompt /tmp/prompt.md --crate $crate

    # Verify
    cd crates/$crate
    if cargo build && cargo test && cargo clippy -- -D warnings; then
        echo "✅ $crate refactored successfully"
        git add -A
        git commit -m "refactor($crate): Modern Rust 1.90+ patterns"
    else
        echo "❌ $crate refactoring failed"
        git restore .
    fi
    cd ../..
done
```

---

## Quick Reference Card

**Find bad patterns:**
```bash
rg "\.unwrap\(\)|\.expect\(|panic!|Box<dyn|&dyn|unsafe|OnceCell" src/
```

**Replace:**
- `unwrap/expect` → `?` or `match`
- `Box<dyn T>` → `<T: Trait>`
- `OnceCell` → `LazyLock`
- `if x.is_some()` → `if let Some(v) = x`
- Add `const` to functions where possible

**Verify:**
```bash
cargo build && cargo test && cargo clippy -- -D warnings
```

---

**END OF TEMPLATE**
