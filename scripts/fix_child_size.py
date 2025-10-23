#!/usr/bin/env python3
"""
Fix ctx.child_size() calls which don't exist.
"""

import re
import os

files_to_fix = [
    r"C:\Users\vanya\RustroverProjects\flui\crates\flui_rendering\src\objects\special\fitted_box.rs",
    r"C:\Users\vanya\RustroverProjects\flui\crates\flui_rendering\src\objects\layout\rotated_box.rs",
    r"C:\Users\vanya\RustroverProjects\flui\crates\flui_rendering\src\objects\layout\sized_overflow_box.rs",
    r"C:\Users\vanya\RustroverProjects\flui\crates\flui_rendering\src\objects\layout\overflow_box.rs",
    r"C:\Users\vanya\RustroverProjects\flui\crates\flui_rendering\src\objects\effects\offstage.rs",
    r"C:\Users\vanya\RustroverProjects\flui\crates\flui_rendering\src\objects\layout\align.rs",
]

def fix_child_size_in_paint(content):
    """Replace ctx.child_size() with accessing RenderObject size directly."""

    # Pattern: let child_size = ctx.child_size(child_id);
    # Replace with getting size from tree
    pattern = r'let child_size = ctx\.child_size\(child_id\);'
    replacement = '''// Get child size from tree
                let child_size = if let Some(child_elem) = ctx.tree().get(child_id) {
                    if let Some(child_ro) = child_elem.render_object() {
                        child_ro.size()
                    } else {
                        Size::ZERO
                    }
                } else {
                    Size::ZERO
                };'''

    content = re.sub(pattern, replacement, content)

    return content

def process_file(filepath):
    """Process a single file."""
    print(f"Processing {filepath}")

    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()

    original = content
    content = fix_child_size_in_paint(content)

    if content != original:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(content)
        print(f"  [FIXED] {filepath}")
        return True
    else:
        print(f"  [SKIP] {filepath}")
        return False

def main():
    """Main function."""
    fixed = 0
    for filepath in files_to_fix:
        if process_file(filepath):
            fixed += 1

    print(f"\nTotal files fixed: {fixed}")

if __name__ == '__main__':
    main()
