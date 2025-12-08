//! Layout RenderObjects

// ============================================================================
// Module declarations - Single Arity (Migrated)
// ============================================================================
pub mod aspect_ratio;
pub mod baseline;
pub mod fractionally_sized_box;
pub mod intrinsic_height;
pub mod intrinsic_width;
pub mod padding;
pub mod positioned_box;
pub mod rotated_box;
pub mod shifted_box;
pub mod sized_overflow_box;

// ============================================================================
// Leaf Arity (Migrated)
// ============================================================================
pub mod empty;

// ============================================================================
// Optional Arity (Migrated)
// ============================================================================
pub mod align;
pub mod sized_box;

// ============================================================================
// Variable Arity (Migrated)
// ============================================================================
pub mod custom_multi_child_layout_box;
pub mod flex;
pub mod flow;
pub mod grid;
pub mod indexed_stack;
pub mod list_body;
pub mod list_wheel_viewport;
pub mod stack;
pub mod table;
pub mod wrap;

// ============================================================================
// Optional Arity (Migrated)
// ============================================================================
pub mod constrained_box;
pub mod constrained_overflow_box;
pub mod limited_box;
pub mod overflow_box;

// ============================================================================
// Single Arity (Migrated - Metadata Providers)
// ============================================================================
pub mod constraints_transform_box;
pub mod flex_item;
pub mod fractional_translation;
pub mod positioned;

pub mod custom_single_child_layout_box;

// ============================================================================
// TODO: Uncomment after migration
// ============================================================================
// pub mod editable_line;                   // Optional arity
pub mod scroll_view; // Single arity

// ============================================================================
// Re-exports - Single Arity (Migrated)
// ============================================================================
pub use aspect_ratio::RenderAspectRatio;
pub use baseline::RenderBaseline;
pub use fractionally_sized_box::RenderFractionallySizedBox;
pub use intrinsic_height::RenderIntrinsicHeight;
pub use intrinsic_width::RenderIntrinsicWidth;
pub use padding::RenderPadding;
pub use positioned_box::RenderPositionedBox;
pub use rotated_box::RenderRotatedBox;
pub use shifted_box::RenderShiftedBox;
pub use sized_overflow_box::RenderSizedOverflowBox;

// Leaf arity
pub use empty::RenderEmpty;

// Optional arity
pub use align::RenderAlign;
pub use sized_box::RenderSizedBox;

// Variable arity
pub use custom_multi_child_layout_box::{
    MultiChildLayoutContext, MultiChildLayoutDelegate, RenderCustomMultiChildLayoutBox,
    SimpleGridDelegate,
};
pub use flex::RenderFlex;
pub use flow::{FlowDelegate, RenderFlow, SimpleFlowDelegate};
pub use grid::{GridPlacement, GridTrackSize, RenderGrid};
pub use indexed_stack::RenderIndexedStack;
pub use list_body::RenderListBody;
pub use list_wheel_viewport::RenderListWheelViewport;
pub use stack::RenderStack;
pub use table::{RenderTable, TableCellVerticalAlignment, TableColumnWidth};
pub use wrap::{RenderWrap, WrapAlignment, WrapCrossAlignment};

// Optional arity
pub use constrained_box::RenderConstrainedBox;
pub use constrained_overflow_box::RenderConstrainedOverflowBox;
pub use limited_box::RenderLimitedBox;
pub use overflow_box::RenderOverflowBox;

// Single arity
pub use constraints_transform_box::{BoxConstraintsTransform, RenderConstraintsTransformBox};
pub use custom_single_child_layout_box::{
    CenterDelegate, FixedSizeDelegate, RenderCustomSingleChildLayoutBox, SingleChildLayoutDelegate,
};
pub use flex_item::{FlexItemMetadata, RenderFlexItem};
pub use fractional_translation::RenderFractionalTranslation;
pub use positioned::{PositionedMetadata, RenderPositioned};

// ============================================================================
// TODO: Uncomment after migration
// ============================================================================
// pub use editable_line::{RenderEditableLine, TextSelection};
pub use scroll_view::RenderScrollView;
