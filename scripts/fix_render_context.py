#!/usr/bin/env python3
"""
Script to automatically fix Single-child RenderObjects to use RenderContext pattern.
"""

import re
import os
import glob

# Pattern 1: Fix layout signature and simple layout implementation
def fix_layout_simple(content):
    """Fix simple layout implementations (pass-through constraints)."""
    pattern = r'''fn layout\(&self, constraints: BoxConstraints, _ctx: &flui_core::RenderContext\) -> Size \{
        // Store constraints
        \*self\.state\(\)\.constraints\.lock\(\) = Some\(constraints\);

        // Layout child with same constraints
        let size = if let Some\(child\) = self\.child\(\) \{
            child\.layout\(constraints, _ctx\)
        \} else \{
            (.*?)
        \};

        // Store size and clear needs_layout flag
        \*self\.state\(\)\.size\.lock\(\) = Some\(size\);
        self\.clear_needs_layout\(\);

        size
    \}'''

    replacement = r'''fn layout(&self, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *self.state().constraints.lock() = Some(constraints);

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Layout child with same constraints
        let size = if let Some(&child_id) = children_ids.first() {
            ctx.layout_child(child_id, constraints)
        } else {
            \1
        };

        // Store size and clear needs_layout flag
        *self.state().size.lock() = Some(size);
        self.clear_needs_layout();

        size
    }'''

    return re.sub(pattern, replacement, content, flags=re.DOTALL)

# Pattern 2: Fix simple paint implementation
def fix_paint_simple(content):
    """Fix simple paint implementations (direct pass-through)."""
    pattern = r'''fn paint\(&self, painter: &egui::Painter, offset: Offset, _ctx: &flui_core::RenderContext\) \{
        (.*?)if let Some\(child\) = self\.child\(\) \{
            child\.paint\(painter, offset, _ctx\);
        \}
    \}'''

    replacement = r'''fn paint(&self, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        \1// Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
        }
    }'''

    return re.sub(pattern, replacement, content, flags=re.DOTALL)

def process_file(filepath):
    """Process a single file."""
    print(f"Processing {filepath}")

    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()

    original_content = content

    # Apply fixes
    content = fix_layout_simple(content)
    content = fix_paint_simple(content)

    # Only write if content changed
    if content != original_content:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(content)
        print(f"  [FIXED] {filepath}")
        return True
    else:
        print(f"  [SKIP] No changes needed for {filepath}")
        return False

def main():
    """Main function."""
    base_dir = r"C:\Users\vanya\RustroverProjects\flui\crates\flui_rendering\src\objects"

    # Find all .rs files in effects, layout, interaction, special subdirectories
    subdirs = ['effects', 'layout', 'interaction', 'special']

    fixed_count = 0
    for subdir in subdirs:
        pattern_path = os.path.join(base_dir, subdir, '*.rs')
        files = glob.glob(pattern_path)

        for filepath in files:
            if process_file(filepath):
                fixed_count += 1

    print(f"\nTotal files fixed: {fixed_count}")

if __name__ == '__main__':
    main()
