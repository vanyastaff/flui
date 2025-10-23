#!/usr/bin/env python3
"""
Script to update DynRenderObject implementations to use RenderContext parameter.

This script performs the following transformations:
1. Changes `fn layout(&mut self, constraints: BoxConstraints)`
   to `fn layout(&mut self, constraints: BoxConstraints, ctx: &flui_core::RenderContext)`

2. Changes `fn paint(&self, painter: &egui::Painter, offset: Offset)`
   to `fn paint(&self, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext)`

3. For leaf RenderObjects (no children), adds `_ctx` parameter (unused)
4. For single-child/multi-child RenderObjects, updates child access to use ElementTree via ctx
"""

import re
import sys
from pathlib import Path

def update_layout_signature(content):
    """Update layout method signature"""
    # Pattern: fn layout(&mut self, constraints: BoxConstraints) -> Size
    pattern = r'fn layout\(&mut self, constraints: BoxConstraints\) -> Size'
    replacement = r'fn layout(&mut self, constraints: BoxConstraints, _ctx: &flui_core::RenderContext) -> Size'
    return re.sub(pattern, replacement, content)

def update_paint_signature(content):
    """Update paint method signature"""
    # Pattern: fn paint(&self, painter: &egui::Painter, offset: Offset)
    pattern = r'fn paint\(&self, painter: &egui::Painter, offset: Offset\)'
    replacement = r'fn paint(&self, painter: &egui::Painter, offset: Offset, _ctx: &flui_core::RenderContext)'
    return re.sub(pattern, replacement, content)

def update_child_layout_calls(content):
    """Update child.layout() calls to child.layout(constraints, ctx)"""
    # Pattern: child.layout(constraints) or child.layout(child_constraints)
    pattern = r'child\.layout\(([^)]+)\)'
    replacement = r'child.layout(\1, ctx)'
    return re.sub(pattern, replacement, content)

def update_child_paint_calls(content):
    """Update child.paint() calls to child.paint(painter, offset, ctx)"""
    # Pattern: child.paint(painter, offset) or child.paint(painter, child_offset)
    pattern = r'child\.paint\(painter, ([^)]+)\)'
    replacement = r'child.paint(painter, \1, ctx)'
    return re.sub(pattern, replacement, content)

def process_file(file_path):
    """Process a single Rust file"""
    print(f"Processing {file_path}...")

    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    original_content = content

    # Apply transformations
    content = update_layout_signature(content)
    content = update_paint_signature(content)
    content = update_child_layout_calls(content)
    content = update_child_paint_calls(content)

    # Rename _ctx to ctx if it's actually used (has child access)
    if 'child.' in content and '_ctx' in content:
        content = content.replace('_ctx: &flui_core::RenderContext', 'ctx: &flui_core::RenderContext')

    if content != original_content:
        with open(file_path, 'w', encoding='utf-8') as f:
            f.write(content)
        print(f"  ✓ Updated {file_path}")
        return True
    else:
        print(f"  - No changes needed for {file_path}")
        return False

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print("Usage: python update_render_context.py <file1> <file2> ...")
        sys.exit(1)

    updated_count = 0
    for file_path_str in sys.argv[1:]:
        file_path = Path(file_path_str)
        if file_path.exists():
            if process_file(file_path):
                updated_count += 1
        else:
            print(f"Warning: {file_path} does not exist")

    print(f"\nTotal files updated: {updated_count}/{len(sys.argv) - 1}")
