# AI Agent Prompts Guide

Complete guide to all available AI prompts for FLUI refactoring.

---

## 📚 Available Prompts (3 Levels)

### 🚀 Level 1: Quick Refactor (Ultra-Short)
**File:** `docs/QUICK_REFACTOR.md`
**Size:** ~50 lines (one page)
**Use When:** Quick refactor of single crate
**Time:** 5 minutes to read + execute

```bash
# Usage
cat docs/QUICK_REFACTOR.md | sed 's/{CRATE}/flui-tree/g'
# Copy output → Give to AI agent
```

**Content:**
- 5 core rules
- Find bad code commands
- Fix patterns (unwrap → ?, dyn → generic, etc.)
- Verification commands
- Output format

**Best For:**
- ✅ Quick wins
- ✅ Single crate refactor
- ✅ Newcomers to the project

---

### 📖 Level 2: Universal Template (Comprehensive)
**File:** `docs/CRATE_REFACTOR_PROMPT.md`
**Size:** ~400 lines (detailed)
**Use When:** Thorough refactor with full context
**Time:** 15 minutes to read + execute

```bash
# Usage
cat docs/CRATE_REFACTOR_PROMPT.md | sed 's/{CRATE_NAME}/flui_rendering/g'
# Copy output → Give to AI agent
```

**Content:**
- Complete principles & guidelines
- Step-by-step refactoring process
- Anti-patterns to remove
- Modern Rust idioms (1.90+)
- Acceptance criteria
- Example before/after code
- Performance verification

**Best For:**
- ✅ Thorough refactoring
- ✅ Learning modern patterns
- ✅ Quality assurance

---

### 🎯 Level 3: Phase-by-Phase (Full Roadmap)
**File:** `docs/AI_AGENT_PROMPTS.md`
**Size:** ~1000 lines (complete guide)
**Use When:** Multi-crate architectural refactoring
**Time:** Full project timeline (3-4 weeks)

```bash
# Usage - Extract specific phase
cat docs/AI_AGENT_PROMPTS.md | sed -n '/^## Phase 1:/,/^## Phase 2:/p'
# Give to AI agent
```

**Content:**
- System prompt (foundation)
- Phase 1: flui-tree const generics
- Phase 2: flui_rendering generics
- Phase 3: flui_core TreeCoordinator
- Verification prompts
- Error recovery prompts
- Complete code examples for each phase

**Best For:**
- ✅ Architectural changes
- ✅ Multi-crate coordination
- ✅ Long-term roadmap execution

---

## 🎪 Comparison Matrix

| Feature | Quick | Universal | Phase-by-Phase |
|---------|-------|-----------|----------------|
| Length | 1 page | 10 pages | 25 pages |
| Time | 5 min | 15 min | 3-4 weeks |
| Scope | 1 crate | 1 crate | All crates |
| Detail | Minimal | Full | Complete |
| Examples | Basic | Extensive | With tests |
| Verification | Simple | Detailed | Comprehensive |
| Use Case | Quick fix | Refactor | Architecture |

---

## 🛠️ Usage Patterns

### Pattern 1: Single Crate Quick Fix

```bash
# 1. Pick the quick prompt
cat docs/QUICK_REFACTOR.md | sed 's/{CRATE}/flui-tree/g' > /tmp/prompt.txt

# 2. Give to AI agent
cat /tmp/prompt.txt
# AI agent reads and executes

# 3. Verify
cd crates/flui-tree
cargo build && cargo test && cargo clippy -- -D warnings

# 4. Check for bad patterns
rg "\.unwrap\(\)" src/  # Should be empty

# 5. Commit
git add -A && git commit -m "refactor(flui-tree): Remove unwrap, use generics"
```

**Expected Output:**
```
Refactored flui-tree:
- Removed 8 .unwrap() calls
- Replaced 2 Box<dyn Trait> with generics
- Changed OnceCell to LazyLock
- Binary size: 145KB → 138KB
```

