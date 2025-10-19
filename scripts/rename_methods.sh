#!/bin/bash
# Script to rename methods to Rust snake_case conventions

set -e

echo "🔥 Aggressive method renaming script"
echo "======================================"

# BuildContext method renames
echo "Renaming BuildContext methods..."

# Find all Rust files in flui_core, flui_widgets, flui_rendering
FILES=$(find crates/{flui_core,flui_widgets,flui_rendering,flui_app}/src -name "*.rs" 2>/dev/null || true)

if [ -z "$FILES" ]; then
    echo "No files found"
    exit 1
fi

# BuildContext renames
echo "  mark_needs_build -> mark_dirty"
echo "$FILES" | xargs sed -i 's/\.mark_needs_build()/.mark_dirty()/g'

echo "  visit_ancestor_elements -> walk_ancestors"
echo "$FILES" | xargs sed -i 's/\.visit_ancestor_elements(/.walk_ancestors(/g'

echo "  visit_child_elements -> walk_children"
echo "$FILES" | xargs sed -i 's/\.visit_child_elements(/.walk_children(/g'

echo "  find_ancestor_widget_of_type -> find_ancestor"
echo "$FILES" | xargs sed -i 's/\.find_ancestor_widget_of_type/\.find_ancestor/g'

echo "  find_ancestor_element_of_type -> find_ancestor_element"
echo "$FILES" | xargs sed -i 's/\.find_ancestor_element_of_type/\.find_ancestor_element/g'

echo "  find_ancestor_render_object_of_type -> find_ancestor_render"
echo "$FILES" | xargs sed -i 's/\.find_ancestor_render_object_of_type/\.find_ancestor_render/g'

echo "  get_element_for_inherited_widget_of_exact_type -> find_inherited_element"
echo "$FILES" | xargs sed -i 's/\.get_element_for_inherited_widget_of_exact_type/\.find_inherited_element/g'

echo "  depend_on_inherited_widget -> subscribe_to"
echo "$FILES" | xargs sed -i 's/\.depend_on_inherited_widget/\.subscribe_to/g'

echo "  get_inherited_widget -> find_inherited"
echo "$FILES" | xargs sed -i 's/\.get_inherited_widget/\.find_inherited/g'

echo "  find_render_object -> render_object (no change needed for property)"

# ElementTree renames
echo ""
echo "Renaming ElementTree methods..."

echo "  mount_root -> set_root"
echo "$FILES" | xargs sed -i 's/\.mount_root(/.set_root(/g'

echo "  mount_child -> insert_child"
echo "$FILES" | xargs sed -i 's/\.mount_child(/.insert_child(/g'

echo "  unmount_element -> remove"
echo "$FILES" | xargs sed -i 's/\.unmount_element(/.remove(/g'

echo "  update_element -> update"
# Careful - don't rename other update() calls
echo "$FILES" | xargs sed -i 's/tree\.update_element(/tree.update(/g'

echo "  mark_element_dirty -> mark_dirty"
echo "$FILES" | xargs sed -i 's/\.mark_element_dirty(/.mark_dirty(/g'

echo "  rebuild_dirty_elements -> rebuild"
echo "$FILES" | xargs sed -i 's/\.rebuild_dirty_elements()/.rebuild()/g'

echo "  get_element -> get"
echo "$FILES" | xargs sed -i 's/\.get_element(/.get(/g'

echo "  get_element_mut -> get_mut"
echo "$FILES" | xargs sed -i 's/\.get_element_mut(/.get_mut(/g'

echo "  has_dirty_elements -> has_dirty"
echo "$FILES" | xargs sed -i 's/\.has_dirty_elements()/.has_dirty()/g'

# Element renames
echo ""
echo "Renaming Element methods..."

echo "  visit_children -> walk_children"
echo "$FILES" | xargs sed -i 's/element\.visit_children(/element.walk_children(/g'

echo "  visit_children_mut -> walk_children_mut"
echo "$FILES" | xargs sed -i 's/\.visit_children_mut(/.walk_children_mut(/g'

echo "  child_ids -> children"
echo "$FILES" | xargs sed -i 's/\.child_ids()/.children()/g'

echo ""
echo "✅ Method renaming complete!"
echo "Next: Run 'cargo test' to verify"
