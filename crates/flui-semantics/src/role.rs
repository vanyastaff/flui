//! Semantics roles for accessibility.
//!
//! This module provides role types for accessibility semantics.
//! Roles describe the structural type of UI element and help
//! assistive technologies understand how to interact with elements.
//!
//! Note: In Flutter, interactive element types like Button, Checkbox,
//! Slider are represented as flags (`SemanticsFlag`), not roles.
//! Roles are for structural elements (tables, menus, regions, etc.).

// ============================================================================
// SemanticsRole
// ============================================================================

/// The role of a semantics node.
///
/// Roles provide additional context about the structural type of UI element
/// to assistive technologies. This helps screen readers and other
/// accessibility tools present the correct interaction model.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SemanticsRole` enum from dart:ui.
///
/// # Note
///
/// Interactive element types (Button, Checkbox, Slider, etc.) are represented
/// as [`SemanticsFlag`](crate::SemanticsFlag), not roles. Roles are for
/// structural elements like tables, menus, landmarks, and regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum SemanticsRole {
    /// No specific role assigned.
    #[default]
    None = 0,

    /// A dialog that alerts the user.
    AlertDialog = 1,

    /// A dialog window.
    Dialog = 2,

    /// A tab in a tab bar.
    Tab = 3,

    /// A container for tabs.
    TabBar = 4,

    /// The content panel for a tab.
    TabPanel = 5,

    /// A table structure.
    Table = 6,

    /// A cell in a table.
    Cell = 7,

    /// A row in a table.
    Row = 8,

    /// A column header in a table.
    ColumnHeader = 9,

    /// A group of mutually exclusive radio buttons.
    RadioGroup = 10,

    /// A menu container.
    Menu = 11,

    /// A horizontal menu bar.
    MenuBar = 12,

    /// An item in a menu.
    MenuItem = 13,

    /// A checkbox item in a menu.
    MenuItemCheckbox = 14,

    /// A radio item in a menu.
    MenuItemRadio = 15,

    /// An alert message (live region).
    Alert = 16,

    /// A status message (live region).
    Status = 17,

    /// A list container.
    List = 18,

    /// An item in a list.
    ListItem = 19,

    /// Complementary content (sidebar, etc.).
    Complementary = 20,

    /// Footer/content info region.
    ContentInfo = 21,

    /// Main content region.
    Main = 22,

    /// Navigation region.
    Navigation = 23,

    /// A generic region with a label.
    Region = 24,

    /// A form container.
    Form = 25,

    /// A handle for drag and drop.
    DragHandle = 26,

    /// A spin button (numeric stepper).
    SpinButton = 27,

    /// A combo box (dropdown with text input).
    ComboBox = 28,

    /// A tooltip popup.
    Tooltip = 29,

    /// A loading spinner/indicator.
    LoadingSpinner = 30,

    /// A progress bar.
    ProgressBar = 31,

    /// A keyboard shortcut indicator.
    HotKey = 32,
}

