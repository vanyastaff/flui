# FLUI Documentation

Complete documentation for FLUI framework architecture and refactoring.

---

## 📚 Quick Navigation

### For AI Agents (Autonomous Refactoring)

| Document | Use Case | Size | Time |
|----------|----------|------|------|
| **[QUICK_REFACTOR.md](QUICK_REFACTOR.md)** | Quick single-crate refactor | 1 page | 5 min |
| **[CRATE_REFACTOR_PROMPT.md](CRATE_REFACTOR_PROMPT.md)** | Thorough crate refactor | 10 pages | 15 min |
| **[AI_AGENT_PROMPTS.md](AI_AGENT_PROMPTS.md)** | Multi-crate architecture | 25 pages | 3-4 weeks |
| **[PROMPTS_GUIDE.md](PROMPTS_GUIDE.md)** | How to use prompts | 10 pages | 10 min |

### For Humans (Understanding & Planning)

| Document | Purpose |
|----------|---------|
| **[GENERIC_REFACTORING_ROADMAP.md](GENERIC_REFACTORING_ROADMAP.md)** | Complete refactoring roadmap |
| **[GENERIC_REFACTORING_CHECKLIST.md](GENERIC_REFACTORING_CHECKLIST.md)** | Quick checklist reference |
| **[SUMMARY.md](SUMMARY.md)** | Research summary & current state |

### Architecture Documentation

| Document | Focus |
|----------|-------|
| **[arch/CORE_ARCHITECTURE.md](arch/CORE_ARCHITECTURE.md)** | Core framework design |
| **[arch/RENDERING_ARCHITECTURE.md](arch/RENDERING_ARCHITECTURE.md)** | Rendering system |
| **[arch/PATTERNS.md](arch/PATTERNS.md)** | Common patterns |

---

## 🚀 Quick Start

### I want to refactor ONE crate quickly:

```bash
# Use the ultra-short prompt
cat docs/QUICK_REFACTOR.md | sed 's/{CRATE}/flui-tree/g'
# Give to AI agent or execute yourself
```

### I want to thoroughly refactor ONE crate:

```bash
# Use the comprehensive template
cat docs/CRATE_REFACTOR_PROMPT.md | sed 's/{CRATE_NAME}/flui_rendering/g'
# Give to AI agent
```

### I want to refactor ENTIRE architecture:

```bash
# Read the roadmap first
cat docs/GENERIC_REFACTORING_ROADMAP.md

# Then use phase-by-phase prompts
cat docs/AI_AGENT_PROMPTS.md | sed -n '/^## Phase 1:/,/^## Phase 2:/p'
# Execute Phase 1, then Phase 2, etc.
```

### I'm confused about which prompt to use:

```bash
# Read the guide
cat docs/PROMPTS_GUIDE.md
```

---

## 📖 Document Descriptions

### AI Agent Prompts

#### QUICK_REFACTOR.md
**Purpose:** One-page prompt for fast refactoring
**Target:** AI agents or experienced developers
**Contains:**
- 5 core rules (Generic > dyn, Safe > unwrap, etc.)
- Quick find/fix patterns
- Verification commands
- Minimal but complete

**When to use:**
- ✅ Need quick results
- ✅ Single crate
- ✅ Simple refactoring

---

#### CRATE_REFACTOR_PROMPT.md
**Purpose:** Universal template for any crate
**Target:** AI agents or thorough refactoring
**Contains:**
- Complete principles & guidelines
- Step-by-step process
- Anti-patterns to remove
- Modern Rust 1.90+ idioms
- Before/after examples
- Acceptance criteria

**When to use:**
- ✅ Thorough refactoring needed
- ✅ Learning modern patterns
- ✅ Quality > speed

---

#### AI_AGENT_PROMPTS.md
**Purpose:** Complete phase-by-phase execution plan
**Target:** AI agents for multi-crate refactoring
**Contains:**
- System prompt (foundation)
- Phase 1: flui-tree const generics
- Phase 2: flui_rendering generics
- Phase 3: flui_core TreeCoordinator
- Complete code examples
- Tests for each phase
- Verification prompts
- Error recovery

**When to use:**
- ✅ Architectural changes
- ✅ Multi-crate coordination
- ✅ Long-term roadmap

---

#### PROMPTS_GUIDE.md
**Purpose:** Master guide for using all prompts
**Target:** Anyone using the prompt system
**Contains:**
- Comparison of 3 prompt levels
- Usage patterns
- Complete workflows
- Verification checklists
- Learning path
- Example scripts

**When to use:**
- ✅ First time using prompts
- ✅ Choosing right prompt
- ✅ Understanding the system

---

### Roadmap Documents

#### GENERIC_REFACTORING_ROADMAP.md
**Purpose:** Complete technical roadmap
**Contains:**
- 6 phases of work
- Technical specifications
- Code examples
- Success criteria
- Timeline (3-4 weeks)
- Rust 1.90+ features guide

**Audience:** Technical leads, architects

---

#### GENERIC_REFACTORING_CHECKLIST.md
**Purpose:** Quick reference checklist
**Contains:**
- Checkbox format
- Quick commands
- Verification steps
- Success criteria

**Audience:** Developers tracking progress

---

#### SUMMARY.md
**Purpose:** Research summary
**Contains:**
- Current state analysis
- Critical issues
- Roadmap decisions
- What's done vs. what's needed

