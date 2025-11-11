#!/usr/bin/env python3
"""
Migrate sliver files from Render trait to RenderSliver trait
"""

import re
import sys
from pathlib import Path

def migrate_file(file_path):
    """Migrate a single file to use RenderSliver trait"""
    print(f"Processing: {file_path}")

    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    original_content = content

    # Step 1: Update imports
    # Replace: use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
    # With: use flui_core::render::{Arity, RenderSliver, SliverLayoutContext, SliverPaintContext};
    content = re.sub(
        r'use flui_core::render::\{([^}]*\b)Render(\b[^}]*)\};',
        r'use flui_core::render::{\1RenderSliver\2};',
        content
    )
    content = re.sub(
        r'(\buse flui_core::render::\{[^}]*)LayoutContext',
        r'\1SliverLayoutContext',
        content
    )
    content = re.sub(
        r'(\buse flui_core::render::\{[^}]*)PaintContext',
        r'\1SliverPaintContext',
        content
    )

    # Step 2: Replace trait implementation
    # Replace: impl Render for RenderSliverXxx {
    # With: impl RenderSliver for RenderSliverXxx {
    content = re.sub(
        r'\bimpl Render for (RenderSliver\w+)',
        r'impl RenderSliver for \1',
        content
    )

    # Step 3: Update layout method signature
    # Replace: fn layout(&mut self, ctx: &LayoutContext) -> Size {
    # With: fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
    content = re.sub(
        r'fn layout\(&mut self, ctx: &LayoutContext\) -> Size',
        r'fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry',
        content
    )

    # Step 4: Update paint method signature
    # Replace: fn paint(&self, ctx: &PaintContext) -> Canvas {
    # With: fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
    content = re.sub(
        r'fn paint\(&self, ctx: &PaintContext\) -> Canvas',
        r'fn paint(&self, ctx: &SliverPaintContext) -> Canvas',
        content
    )

    if content != original_content:
        with open(file_path, 'w', encoding='utf-8') as f:
            f.write(content)
        print(f"  [+] Updated")
        return True
    else:
        print(f"  [-] No changes needed")
        return False

def main():
    sliver_dir = Path("crates/flui_rendering/src/objects/sliver")

    # Files to migrate (those still using Render trait)
    files_to_migrate = [
        "sliver_app_bar.rs",
        "sliver_fill_remaining.rs",
        "sliver_fill_viewport.rs",
        "sliver_fixed_extent_list.rs",
        "sliver_grid.rs",
        "sliver_list.rs",
        "sliver_persistent_header.rs",
        "sliver_prototype_extent_list.rs",
        "viewport.rs",
    ]

    updated_count = 0
    for filename in files_to_migrate:
        file_path = sliver_dir / filename
        if file_path.exists():
            if migrate_file(file_path):
                updated_count += 1
        else:
            print(f"Warning: {file_path} not found")

    print(f"\n{'='*60}")
    print(f"Migration complete! Updated {updated_count} files.")
    print(f"{'='*60}")
    print("\nNext steps:")
    print("1. Review the changes with: git diff")
    print("2. Build the project: cargo build -p flui_rendering")
    print("3. Fix any compilation errors")

if __name__ == "__main__":
    main()
