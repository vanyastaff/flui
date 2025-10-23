#!/usr/bin/env python3
"""
Script to fix syntax errors caused by the previous script.
"""

import re
import glob
import os

def fix_syntax_error(content):
    """Fix the syntax error: let size = let children_ids = ..."""

    # Pattern: let size = let children_ids = ctx.children();
    # Should be: let children_ids = ctx.children();\n        let size =
    pattern = r'let (size|child_size) = let children_ids = ctx\.children\(\);'
    replacement = r'let children_ids = ctx.children();\n        let \1 ='

    content = re.sub(pattern, replacement, content)

    # Also fix: child.layout/child.paint that wasn't replaced
    content = re.sub(r'\bchild\.layout\(', r'ctx.layout_child(child_id, ', content)
    content = re.sub(r'\bchild\.paint\(', r'ctx.paint_child(child_id, ', content)

    return content

def process_file(filepath):
    """Process a single file."""
    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()

    original = content
    content = fix_syntax_error(content)

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
