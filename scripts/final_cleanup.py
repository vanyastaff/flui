#!/usr/bin/env python3
"""
Final cleanup script to fix remaining issues.
"""

import re
import glob
import os

def final_cleanup(content):
    """Final cleanup of syntax issues."""

    # Fix: ctx.layout_child(child_id, ..., _ctx) -> ctx.layout_child(child_id, ...)
    content = re.sub(r'ctx\.layout_child\((.*?), _ctx\)', r'ctx.layout_child(\1)', content)

    # Fix: ctx.paint_child(child_id, ..., _ctx) -> ctx.paint_child(child_id, ...)
    content = re.sub(r'ctx\.paint_child\((.*?), _ctx\)', r'ctx.paint_child(\1)', content)

    # Fix: _ctx in function signatures that should be ctx
    content = re.sub(
        r'fn (layout|paint)\((.*?), _ctx: &flui_core::RenderContext\)',
        r'fn \1(\2, ctx: &flui_core::RenderContext)',
        content
    )

    return content

def process_file(filepath):
    """Process a single file."""
    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()

    original = content
    content = final_cleanup(content)

    if content != original:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(content)
        print(f"Fixed {filepath}")
        return True
    return False

def main():
    """Main function."""
    base_dir = r"C:\Users\vanya\RustroverProjects\flui\crates\flui_rendering\src\objects"

    fixed = 0
    for subdir in ['effects', 'layout', 'interaction', 'special']:
        pattern = os.path.join(base_dir, subdir, '*.rs')
        for filepath in glob.glob(pattern):
            if process_file(filepath):
                fixed += 1

    print(f"\nTotal files fixed: {fixed}")

if __name__ == '__main__':
    main()
