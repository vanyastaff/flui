1. Generics**

```rust
// Базовый
fn foo<T>(x: T) {}

// С bounds
fn foo<T: Clone + Send>(x: T) {}

// Where clause
fn foo<T, U>(x: T, y: U) 
where 
    T: Clone + Send,
    U: Into<T>,
{}

// Default type parameter
struct Container<T = String>(T);

// Const generics
struct Array<T, const N: usize>([T; N]);
```

## **2. Associated Types**

```rust
trait Iterator {
    type Item;
    fn next(&mut self) -> Option<Self::Item>;
}

// С bounds
trait Container {
    type Item: Clone + Send;
    type Error: std::error::Error;
}

// Использование
impl Iterator for MyIter {
    type Item = i32;
    fn next(&mut self) -> Option<i32> { None }
}
```

## **3. GAT (Generic Associated Types)** - стабильно с 1.65

```rust
trait LendingIterator {
    type Item<'a> where Self: 'a;
    
    fn next<'a>(&'a mut self) -> Option<Self::Item<'a>>;
}

trait Factory {
    type Output<T>;
    
    fn create<T: Default>(&self) -> Self::Output<T>;
}

// Пример: возвращаем ссылку с lifetime
trait TreeAccess {
    type NodeRef<'a>: Deref<Target = Node> where Self: 'a;
    type NodeMut<'a>: DerefMut<Target = Node> where Self: 'a;
    
    fn get<'a>(&'a self, id: Id) -> Option<Self::NodeRef<'a>>;
    fn get_mut<'a>(&'a mut self, id: Id) -> Option<Self::NodeMut<'a>>;
}
```

## **4. Lifetimes**

```rust
// Базовый
fn foo<'a>(x: &'a str) -> &'a str { x }

// Multiple lifetimes
fn foo<'a, 'b>(x: &'a str, y: &'b str) -> &'a str { x }

// Lifetime bounds
fn foo<'a, 'b: 'a>(x: &'a str, y: &'b str) -> &'a str { y }

// В struct
struct Wrapper<'a, T: 'a> {
    data: &'a T,
}

// HRTB (Higher-Ranked Trait Bounds)
fn apply<F>(f: F) 
where 
    F: for<'a> Fn(&'a str) -> &'a str 
{}

// Static
fn foo() -> &'static str { "hello" }
```

## **5. Trait Bounds & Supertraits**

```rust
// Supertraits
trait Render: Send + Sync + Debug + 'static {}

// Impl автоматически требует supertraits
impl Render for MyType {}  // MyType должен impl Send + Sync + Debug

// Комплексные bounds
trait FullAccess: TreeRead + TreeWrite + TreeNav 
where 
    Self: Send + Sync 
{}

// Blanket impl
impl<T> FullAccess for T 
where 
    T: TreeRead + TreeWrite + TreeNav + Send + Sync 
{}
```

## **6. Sealed Traits**

```rust
mod private {
    pub trait Sealed {}
}

// Публичный trait, но impl только для наших типов
pub trait RenderTree: private::Sealed {
    fn layout(&mut self);
}

// Только мы можем impl Sealed
impl private::Sealed for ElementTree {}
impl RenderTree for ElementTree {
    fn layout(&mut self) {}
}
```

## **7. Extension Traits**

```rust
// Базовый trait
pub trait TreeRead {
    fn get(&self, id: ElementId) -> Option<&dyn Any>;
}

// Extension с default impl
pub trait TreeReadExt: TreeRead {
    fn get_typed<T: 'static>(&self, id: ElementId) -> Option<&T> {
        self.get(id)?.downcast_ref()
    }
    
    fn contains(&self, id: ElementId) -> bool {
        self.get(id).is_some()
    }
}

// Blanket impl - все TreeRead автоматически получают Ext
impl<T: TreeRead + ?Sized> TreeReadExt for T {}
```

## **8. Blanket Implementations**

