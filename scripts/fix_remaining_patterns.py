#!/usr/bin/env python3
"""
Script to fix remaining complex patterns in RenderObjects.
"""

import re
import os
import glob

def fix_all_patterns(content):
    """Fix all remaining self.child() patterns."""

    # Pattern 1: if let Some(child) = self.child() { child.layout(...) }
    content = re.sub(
        r'if let Some\(child\) = self\.child\(\) \{\s*child\.layout\((.*?), _ctx\);',
        r'let children_ids = ctx.children();\n        if let Some(&child_id) = children_ids.first() {\n            ctx.layout_child(child_id, \1);',
        content
    )

    # Pattern 2: if let Some(child) = self.child() { child.paint(...) }
    content = re.sub(
        r'if let Some\(child\) = self\.child\(\) \{\s*child\.paint\((.*?), _ctx\);',
        r'let children_ids = ctx.children();\n        if let Some(&child_id) = children_ids.first() {\n            ctx.paint_child(child_id, \1);',
        content
    )

    # Pattern 3: let _ = child.layout(...)
    content = re.sub(
        r'let _ = child\.layout\((.*?), _ctx\);',
        r'let _ = ctx.layout_child(child_id, \1);',
        content
    )

    # Pattern 4: child.paint(...) standalone
    content = re.sub(
        r'\bchild\.paint\((.*?), _ctx\);',
        r'ctx.paint_child(child_id, \1);',
        content
    )

    # Pattern 5: child.size()
    content = re.sub(
        r'\bchild\.size\(\)',
        r'ctx.child_size(child_id)',
        content
    )

    # Pattern 6: Change _ctx to ctx in function signatures
    content = re.sub(
        r'fn layout\(&self, constraints: BoxConstraints, _ctx: &flui_core::RenderContext\)',
        r'fn layout(&self, constraints: BoxConstraints, ctx: &flui_core::RenderContext)',
        content
    )

    content = re.sub(
        r'fn paint\(&self, painter: &egui::Painter, offset: Offset, _ctx: &flui_core::RenderContext\)',
        r'fn paint(&self, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext)',
        content
    )

    # Pattern 7: Handle cases where child is used multiple times
    # if let Some(child) = self.child() { ... multiple uses ... }
    # This is complex, so we'll do a simple substitution
    content = re.sub(
        r'if let Some\(child\) = self\.child\(\) \{',
        r'let children_ids = ctx.children();\n        if let Some(&child_id) = children_ids.first() {',
        content
    )

    return content

def process_file(filepath):
    """Process a single file."""
    print(f"Processing {filepath}")

    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()

    # Skip if no self.child() calls
    if 'self.child()' not in content:
        print(f"  [SKIP] No self.child() in {filepath}")
        return False

    original_content = content

    # Apply fixes
    content = fix_all_patterns(content)

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