impl SemanticsRole {
    /// Returns the string name of this role.
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AlertDialog => "alertDialog",
            Self::Dialog => "dialog",
            Self::Tab => "tab",
            Self::TabBar => "tabBar",
            Self::TabPanel => "tabPanel",
            Self::Table => "table",
            Self::Cell => "cell",
            Self::Row => "row",
            Self::ColumnHeader => "columnHeader",
            Self::RadioGroup => "radioGroup",
            Self::Menu => "menu",
            Self::MenuBar => "menuBar",
            Self::MenuItem => "menuItem",
            Self::MenuItemCheckbox => "menuItemCheckbox",
            Self::MenuItemRadio => "menuItemRadio",
            Self::Alert => "alert",
            Self::Status => "status",
            Self::List => "list",
            Self::ListItem => "listItem",
            Self::Complementary => "complementary",
            Self::ContentInfo => "contentInfo",
            Self::Main => "main",
            Self::Navigation => "navigation",
            Self::Region => "region",
            Self::Form => "form",
            Self::DragHandle => "dragHandle",
            Self::SpinButton => "spinButton",
            Self::ComboBox => "comboBox",
            Self::Tooltip => "tooltip",
            Self::LoadingSpinner => "loadingSpinner",
            Self::ProgressBar => "progressBar",
            Self::HotKey => "hotKey",
        }
    }

    /// Returns the numeric value of this role.
    #[inline]
    pub fn value(self) -> u32 {
        self as u32
    }

    /// Returns all semantics roles.
    pub fn values() -> &'static [SemanticsRole] {
        &[
            Self::None,
            Self::AlertDialog,
            Self::Dialog,
            Self::Tab,
            Self::TabBar,
            Self::TabPanel,
            Self::Table,
            Self::Cell,
            Self::Row,
            Self::ColumnHeader,
            Self::RadioGroup,
            Self::Menu,
            Self::MenuBar,
            Self::MenuItem,
            Self::MenuItemCheckbox,
            Self::MenuItemRadio,
            Self::Alert,
            Self::Status,
            Self::List,
            Self::ListItem,
            Self::Complementary,
            Self::ContentInfo,
            Self::Main,
            Self::Navigation,
            Self::Region,
            Self::Form,
            Self::DragHandle,
            Self::SpinButton,
            Self::ComboBox,
            Self::Tooltip,
            Self::LoadingSpinner,
            Self::ProgressBar,
            Self::HotKey,
        ]
    }

    /// Returns whether this is a landmark role.
    ///
    /// Landmark roles help users navigate the page structure.
    pub fn is_landmark(self) -> bool {
        matches!(
            self,
            Self::Complementary | Self::ContentInfo | Self::Main | Self::Navigation | Self::Region
        )
    }

    /// Returns whether this is a live region role.
    ///
    /// Live region roles announce changes automatically.
    pub fn is_live_region(self) -> bool {
        matches!(self, Self::Alert | Self::Status)
    }

    /// Returns whether this is a menu-related role.
    pub fn is_menu_related(self) -> bool {
        matches!(
            self,
            Self::Menu
                | Self::MenuBar
                | Self::MenuItem
                | Self::MenuItemCheckbox
                | Self::MenuItemRadio
        )
    }

    /// Returns whether this is a table-related role.
    pub fn is_table_related(self) -> bool {
        matches!(
            self,
            Self::Table | Self::Cell | Self::Row | Self::ColumnHeader
        )
    }

    /// Returns whether this is a dialog role.
    pub fn is_dialog(self) -> bool {
        matches!(self, Self::Dialog | Self::AlertDialog)
    }

    /// Returns whether this is a list-related role.
    pub fn is_list_related(self) -> bool {
        matches!(self, Self::List | Self::ListItem)
    }

    /// Returns whether this is a tab-related role.
    pub fn is_tab_related(self) -> bool {
        matches!(self, Self::Tab | Self::TabBar | Self::TabPanel)
    }
}

impl std::fmt::Display for SemanticsRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// AccessibilityFocusBlockType
// ============================================================================

/// Controls how accessibility focus is blocked.
///
/// This is typically used to prevent screen readers from focusing
/// on parts of the UI.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `AccessiblityFocusBlockType` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AccessibilityFocusBlockType {
    /// Accessibility focus is **not blocked**.
    #[default]
    None,

    /// Blocks accessibility focus for the entire subtree.
    BlockSubtree,

    /// Blocks accessibility focus for the **current node only**.
    /// Its descendants may still be focusable.
    BlockNode,
}

impl AccessibilityFocusBlockType {
    /// Merges two focus block types.
    ///
    /// The result follows these rules:
    /// 1. If either is `BlockSubtree`, the result is `BlockSubtree`.
    /// 2. If either is `BlockNode`, the result is `BlockNode`.
    /// 3. Otherwise, the result is `None`.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        // If either is blockSubtree, the result is blockSubtree
        if self == Self::BlockSubtree || other == Self::BlockSubtree {
            return Self::BlockSubtree;
        }

        // If either is blockNode, the result is blockNode
        if self == Self::BlockNode || other == Self::BlockNode {
            return Self::BlockNode;
        }

        // Otherwise both are none
        Self::None
    }

    /// Returns whether focus is blocked in some way.
    pub fn is_blocked(self) -> bool {
        self != Self::None
    }
}

// ============================================================================
// DebugSemanticsDumpOrder
// ============================================================================

/// Order for dumping the semantics tree in debug output.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `DebugSemanticsDumpOrder` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DebugSemanticsDumpOrder {
    /// Inverse hit test order (visual, bottom-to-top).
    #[default]
    InverseHitTest,

    /// Traversal order (accessibility navigation order).
    TraversalOrder,
}

// ============================================================================
// Assertiveness
// ============================================================================

