//! Integration test for new Render architecture
//!
//! This test verifies that the new enum-based Render architecture works correctly
//! and that backward compatibility with legacy Render trait is maintained.

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::{
    LayoutCx, LeafAdapter, LeafArity, LeafRender, MultiAdapter, MultiArity, MultiRender, PaintCx,
    Render, Render, SingleAdapter, SingleArity, SingleRender,
};
use flui_engine::{BoxedLayer, ContainerLayer};
use flui_types::{constraints::BoxConstraints, Offset, Size};

// ========== Test implementations using NEW API ==========

#[derive(Debug)]
struct NewLeafImpl {
    size: Size,
}

impl LeafRender for NewLeafImpl {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        constraints.constrain(self.size)
    }

    fn paint(&self, _offset: Offset) -> BoxedLayer {
        Box::new(ContainerLayer::new())
    }

    fn debug_name(&self) -> &'static str {
        "NewLeafImpl"
    }
}

#[derive(Debug)]
struct NewSingleImpl;

impl SingleRender for NewSingleImpl {
    fn layout(
        &mut self,
        _tree: &ElementTree,
        _child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        constraints.constrain(Size::new(200.0, 200.0))
    }

    fn paint(&self, _tree: &ElementTree, _child_id: ElementId, _offset: Offset) -> BoxedLayer {
        Box::new(ContainerLayer::new())
    }

    fn debug_name(&self) -> &'static str {
        "NewSingleImpl"
    }
}

#[derive(Debug)]
struct NewMultiImpl;

impl MultiRender for NewMultiImpl {
    fn layout(
        &mut self,
        _tree: &ElementTree,
        _children: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size {
        constraints.constrain(Size::new(300.0, 300.0))
    }

    fn paint(&self, _tree: &ElementTree, _children: &[ElementId], _offset: Offset) -> BoxedLayer {
        Box::new(ContainerLayer::new())
    }

    fn debug_name(&self) -> &'static str {
        "NewMultiImpl"
    }
}

// ========== Test implementations using LEGACY API ==========

#[derive(Debug)]
struct LegacyLeafImpl {
    size: Size,
}

impl Render for LegacyLeafImpl {
    type Arity = LeafArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        cx.constraints().constrain(self.size)
    }

    fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        Box::new(ContainerLayer::new())
    }

    fn debug_name(&self) -> &'static str {
        "LegacyLeafImpl"
    }
}

#[derive(Debug)]
struct LegacySingleImpl;

impl Render for LegacySingleImpl {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        cx.constraints().constrain(Size::new(150.0, 150.0))
    }

    fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        Box::new(ContainerLayer::new())
    }

    fn debug_name(&self) -> &'static str {
        "LegacySingleImpl"
    }
}

#[derive(Debug)]
struct LegacyMultiImpl;

impl Render for LegacyMultiImpl {
    type Arity = MultiArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        cx.constraints().constrain(Size::new(250.0, 250.0))
    }

    fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        Box::new(ContainerLayer::new())
    }

    fn debug_name(&self) -> &'static str {
        "LegacyMultiImpl"
    }
}

// ========== Tests ==========

#[test]
fn test_new_leaf_render() {
    let mut render = Render::new_leaf(Box::new(NewLeafImpl {
        size: Size::new(100.0, 100.0),
    }));

    let tree = ElementTree::new();
    let constraints = BoxConstraints::tight(Size::new(80.0, 80.0));

    let size = render.layout(&tree, constraints);
    assert_eq!(size, Size::new(80.0, 80.0)); // Constrained

    let _layer = render.paint(&tree, Offset::ZERO);
    assert_eq!(render.debug_name(), "NewLeafImpl");
}

#[test]
fn test_new_single_render() {
    let child_id = 42;
    let mut render = Render::new_single(Box::new(NewSingleImpl), child_id);

    let tree = ElementTree::new();
    let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));

    let size = render.layout(&tree, constraints);
    assert_eq!(size, Size::new(50.0, 50.0)); // Constrained

    let _layer = render.paint(&tree, Offset::ZERO);
    assert_eq!(render.debug_name(), "NewSingleImpl");
}