---

### Pattern 2: Thorough Crate Refactor

```bash
# 1. Use comprehensive template
cat docs/CRATE_REFACTOR_PROMPT.md | sed 's/{CRATE_NAME}/flui_rendering/g' > /tmp/refactor.md

# 2. Give to AI agent
cat /tmp/refactor.md
# AI agent executes all steps

# 3. Verify thoroughly
cd crates/flui_rendering
cargo build --release
cargo test --release
cargo clippy -- -D warnings
cargo bench  # Performance check

# 4. Verify no bad patterns
rg "\.unwrap\(\)|\.expect\(|panic!|Box<dyn" src/

# 5. Check assembly (zero-cost check)
cargo asm flui_rendering::RenderElement::new

# 6. Commit with details
git add -A
git commit -m "refactor(flui_rendering): Modern Rust 1.90+ patterns

- Generic RenderElement<R, P> instead of Box<dyn>
- Removed all unwrap/expect (15 instances)
- LazyLock instead of OnceCell
- Added const fn where possible
- Zero unsafe blocks remaining

Performance: 210µs → 185µs (12% faster)
Binary size: 425KB → 398KB
"
```

---

### Pattern 3: Multi-Crate Architectural Refactor

```bash
# 1. Start with Phase 1 (flui-tree)
cat docs/AI_AGENT_PROMPTS.md | sed -n '/^## Phase 1:/,/^## Phase 2:/p' > /tmp/phase1.md

# 2. AI agent executes Phase 1
cat /tmp/phase1.md
# Agent implements const generics for Arity

# 3. Verify & commit
cd crates/flui-tree
cargo build && cargo test
git commit -m "phase 1: flui-tree const generics"

# 4. Move to Phase 2 (flui_rendering)
cat docs/AI_AGENT_PROMPTS.md | sed -n '/^## Phase 2:/,/^## Phase 3:/p' > /tmp/phase2.md

# 5. AI agent executes Phase 2
cat /tmp/phase2.md
# Agent adds Arity to RenderElement<R, P, A>

# 6. Verify & commit
cd crates/flui_rendering
cargo build && cargo test
git commit -m "phase 2: flui_rendering generic RenderElement"

# Continue through all phases...
```

---

## 🔍 Choosing the Right Prompt

### Use **QUICK_REFACTOR.md** if:
- ✅ You want fast results
- ✅ Single crate needs cleanup
- ✅ Simple refactoring (unwrap → ?, dyn → generic)
- ✅ Time-constrained

### Use **CRATE_REFACTOR_PROMPT.md** if:
- ✅ You want thorough refactoring
- ✅ Learning modern patterns
- ✅ Quality is more important than speed
- ✅ Need detailed verification

### Use **AI_AGENT_PROMPTS.md** if:
- ✅ Architectural changes needed
- ✅ Multi-crate coordination required
- ✅ Long-term roadmap execution
- ✅ Need complete code examples with tests

---

## 📊 Example Workflow: Refactor All Crates

```bash
#!/bin/bash
# refactor_all_crates.sh

CRATES=(
    "flui-foundation"
    "flui-tree"
    "flui_types"
    "flui_rendering"
    "flui_core"
    "flui_widgets"
)

for crate in "${CRATES[@]}"; do
    echo "=== Refactoring $crate ==="

    # Generate prompt (using comprehensive template)
    cat docs/CRATE_REFACTOR_PROMPT.md | sed "s/{CRATE_NAME}/$crate/g" > /tmp/refactor_$crate.md

    # AI agent executes (pseudo-code)
    echo "Give this prompt to AI agent:"
    echo "/tmp/refactor_$crate.md"
    read -p "Press enter when agent finishes..."

    # Verify
    cd crates/$crate || continue

    if cargo build && cargo test && cargo clippy -- -D warnings; then
        echo "✅ $crate refactored successfully"

        # Check for bad patterns
        if rg "\.unwrap\(\)" src/; then
            echo "⚠️  Warning: unwrap() still found in $crate"
        fi

        # Commit
        git add -A
        git commit -m "refactor($crate): Modern Rust 1.90+ patterns"
    else
        echo "❌ $crate refactoring failed"
        git restore .
    fi

    cd ../..
done

echo "=== All crates refactored! ==="
```