```rust
// Для всех T с определённым trait
impl<T: Display> ToString for T {
    fn to_string(&self) -> String { format!("{}", self) }
}

// Для references
impl<T: TreeRead + ?Sized> TreeRead for &T {
    fn get(&self, id: ElementId) -> Option<&Node> {
        (**self).get(id)
    }
}

impl<T: TreeRead + ?Sized> TreeRead for &mut T { /* ... */ }
impl<T: TreeRead + ?Sized> TreeRead for Box<T> { /* ... */ }
impl<T: TreeRead + Send + Sync> TreeRead for Arc<T> { /* ... */ }
```

## **9. Marker Traits & PhantomData**

```rust
use std::marker::PhantomData;

// Marker traits (без методов)
pub trait Arity {}
pub struct Leaf;
pub struct Single;
pub struct Variable;

impl Arity for Leaf {}
impl Arity for Single {}
impl Arity for Variable {}

// PhantomData для unused generics
pub struct RenderBox<A: Arity> {
    size: Size,
    _arity: PhantomData<A>,
}

// PhantomData для lifetimes
pub struct TreeRef<'a, T> {
    ptr: *const T,
    _lifetime: PhantomData<&'a T>,
}
```

## **10. Type State Pattern**

```rust
use std::marker::PhantomData;

// States
pub struct Unlocked;
pub struct LockedRead;
pub struct LockedWrite;

pub struct Tree<State = Unlocked> {
    inner: Arc<RwLock<ElementTree>>,
    _state: PhantomData<State>,
}

impl Tree<Unlocked> {
    pub fn lock_read(self) -> Tree<LockedRead> {
        Tree { inner: self.inner, _state: PhantomData }
    }
    
    pub fn lock_write(self) -> Tree<LockedWrite> {
        Tree { inner: self.inner, _state: PhantomData }
    }
}

impl Tree<LockedRead> {
    pub fn get(&self, id: ElementId) -> Option<&Node> { /* ... */ }
    pub fn unlock(self) -> Tree<Unlocked> {
        Tree { inner: self.inner, _state: PhantomData }
    }
}

impl Tree<LockedWrite> {
    pub fn get_mut(&mut self, id: ElementId) -> Option<&mut Node> { /* ... */ }
    pub fn layout_child(&mut self, id: ElementId) -> Size { /* ... */ }
}
```

## **11. Newtype Pattern**

```rust
// Wrapper для foreign traits
pub struct ElementId(NonZeroUsize);

impl ElementId {
    pub fn new(id: usize) -> Self {
        Self(NonZeroUsize::new(id).expect("id must be non-zero"))
    }
    
    pub fn get(&self) -> usize {
        self.0.get()
    }
}

// Добавляем traits
impl From<usize> for ElementId {
    fn from(id: usize) -> Self { Self::new(id) }
}
```

## **12. Deref / DerefMut**

```rust
use std::ops::{Deref, DerefMut};

pub struct TreeGuard<'a, T> {
    inner: RwLockReadGuard<'a, T>,
}

impl<'a, T> Deref for TreeGuard<'a, T> {
    type Target = T;
    
    fn deref(&self) -> &T {
        &*self.inner
    }
}

// Теперь TreeGuard автоматически имеет методы T
// guard.get(id) вместо guard.inner.get(id)
```

## **13. From / Into / TryFrom / TryInto**

```rust
// From (автоматически даёт Into)
impl From<ElementTree> for RenderTree {
    fn from(tree: ElementTree) -> Self {
        RenderTree { inner: Arc::new(RwLock::new(tree)) }
    }
}

// TryFrom для fallible conversion
impl TryFrom<String> for ElementId {
    type Error = ParseError;
    
    fn try_from(s: String) -> Result<Self, Self::Error> {
        let id: usize = s.parse()?;
        Ok(ElementId::new(id))
    }
}

// Использование
let tree: RenderTree = element_tree.into();
let id: ElementId = "42".to_string().try_into()?;
```

