# Quick Refactor Prompt (One-Page)

Copy-paste this for any crate. Replace `{CRATE}` with crate name.

---

```markdown
Refactor `crates/{CRATE}` to modern Rust 1.90+ standards:

## Rules
1. **Generic > dyn**: Replace `Box<dyn T>` with `<T: Trait>`
2. **Safe > unwrap**: Replace `.unwrap()/.expect()` with `?` or `match`
3. **Modern features**: Use LazyLock, const fn, const generics, GATs
4. **Zero unsafe**: Remove OR document with SAFETY comments
5. **Strong types**: Enums > strings, newtype pattern

## Find Bad Code
```bash
cd crates/{CRATE}
rg "\.unwrap\(\)|\.expect\(|panic!|Box<dyn|unsafe|OnceCell" src/
```

## Fix Patterns

### unwrap → Result
```rust
// Before
fn get(&self) -> T { self.val.unwrap() }
// After
fn get(&self) -> Option<T> { self.val.clone() }
```

### dyn → Generic
```rust
// Before
struct S { items: Vec<Box<dyn Item>> }
// After
struct S<T: Item> { items: Vec<T> }
```

### OnceCell → LazyLock
```rust
// Before
use once_cell::sync::OnceCell;
// After (1.80+)
use std::sync::LazyLock;
static X: LazyLock<T> = LazyLock::new(|| ...);
```

### Const where possible
```rust
// Before
fn new() -> Self { ... }
// After
const fn new() -> Self { ... }
```

### if x.is_some() → if let
```rust
// Before
if x.is_some() { x.unwrap() }
// After
if let Some(v) = x { v }
```

## Verify
```bash
cargo build && cargo test && cargo clippy -- -D warnings
rg "\.unwrap\(\)" src/  # Should be empty
```

## Output
Report:
- Changes made (e.g., "Removed 10 unwraps, 3 dyn → generic")
- Performance impact
- Remaining issues (if any)
```

---

**Example Usage:**

```bash
# For flui-tree
cat QUICK_REFACTOR.md | sed 's/{CRATE}/flui-tree/g'

# For flui_rendering
cat QUICK_REFACTOR.md | sed 's/{CRATE}/flui_rendering/g'
```
