Код: Pure Slab Implementation
rustuse slab::Slab;
use std::cell::RefCell;

pub struct ElementTree {
elements: Slab<Element>,
render_objects: Slab<RefCell<Box<dyn DynRenderObject>>>,
root: Option<usize>,
}

pub struct Element {
widget: Box<dyn DynWidget>,
render_object_key: Option<usize>,
parent: Option<usize>,
children: Vec<usize>,
}

impl ElementTree {
pub fn new() -> Self {
Self {
elements: Slab::new(),
render_objects: Slab::new(),
root: None,
}
}

    pub fn create_element(&mut self, widget: Box<dyn DynWidget>) -> usize {
        self.elements.insert(Element {
            widget,
            render_object_key: None,
            parent: None,
            children: Vec::new(),
        })
    }
    
    pub fn attach_render_object(
        &mut self,
        elem_key: usize,
        ro: Box<dyn DynRenderObject>,
    ) {
        let ro_key = self.render_objects.insert(RefCell::new(ro));
        if let Some(element) = self.elements.get_mut(elem_key) {
            element.render_object_key = Some(ro_key);
        }
    }
    
    pub fn add_child(&mut self, parent_key: usize, child_key: usize) {
        // Add to parent's children
        if let Some(parent) = self.elements.get_mut(parent_key) {
            if !parent.children.contains(&child_key) {
                parent.children.push(child_key);
            }
        }
        
        // Set child's parent
        if let Some(child) = self.elements.get_mut(child_key) {
            child.parent = Some(parent_key);
        }
    }
    
    pub fn remove_child(&mut self, parent_key: usize, child_key: usize) {
        if let Some(parent) = self.elements.get_mut(parent_key) {
            parent.children.retain(|&k| k != child_key);
        }
        
        if let Some(child) = self.elements.get_mut(child_key) {
            child.parent = None;
        }
    }
    
    pub fn children(&self, elem_key: usize) -> &[usize] {
        self.elements.get(elem_key)
            .map(|e| e.children.as_slice())
            .unwrap_or(&[])
    }
    
    pub fn parent(&self, elem_key: usize) -> Option<usize> {
        self.elements.get(elem_key)
            .and_then(|e| e.parent)
    }
}
RenderContext с Slab
rustpub struct RenderContext<'a> {
elements: &'a Slab<Element>,
render_objects: &'a Slab<RefCell<Box<dyn DynRenderObject>>>,
current_key: usize,
}

impl<'a> RenderContext<'a> {
pub fn children(&self) -> &[usize] {
self.elements.get(self.current_key)
.map(|e| e.children.as_slice())
.unwrap_or(&[])
}

    pub fn layout_child(&self, child_key: usize, constraints: BoxConstraints) -> Size {
        let child = self.elements.get(child_key)?;
        let ro_key = child.render_object_key?;
        let ro_cell = self.render_objects.get(ro_key)?;
        
        let mut ro = ro_cell.borrow_mut();
        let child_ctx = RenderContext {
            elements: self.elements,
            render_objects: self.render_objects,
            current_key: child_key,
        };
        
        ro.layout(constraints, &child_ctx)
    }
}