---

## 🎓 Learning Path

### Beginner
1. Read `QUICK_REFACTOR.md`
2. Practice on `flui-foundation` (small crate)
3. Verify results, learn patterns

### Intermediate
1. Read `CRATE_REFACTOR_PROMPT.md`
2. Refactor `flui-tree` using template
3. Study anti-patterns section

### Advanced
1. Read `AI_AGENT_PROMPTS.md`
2. Execute Phase 1 (flui-tree const generics)
3. Understand architectural decisions
4. Execute remaining phases

---

## 📝 Prompt Customization

### Add Project-Specific Rules

Edit the template and add:
```markdown
## Project-Specific Rules

### FLUI Naming Conventions
- RenderObject types: `Render` prefix (e.g., `RenderPadding`)
- Protocol types: `Protocol` suffix (e.g., `BoxProtocol`)
- Arity types: Descriptive (e.g., `Single`, `Variable`)

### FLUI Error Handling
- Use `RenderError` for rendering errors
- Use `FrameworkError` for framework errors
- Never panic in release builds

### FLUI Performance
- No allocations in hot paths
- Use `parking_lot` instead of `std::sync`
- Prefer `&[T]` over `Vec<T>` for returns
```

---

## ✅ Verification Checklist

After using ANY prompt, verify:

```bash
cd crates/{CRATE}

# Build
✓ cargo build
✓ cargo build --release

# Test
✓ cargo test
✓ cargo test --release

# Quality
✓ cargo clippy -- -D warnings
✓ cargo fmt -- --check

# Bad patterns check
✓ rg "\.unwrap\(\)" src/ | wc -l  # Should be 0
✓ rg "\.expect\(" src/ | wc -l   # Should be 0
✓ rg "panic!\(" src/ | wc -l     # Should be 0

# Dependencies
✓ cargo tree -p {CRATE} --depth 1  # Minimal deps?

# Performance (if applicable)
✓ cargo bench
✓ cargo bloat --release -n 20
```

---

## 🚀 Quick Start

**First time? Start here:**

```bash
# 1. Pick a small crate
export CRATE="flui-foundation"

# 2. Use quick prompt
cat docs/QUICK_REFACTOR.md | sed "s/{CRATE}/$CRATE/g" > /tmp/prompt.txt

# 3. Read the prompt
cat /tmp/prompt.txt

# 4. Give to AI agent (you or Claude/GPT)
# Agent executes the refactoring

# 5. Verify
cd crates/$CRATE
cargo build && cargo test && cargo clippy -- -D warnings

# 6. Commit
git add -A && git commit -m "refactor($CRATE): Modern patterns"

# 7. Celebrate! 🎉
```

---

## 📞 Need Help?

- **Short prompt not enough?** → Use comprehensive template
- **Template not enough?** → Use phase-by-phase prompts
- **Phase prompts not enough?** → Check `GENERIC_REFACTORING_ROADMAP.md`
- **Roadmap not clear?** → Check `GENERIC_REFACTORING_CHECKLIST.md`

**All documents:**
```
docs/
├── QUICK_REFACTOR.md                    # 🚀 Ultra-short (1 page)
├── CRATE_REFACTOR_PROMPT.md             # 📖 Comprehensive template
├── AI_AGENT_PROMPTS.md                  # 🎯 Phase-by-phase
├── GENERIC_REFACTORING_ROADMAP.md       # 📚 Full roadmap
├── GENERIC_REFACTORING_CHECKLIST.md     # ✅ Checklist
└── PROMPTS_GUIDE.md                     # 📖 This file
```

---

**Happy Refactoring! 🦀✨**