## **14. AsRef / AsMut / Borrow**

```rust
// AsRef - дешёвая ссылочная конверсия
impl AsRef<ElementTree> for RenderTree {
    fn as_ref(&self) -> &ElementTree {
        // ...
    }
}

// Generic функция принимает что угодно convertible
fn process<T: AsRef<ElementTree>>(tree: T) {
    let tree: &ElementTree = tree.as_ref();
}

// Borrow - для HashMap keys
use std::borrow::Borrow;

impl Borrow<usize> for ElementId {
    fn borrow(&self) -> &usize {
        // ElementId can be used as key, lookup by &usize
    }
}
```

## **15. Builder Pattern (Typestate)**

```rust
pub struct TreeBuilder<HasRoot = (), HasCapacity = ()> {
    root: HasRoot,
    capacity: HasCapacity,
}

impl TreeBuilder<(), ()> {
    pub fn new() -> Self {
        TreeBuilder { root: (), capacity: () }
    }
}

impl<C> TreeBuilder<(), C> {
    pub fn with_root(self, root: ElementId) -> TreeBuilder<ElementId, C> {
        TreeBuilder { root, capacity: self.capacity }
    }
}

impl<R> TreeBuilder<R, ()> {
    pub fn with_capacity(self, cap: usize) -> TreeBuilder<R, usize> {
        TreeBuilder { root: self.root, capacity: cap }
    }
}

// build() только когда всё указано
impl TreeBuilder<ElementId, usize> {
    pub fn build(self) -> ElementTree {
        ElementTree::with_capacity(self.capacity)
    }
}

// Использование
let tree = TreeBuilder::new()
    .with_capacity(100)
    .with_root(root_id)
    .build();
```

## **16. Closure Traits**

```rust
// Fn - только читает captured variables
fn apply<F: Fn(i32) -> i32>(f: F, x: i32) -> i32 {
    f(x)
}

// FnMut - может mutate captured variables  
fn apply_mut<F: FnMut(i32) -> i32>(mut f: F, x: i32) -> i32 {
    f(x)
}

// FnOnce - потребляет captured variables
fn apply_once<F: FnOnce(i32) -> i32>(f: F, x: i32) -> i32 {
    f(x)
}

// В struct
pub struct Callback<F: Fn(Size) -> Size> {
    callback: F,
}

// С Box для dynamic dispatch
pub struct DynCallback {
    callback: Box<dyn Fn(Size) -> Size + Send + Sync>,
}
```

## **17. Smart Pointers**

```rust
// Box - heap allocation
let node: Box<dyn RenderNode> = Box::new(my_node);

// Rc - single-threaded reference counting
let shared: Rc<Node> = Rc::new(node);
let clone = Rc::clone(&shared);
let weak: Weak<Node> = Rc::downgrade(&shared);

// Arc - thread-safe reference counting
let shared: Arc<Node> = Arc::new(node);
let clone = Arc::clone(&shared);

// RefCell - interior mutability (single-thread)
let cell: RefCell<Node> = RefCell::new(node);
let borrowed: Ref<Node> = cell.borrow();
let borrowed_mut: RefMut<Node> = cell.borrow_mut();

// Cow - Clone on Write
let cow: Cow<str> = Cow::Borrowed("hello");
let cow: Cow<str> = Cow::Owned(String::from("hello"));
```

## **18. Interior Mutability**

```rust
use std::cell::{Cell, RefCell};
use std::sync::{Mutex, RwLock};
use parking_lot::{Mutex as PLMutex, RwLock as PLRwLock};

// Cell - для Copy types
pub struct Counter {
    count: Cell<usize>,
}

impl Counter {
    pub fn increment(&self) {  // &self, не &mut self!
        self.count.set(self.count.get() + 1);
    }
}

// RefCell - runtime borrow checking
pub struct Tree {
    nodes: RefCell<Vec<Node>>,
}

// Mutex/RwLock - thread-safe
pub struct SharedTree {
    inner: Arc<RwLock<ElementTree>>,
}

impl SharedTree {
    pub fn read(&self) -> RwLockReadGuard<ElementTree> {
        self.inner.read().unwrap()  // parking_lot: self.inner.read()
    }
}
```