**Audience:** Anyone joining the project

---

## 🎯 Common Workflows

### Workflow 1: Quick Win

```bash
# Pick a small crate
CRATE="flui-foundation"

# Generate quick prompt
cat docs/QUICK_REFACTOR.md | sed "s/{CRATE}/$CRATE/g"

# Execute (give to AI or do yourself)
# Verify
cd crates/$CRATE && cargo build && cargo test

# Commit
git commit -m "refactor($CRATE): Modern patterns"
```

---

### Workflow 2: Thorough Refactor

```bash
# Pick any crate
CRATE="flui_rendering"

# Generate comprehensive prompt
cat docs/CRATE_REFACTOR_PROMPT.md | sed "s/{CRATE_NAME}/$CRATE/g" > /tmp/refactor.md

# Give to AI agent
cat /tmp/refactor.md

# AI executes, you verify
cd crates/$CRATE
cargo build --release && cargo test --release
cargo clippy -- -D warnings
cargo bench

# Check results
rg "\.unwrap\(\)" src/  # Should be empty

# Commit with details
git commit -m "refactor($CRATE): Comprehensive modernization

- Generic instead of dyn
- Removed all unwrap/expect
- Modern Rust 1.90+ features
- Performance improved by 12%
"
```

---

### Workflow 3: Full Architecture

```bash
# Phase 1: flui-tree
cat docs/AI_AGENT_PROMPTS.md | sed -n '/^## Phase 1:/,/^## Phase 2:/p' > /tmp/phase1.md
# Execute phase 1
# Commit

# Phase 2: flui_rendering
cat docs/AI_AGENT_PROMPTS.md | sed -n '/^## Phase 2:/,/^## Phase 3:/p' > /tmp/phase2.md
# Execute phase 2
# Commit

# Continue through all phases...
```

---

## 🔍 Finding Information

### "How do I refactor a crate?"
→ Read `PROMPTS_GUIDE.md`

### "What needs to be done overall?"
→ Read `GENERIC_REFACTORING_ROADMAP.md`

### "Quick checklist for tracking?"
→ Read `GENERIC_REFACTORING_CHECKLIST.md`

### "What's the current state?"
→ Read `SUMMARY.md`

### "Complete autonomous AI execution?"
→ Use `AI_AGENT_PROMPTS.md`

### "Fast single-crate refactor?"
→ Use `QUICK_REFACTOR.md`

### "Thorough single-crate refactor?"
→ Use `CRATE_REFACTOR_PROMPT.md`

---

## 📊 Document Relationship

```
PROMPTS_GUIDE.md (START HERE!)
    ├─→ QUICK_REFACTOR.md (Fast path)
    ├─→ CRATE_REFACTOR_PROMPT.md (Thorough path)
    └─→ AI_AGENT_PROMPTS.md (Architecture path)
            └─→ GENERIC_REFACTORING_ROADMAP.md (Details)
                    └─→ GENERIC_REFACTORING_CHECKLIST.md (Tracking)
                            └─→ SUMMARY.md (Context)
```

**Reading Order:**
1. **PROMPTS_GUIDE.md** - Understand the system
2. Pick your path:
   - Fast → **QUICK_REFACTOR.md**
   - Thorough → **CRATE_REFACTOR_PROMPT.md**
   - Architecture → **AI_AGENT_PROMPTS.md** + **ROADMAP**
3. Track progress with **CHECKLIST.md**
4. Reference **SUMMARY.md** for context

---

## 🎓 Learning Path

### Beginner (First Day)
1. Read this README
2. Read `PROMPTS_GUIDE.md`
3. Try `QUICK_REFACTOR.md` on `flui-foundation`
4. Verify results, learn patterns

### Intermediate (First Week)
1. Read `CRATE_REFACTOR_PROMPT.md`
2. Refactor `flui-tree` using template
3. Study anti-patterns section
4. Read `GENERIC_REFACTORING_CHECKLIST.md`

### Advanced (First Month)
1. Read `GENERIC_REFACTORING_ROADMAP.md`
2. Read `AI_AGENT_PROMPTS.md`
3. Execute Phase 1 on flui-tree
4. Understand architectural decisions
5. Complete all phases

---

## ✅ Success Criteria

After using any prompt, verify:

```bash
# Build
✓ cargo build
✓ cargo build --release

# Test
✓ cargo test

# Quality
✓ cargo clippy -- -D warnings
✓ cargo fmt -- --check

# No bad patterns
✓ rg "\.unwrap\(\)" src/ | wc -l  # = 0
✓ rg "\.expect\(" src/ | wc -l   # = 0
✓ rg "panic!\(" src/ | wc -l     # = 0

# Performance
✓ cargo bench (no regressions)
✓ cargo bloat --release -n 20 (no size increase)
```

---

## 🚀 Getting Started

**First time here?**

```bash
# 1. Read the guide
cat docs/PROMPTS_GUIDE.md

# 2. Pick the quick prompt
cat docs/QUICK_REFACTOR.md

# 3. Try it on a small crate
cat docs/QUICK_REFACTOR.md | sed 's/{CRATE}/flui-foundation/g'

# 4. Execute and learn

# 5. Graduate to comprehensive prompts

# 6. Eventually tackle full architecture
```

---

**Questions?** Check the [PROMPTS_GUIDE.md](PROMPTS_GUIDE.md) first!

**Happy Refactoring! 🦀✨**