#[test]
fn test_new_multi_render() {
    let children = vec![1, 2, 3];
    let mut render = Render::new_multi(Box::new(NewMultiImpl), children);

    let tree = ElementTree::new();
    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

    let size = render.layout(&tree, constraints);
    assert_eq!(size, Size::new(100.0, 100.0)); // Constrained

    let _layer = render.paint(&tree, Offset::ZERO);
    assert_eq!(render.debug_name(), "NewMultiImpl");
}

#[test]
fn test_legacy_leaf_adapter() {
    let mut render = Render::from_legacy_leaf(LegacyLeafImpl {
        size: Size::new(120.0, 120.0),
    });

    let tree = ElementTree::new();
    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

    let size = render.layout(&tree, constraints);
    assert_eq!(size, Size::new(100.0, 100.0)); // Constrained

    let _layer = render.paint(&tree, Offset::ZERO);
    assert!(render.debug_name().contains("LegacyLeafImpl"));
}

#[test]
fn test_legacy_single_adapter() {
    let child_id = 99;
    let mut render = Render::from_legacy_single(LegacySingleImpl, child_id);

    let tree = ElementTree::new();
    let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));

    let size = render.layout(&tree, constraints);
    assert_eq!(size, Size::new(50.0, 50.0)); // Constrained

    let _layer = render.paint(&tree, Offset::ZERO);
    assert!(render.debug_name().contains("LegacySingleImpl"));
}

#[test]
fn test_legacy_multi_adapter() {
    let children = vec![10, 20, 30];
    let mut render = Render::from_legacy_multi(LegacyMultiImpl, children);

    let tree = ElementTree::new();
    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

    let size = render.layout(&tree, constraints);
    assert_eq!(size, Size::new(100.0, 100.0)); // Constrained

    let _layer = render.paint(&tree, Offset::ZERO);
    assert!(render.debug_name().contains("LegacyMultiImpl"));
}

#[test]
fn test_render_intrinsics() {
    #[derive(Debug)]
    struct LeafWithIntrinsics;

    impl LeafRender for LeafWithIntrinsics {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            constraints.constrain(Size::new(100.0, 100.0))
        }

        fn paint(&self, _offset: Offset) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }

        fn intrinsic_width(&self, _height: Option<f32>) -> Option<f32> {
            Some(200.0)
        }

        fn intrinsic_height(&self, _width: Option<f32>) -> Option<f32> {
            Some(150.0)
        }
    }

    let render = Render::new_leaf(Box::new(LeafWithIntrinsics));

    assert_eq!(render.intrinsic_width(Some(100.0)), Some(200.0));
    assert_eq!(render.intrinsic_height(Some(100.0)), Some(150.0));
}

#[test]
fn test_render_pattern_matching() {
    let leaf = Render::new_leaf(Box::new(NewLeafImpl {
        size: Size::new(50.0, 50.0),
    }));
    let single = Render::new_single(Box::new(NewSingleImpl), 1);
    let multi = Render::new_multi(Box::new(NewMultiImpl), vec![1, 2, 3]);

    match leaf {
        Render::Leaf(_) => {}
        _ => panic!("Expected Leaf variant"),
    }

    match single {
        Render::Single { .. } => {}
        _ => panic!("Expected Single variant"),
    }

    match multi {
        Render::Multi { .. } => {}
        _ => panic!("Expected Multi variant"),
    }
}

#[test]
fn test_mixed_legacy_and_new() {
    // This test verifies that legacy and new implementations can coexist
    let renders: Vec<Render> = vec![
        Render::new_leaf(Box::new(NewLeafImpl {
            size: Size::new(10.0, 10.0),
        })),
        Render::from_legacy_leaf(LegacyLeafImpl {
            size: Size::new(20.0, 20.0),
        }),
        Render::new_single(Box::new(NewSingleImpl), 1),
        Render::from_legacy_single(LegacySingleImpl, 2),
        Render::new_multi(Box::new(NewMultiImpl), vec![1]),
        Render::from_legacy_multi(LegacyMultiImpl, vec![2]),
    ];

    assert_eq!(renders.len(), 6);

    // All should have debug names
    for render in &renders {
        assert!(!render.debug_name().is_empty());
    }
}