## **19. Iterator Pattern**

```rust
pub struct ChildIterator<'a> {
    tree: &'a ElementTree,
    children: &'a [ElementId],
    index: usize,
}

impl<'a> Iterator for ChildIterator<'a> {
    type Item = &'a Node;
    
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.children.get(self.index)?;
        self.index += 1;
        self.tree.get(*id)
    }
}

// IntoIterator для for loops
impl<'a> IntoIterator for &'a ElementTree {
    type Item = &'a Node;
    type IntoIter = NodeIterator<'a>;
    
    fn into_iter(self) -> Self::IntoIter {
        NodeIterator::new(self)
    }
}
```

## **20. Error Handling**

```rust
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub enum TreeError {
    NotFound(ElementId),
    CycleDetected(ElementId),
    InvalidState(String),
}

impl Display for TreeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "Element not found: {:?}", id),
            Self::CycleDetected(id) => write!(f, "Cycle detected at: {:?}", id),
            Self::InvalidState(msg) => write!(f, "Invalid state: {}", msg),
        }
    }
}

impl Error for TreeError {}

// Conversion для ?
impl From<std::io::Error> for TreeError {
    fn from(e: std::io::Error) -> Self {
        TreeError::InvalidState(e.to_string())
    }
}

pub type TreeResult<T> = Result<T, TreeError>;
```

## **21. Drop & RAII**

```rust
pub struct TreeLock<'a> {
    tree: &'a RwLock<ElementTree>,
    _guard: RwLockWriteGuard<'a, ElementTree>,
}

impl<'a> Drop for TreeLock<'a> {
    fn drop(&mut self) {
        // Автоматически освобождает lock
        // Можно добавить cleanup логику
        tracing::debug!("TreeLock released");
    }
}
```

## **22. Const Fn**

```rust
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Size = Size { width: 0.0, height: 0.0 };
    
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
    
    pub const fn square(size: f32) -> Self {
        Self { width: size, height: size }
    }
}

// Можно использовать в const context
const DEFAULT_SIZE: Size = Size::new(100.0, 100.0);
```

## **23. Conditional Compilation**

```rust
#[cfg(feature = "parallel")]
use rayon::prelude::*;

#[cfg(feature = "parallel")]
impl ElementTree {
    pub fn layout_parallel(&mut self) {
        self.nodes.par_iter_mut().for_each(|node| {
            // parallel layout
        });
    }
}

#[cfg(not(feature = "parallel"))]
impl ElementTree {
    pub fn layout_parallel(&mut self) {
        self.nodes.iter_mut().for_each(|node| {
            // sequential fallback
        });
    }
}

// cfg_attr
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Node { /* ... */ }
```

## **24. Visibility Modifiers**

```rust
pub struct Tree {
    pub root: ElementId,           // public
    pub(crate) nodes: Vec<Node>,   // crate-visible
    pub(super) cache: Cache,       // parent module visible
    inner: Inner,                  // private
}

// Re-exports
pub use inner::TreeBuilder;
pub use crate::core::*;
```

## **25. Non-Exhaustive**

```rust
#[non_exhaustive]
pub enum TreeError {
    NotFound,
    CycleDetected,
    // Future variants can be added without breaking change
}

#[non_exhaustive]
pub struct TreeConfig {
    pub capacity: usize,
    pub enable_cache: bool,
    // Future fields can be added
}

// Users must use .. in patterns
match error {
    TreeError::NotFound => {},
    TreeError::CycleDetected => {},
    _ => {},  // required!
}
