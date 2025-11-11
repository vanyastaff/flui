#!/usr/bin/env python3
"""
Fix sliver layout() methods to return SliverGeometry instead of Size
"""

import re
from pathlib import Path

def fix_sliver_fill_remaining():
    """Fix sliver_fill_remaining.rs"""
    file_path = Path("crates/flui_rendering/src/objects/sliver/sliver_fill_remaining.rs")

    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Replace the layout method body
    old_layout = r'''    fn layout\(&mut self, ctx: &SliverLayoutContext\) -> SliverGeometry \{
        let constraints = &ctx\.constraints;

        let child_size = Size::new\(
            constraints\.max_width,
            constraints\.max_height,
        \);

        self\.child_size'''

    new_layout = '''    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        let constraints = &ctx.constraints;

        // Placeholder: Fill remaining viewport space
        let extent = constraints.remaining_paint_extent;

        SliverGeometry::simple(extent, extent)'''

    content = re.sub(old_layout, new_layout, content, flags=re.DOTALL)

    with open(file_path, 'w', encoding='utf-8') as f:
        f.write(content)

    print(f"[+] Fixed {file_path}")


def fix_sliver_fill_viewport():
    """Fix sliver_fill_viewport.rs"""
    file_path = Path("crates/flui_rendering/src/objects/sliver/sliver_fill_viewport.rs")

    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Replace Size::new with SliverGeometry
    content = re.sub(
        r'Size::new\(constraints\.max_width, constraints\.max_height\)',
        'SliverGeometry::simple(ctx.constraints.remaining_paint_extent, ctx.constraints.remaining_paint_extent)',
        content
    )

    with open(file_path, 'w', encoding='utf-8') as f:
        f.write(content)

    print(f"[+] Fixed {file_path}")


def fix_sliver_fixed_extent_list():
    """Fix sliver_fixed_extent_list.rs"""
    file_path = Path("crates/flui_rendering/src/objects/sliver/sliver_fixed_extent_list.rs")

    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Replace Size::new with SliverGeometry
    content = re.sub(
        r'Size::new\(constraints\.max_width, constraints\.max_height\)',
        'SliverGeometry::simple(0.0, 0.0)  // TODO: Implement proper fixed extent list layout',
        content
    )

    with open(file_path, 'w', encoding='utf-8') as f:
        f.write(content)

    print(f"[+] Fixed {file_path}")


def fix_sliver_grid():
    """Fix sliver_grid.rs"""
    file_path = Path("crates/flui_rendering/src/objects/sliver/sliver_grid.rs")

    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Replace Size::new with SliverGeometry
    content = re.sub(
        r'Size::new\(constraints\.max_width, constraints\.max_height\)',
        'SliverGeometry::simple(0.0, 0.0)  // TODO: Implement proper grid layout',
        content
    )

    with open(file_path, 'w', encoding='utf-8') as f:
        f.write(content)

    print(f"[+] Fixed {file_path}")


def fix_sliver_list():
    """Fix sliver_list.rs"""
    file_path = Path("crates/flui_rendering/src/objects/sliver/sliver_list.rs")

    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Fix cross_axis_extent assignment
    content = re.sub(
        r'self\.cross_axis_extent = constraints\.max_width;',
        'self.cross_axis_extent = ctx.constraints.cross_axis_extent;',
        content
    )

    # Replace Size::new with SliverGeometry
    content = re.sub(
        r'Size::new\(constraints\.max_width, constraints\.max_height\)',
        'SliverGeometry::simple(0.0, 0.0)  // TODO: Implement proper list layout',
        content
    )

    with open(file_path, 'w', encoding='utf-8') as f:
        f.write(content)

    print(f"[+] Fixed {file_path}")


def fix_sliver_prototype_extent_list():
    """Fix sliver_prototype_extent_list.rs"""
    file_path = Path("crates/flui_rendering/src/objects/sliver/sliver_prototype_extent_list.rs")

    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Replace Size::new with SliverGeometry
    content = re.sub(
        r'Size::new\(constraints\.max_width, constraints\.max_height\)',
        'SliverGeometry::simple(0.0, 0.0)  // TODO: Implement proper prototype extent list layout',
        content
    )

    with open(file_path, 'w', encoding='utf-8') as f:
        f.write(content)

    print(f"[+] Fixed {file_path}")


def fix_sliver_app_bar():
    """Fix sliver_app_bar.rs"""
    file_path = Path("crates/flui_rendering/src/objects/sliver/sliver_app_bar.rs")

    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()

    # Check if it needs fixing
    if 'Size::new(constraints.max_width' in content or 'constraints.max_width' in content:
        # Replace Size::new if present
        content = re.sub(
            r'Size::new\(constraints\.max_width, constraints\.max_height\)',
            'SliverGeometry::simple(self.expanded_height, self.collapsed_height.min(ctx.constraints.remaining_paint_extent))',
            content
        )

        with open(file_path, 'w', encoding='utf-8') as f:
            f.write(content)

        print(f"[+] Fixed {file_path}")
    else:
        print(f"[-] {file_path} already OK or needs manual review")


def main():
    print("Fixing sliver layout methods...")
    print("=" * 60)

    fix_sliver_fill_remaining()
    fix_sliver_fill_viewport()
    fix_sliver_fixed_extent_list()
    fix_sliver_grid()
    fix_sliver_list()
    fix_sliver_prototype_extent_list()
    fix_sliver_app_bar()

    print("=" * 60)
    print("Done! Now try: cargo build -p flui_rendering")


if __name__ == "__main__":
    main()
