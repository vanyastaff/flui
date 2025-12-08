---
name: Profile
description: Profile FLUI performance and identify bottlenecks
---

Profile the specified target: **$ARGUMENTS**

Steps:

1. **Build with profiling**:
```bash
cargo build --release --example <target>
```

2. **Run with timing**:
```bash
RUST_LOG=info cargo run --release --example <target>
```

3. **Analyze output**:
- Look for slow spans in tracing-forest output
- Identify layout/paint phase bottlenecks
- Check for excessive rebuilds

4. **Memory analysis** (if available):
```bash
cargo +nightly build --example <target> -Z build-std
```

Focus areas:
- Build phase timing
- Layout phase timing  
- Paint phase timing
- Signal updates frequency
