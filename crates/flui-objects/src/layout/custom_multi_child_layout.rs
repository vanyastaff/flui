//! `RenderCustomMultiChildLayoutBox` — delegates multi-child layout to a
//! [`MultiChildLayoutDelegate`].
//!
//! Flutter parity: `rendering/custom_layout.dart`
//! `RenderCustomMultiChildLayoutBox`. The render object keeps Flutter's
//! contract: parent size is `constraints.constrain(delegate.getSize)`;
//! intrinsics use the same finite-tight probe as layout; dry layout never
//! touches children; every child must carry a `LayoutId`/parent-data id; and
//! the delegate must lay out each child exactly once.

use std::{collections::HashMap, sync::Arc};

use flui_tree::Variable;
use flui_types::{Offset, Pixels, Size};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext},
    delegates::{MultiChildLayoutContext, MultiChildLayoutDelegate},
    parent_data::MultiChildLayoutParentData,
    traits::RenderBox,
};

/// A multi-child render box whose layout is controlled by a delegate.
#[derive(Debug, Clone)]
pub struct RenderCustomMultiChildLayoutBox {
    delegate: Arc<dyn MultiChildLayoutDelegate>,
    child_count: usize,
}

impl RenderCustomMultiChildLayoutBox {
    /// Creates a custom multi-child layout box.
    pub fn new(delegate: Arc<dyn MultiChildLayoutDelegate>) -> Self {
        Self {
            delegate,
            child_count: 0,
        }
    }

    /// Returns the current layout delegate.
    pub fn delegate(&self) -> &dyn MultiChildLayoutDelegate {
        &*self.delegate
    }

    /// Replaces the delegate and returns whether layout must be recomputed.
    ///
    /// Mirrors Flutter's setter: the identical delegate instance is a no-op;
    /// changing the concrete delegate type forces relayout; otherwise the new
    /// delegate's `should_relayout(old_delegate)` decides.
    pub fn set_delegate(&mut self, delegate: Arc<dyn MultiChildLayoutDelegate>) -> bool {
        if Arc::ptr_eq(&self.delegate, &delegate) {
            return false;
        }
        let type_changed = self.delegate.as_any().type_id() != delegate.as_any().type_id();
        let relayout = type_changed || delegate.should_relayout(&*self.delegate);
        self.delegate = delegate;
        relayout
    }

    /// Flutter's private `_getSize`: delegate size, then incoming constraints.
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.constrain(self.delegate.get_size(constraints))
    }

    fn intrinsic_width(&self, height: f32) -> f32 {
        let width = self
            .get_size(BoxConstraints::tight_for_finite(
                Pixels::INFINITY,
                Pixels::new(height),
            ))
            .width;
        if width.is_finite() { width.get() } else { 0.0 }
    }

    fn intrinsic_height(&self, width: f32) -> f32 {
        let height = self
            .get_size(BoxConstraints::tight_for_finite(
                Pixels::new(width),
                Pixels::INFINITY,
            ))
            .height;
        if height.is_finite() {
            height.get()
        } else {
            0.0
        }
    }

    fn child_slots(
        ctx: &mut BoxLayoutContext<'_, Variable, MultiChildLayoutParentData>,
    ) -> LayoutChildSlots {
        let child_count = ctx.child_count();
        let mut id_to_index = HashMap::with_capacity(child_count);
        let mut index_to_id = Vec::with_capacity(child_count);

        for index in 0..child_count {
            let id = ctx
                .child_parent_data_mut(index)
                .and_then(|data| data.id.clone())
                .unwrap_or_else(|| {
                    panic!(
                        "Every child of RenderCustomMultiChildLayoutBox must have an id in its parent data"
                    )
                });
            assert!(
                id_to_index.insert(id.clone(), index).is_none(),
                "Duplicate LayoutId {id:?} in RenderCustomMultiChildLayoutBox"
            );
            index_to_id.push(id);
        }

        LayoutChildSlots {
            id_to_index,
            index_to_id,
        }
    }
}

impl flui_foundation::Diagnosticable for RenderCustomMultiChildLayoutBox {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add("delegate", format!("{:?}", self.delegate));
    }
}

impl RenderBox for RenderCustomMultiChildLayoutBox {
    type Arity = Variable;
    type ParentData = MultiChildLayoutParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, MultiChildLayoutParentData>,
    ) -> Size {
        let constraints = *ctx.constraints();
        let size = self.get_size(constraints);
        self.child_count = ctx.child_count();
        let slots = Self::child_slots(ctx);
        let mut delegate_context = DelegateLayoutContext::new(ctx, slots);
        self.delegate.perform_layout(&mut delegate_context, size);
        delegate_context.finish();
        size
    }

    fn compute_min_intrinsic_width(&self, height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_width(height)
    }

    fn compute_max_intrinsic_width(&self, height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_width(height)
    }

    fn compute_min_intrinsic_height(&self, width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_height(width)
    }

    fn compute_max_intrinsic_height(&self, width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_height(width)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        self.get_size(constraints)
    }

    fn hit_test(
        &self,
        ctx: &mut BoxHitTestContext<'_, Variable, MultiChildLayoutParentData>,
    ) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        for index in (0..self.child_count).rev() {
            if ctx.hit_test_child_at_layout_offset(index) {
                return true;
            }
        }
        false
    }
}

struct LayoutChildSlots {
    id_to_index: HashMap<String, usize>,
    index_to_id: Vec<String>,
}

struct DelegateLayoutContext<'ctx, 'layout> {
    ctx: &'ctx mut BoxLayoutContext<'layout, Variable, MultiChildLayoutParentData>,
    slots: LayoutChildSlots,
    laid_out: Vec<bool>,
}

impl<'ctx, 'layout> DelegateLayoutContext<'ctx, 'layout> {
    fn new(
        ctx: &'ctx mut BoxLayoutContext<'layout, Variable, MultiChildLayoutParentData>,
        slots: LayoutChildSlots,
    ) -> Self {
        let laid_out = vec![false; slots.index_to_id.len()];
        Self {
            ctx,
            slots,
            laid_out,
        }
    }

    fn index_for(&self, child_id: &str) -> usize {
        *self.slots.id_to_index.get(child_id).unwrap_or_else(|| {
            panic!(
                "The custom multi-child layout delegate tried to access a non-existent child id {child_id:?}"
            )
        })
    }

    fn finish(self) {
        if let Some(index) = self.laid_out.iter().position(|laid_out| !*laid_out) {
            panic!(
                "Each child of RenderCustomMultiChildLayoutBox must be laid out exactly once; missing id {:?}",
                self.slots.index_to_id[index]
            );
        }
    }
}

impl MultiChildLayoutContext for DelegateLayoutContext<'_, '_> {
    fn has_child(&self, child_id: &str) -> bool {
        self.slots.id_to_index.contains_key(child_id)
    }

    fn layout_child(&mut self, child_id: &str, constraints: BoxConstraints) -> Size {
        let index = self.index_for(child_id);
        assert!(
            !self.laid_out[index],
            "The custom multi-child layout delegate tried to lay out child id {child_id:?} more than once"
        );
        let size = self.ctx.layout_child(index, constraints);
        self.laid_out[index] = true;
        size
    }

    fn position_child(&mut self, child_id: &str, offset: Offset) {
        let index = self.index_for(child_id);
        self.ctx.position_child(index, offset);
    }
}