/// The assertiveness level for accessibility announcements.
///
/// This controls how urgently screen readers announce content.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `Assertiveness` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Assertiveness {
    /// Polite announcements wait for the user to finish.
    #[default]
    Polite,

    /// Assertive announcements interrupt the user immediately.
    Assertive,
}

impl Assertiveness {
    /// Returns the string name of this assertiveness level.
    pub fn name(self) -> &'static str {
        match self {
            Self::Polite => "polite",
            Self::Assertive => "assertive",
        }
    }
}

impl std::fmt::Display for Assertiveness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_values() {
        assert_eq!(SemanticsRole::None.value(), 0);
        assert_eq!(SemanticsRole::AlertDialog.value(), 1);
        assert_eq!(SemanticsRole::Dialog.value(), 2);
    }

    #[test]
    fn test_role_names() {
        assert_eq!(SemanticsRole::None.name(), "none");
        assert_eq!(SemanticsRole::AlertDialog.name(), "alertDialog");
        assert_eq!(SemanticsRole::MenuItem.name(), "menuItem");
    }

    #[test]
    fn test_role_is_landmark() {
        assert!(SemanticsRole::Main.is_landmark());
        assert!(SemanticsRole::Navigation.is_landmark());
        assert!(SemanticsRole::Complementary.is_landmark());
        assert!(!SemanticsRole::None.is_landmark());
        assert!(!SemanticsRole::Menu.is_landmark());
    }

    #[test]
    fn test_role_is_live_region() {
        assert!(SemanticsRole::Alert.is_live_region());
        assert!(SemanticsRole::Status.is_live_region());
        assert!(!SemanticsRole::None.is_live_region());
    }

    #[test]
    fn test_role_is_menu_related() {
        assert!(SemanticsRole::Menu.is_menu_related());
        assert!(SemanticsRole::MenuBar.is_menu_related());
        assert!(SemanticsRole::MenuItem.is_menu_related());
        assert!(!SemanticsRole::List.is_menu_related());
    }

    #[test]
    fn test_role_is_table_related() {
        assert!(SemanticsRole::Table.is_table_related());
        assert!(SemanticsRole::Cell.is_table_related());
        assert!(SemanticsRole::Row.is_table_related());
        assert!(!SemanticsRole::List.is_table_related());
    }

    #[test]
    fn test_role_is_dialog() {
        assert!(SemanticsRole::Dialog.is_dialog());
        assert!(SemanticsRole::AlertDialog.is_dialog());
        assert!(!SemanticsRole::Menu.is_dialog());
    }

    #[test]
    fn test_role_is_list_related() {
        assert!(SemanticsRole::List.is_list_related());
        assert!(SemanticsRole::ListItem.is_list_related());
        assert!(!SemanticsRole::Table.is_list_related());
    }

    #[test]
    fn test_role_is_tab_related() {
        assert!(SemanticsRole::Tab.is_tab_related());
        assert!(SemanticsRole::TabBar.is_tab_related());
        assert!(SemanticsRole::TabPanel.is_tab_related());
        assert!(!SemanticsRole::Menu.is_tab_related());
    }

    #[test]
    fn test_focus_block_merge() {
        use AccessibilityFocusBlockType::*;

        // BlockSubtree takes precedence
        assert_eq!(BlockSubtree.merge(None), BlockSubtree);
        assert_eq!(None.merge(BlockSubtree), BlockSubtree);
        assert_eq!(BlockSubtree.merge(BlockNode), BlockSubtree);

        // BlockNode next
        assert_eq!(BlockNode.merge(None), BlockNode);
        assert_eq!(None.merge(BlockNode), BlockNode);

        // None + None = None
        assert_eq!(None.merge(None), None);
    }

    #[test]
    fn test_focus_block_is_blocked() {
        assert!(!AccessibilityFocusBlockType::None.is_blocked());
        assert!(AccessibilityFocusBlockType::BlockNode.is_blocked());
        assert!(AccessibilityFocusBlockType::BlockSubtree.is_blocked());
    }

    #[test]
    fn test_assertiveness() {
        assert_eq!(Assertiveness::Polite.name(), "polite");
        assert_eq!(Assertiveness::Assertive.name(), "assertive");
    }

    #[test]
    fn test_all_roles() {
        let roles = SemanticsRole::values();
        assert_eq!(roles.len(), 33); // 0-32 inclusive
        assert!(roles.contains(&SemanticsRole::None));
        assert!(roles.contains(&SemanticsRole::HotKey));
    }
}
