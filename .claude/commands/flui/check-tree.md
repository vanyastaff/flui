---
name: Check Tree Architecture
description: Verify three-tree architecture compliance (View-Element-Render)
---

Analyze the codebase to verify three-tree architecture compliance.

Check for:
1. **View Tree**: Views are immutable, implement `build()` returning `impl IntoElement`
2. **Element Tree**: Elements use ElementId with NonZeroUsize, proper lifecycle management
3. **Render Tree**: RenderBox uses correct arity types (Leaf, Single, Optional, Variable)

Search for violations:
- Views that mutate state directly (should use signals)
- Elements with incorrect parent/child relationships
- RenderObjects with wrong arity constraints

Report:
- Architecture compliance status
- Any violations found with file:line references
- Suggestions for fixes
