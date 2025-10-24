FLUI / docs / RENDER_OBJECT_TYPED_ARITY.md
Глава 1: Почему текущая модель RenderObjectWidget теряет типы
🧩 1.1. Контекст

FLUI изначально создавался как Rust-альтернатива Flutter’у — архитектура Widget → Element → RenderObject была перенесена с максимальной точностью, но с учётом строгих Rust-гарантий и без использования GC.

В результате появилась мощная, но динамически связанная система:

Widget ──▶ Element ──▶ RenderObject
(holds state)  (does layout/paint)


Каждый слой держит ссылку на следующий, но типы разрываются на этапе компиляции — связь между Widget и RenderObject осуществляется через Box<dyn DynRenderObject>.

⚠️ 1.2. Проблема: потеря типовой информации

Посмотрим на исходный контракт:

pub trait RenderObjectWidget: Widget {
fn create_render_object(&self) -> Box<dyn DynRenderObject>;
fn update_render_object(&self, render_object: &mut dyn DynRenderObject);
}


Что здесь происходит:

create_render_object возвращает динамический тип, теряя конкретный RenderObject.

update_render_object получает &mut dyn DynRenderObject, и чтобы применить обновление,
нужно делать downcast_mut::<RenderPadding>(), RenderFlex, RenderOpacity, и т.д.

Система не знает, сколько детей (Leaf, Single, Multi) допустимо.

Компилятор не может гарантировать, что RenderOpacity не окажется с тремя детьми.

Никакая IDE-подсветка не знает, какие методы доступны у конкретного рендерера.

💣 1.3. Последствия
1. Runtime-ошибки вместо compile-time гарантии
   if let Some(render) = render_object.downcast_mut::<RenderFlex>() {
   render.set_main_axis_alignment(self.main_axis_alignment);
   }


➡️ Если тип не совпадёт — panic или no-op.
Rust здесь бесполезен — типовая безопасность обходит систему.

2. Сложность generic-связей

RenderObjectWidget не знает свой RenderObject.
А RenderObject не знает свой Widget.
Из-за этого невозможно связать:

layout → конкретный набор children

paint → конкретный state

update → конкретный render type

3. Сложность расширения

При добавлении новых классов (RenderAnimatedOpacity, RenderConstrainedBox и т.д.)
разработчик вынужден вручную писать:

if let Some(render) = render_object.downcast_mut::<RenderAnimatedOpacity>() { … }


Любое изменение в API требует переписывания десятков методов.

4. Потеря compile-time оптимизаций

Rust не может inline-ить или specialize-ить вызовы, потому что всё упаковано в Box<dyn DynRenderObject>.

dyn dispatch = 🔒  no inlining


Это значит:

layout и paint не оптимизируются LLVM’ом,

branch prediction ухудшается,

cache-локальность теряется.

5. Слабая эргономика RenderContext

Чтобы рендеры могли работать с ElementTree, был добавлен RenderContext:

fn layout(&self, state: &mut RenderState, constraints: BoxConstraints, ctx: &RenderContext) -> Size


Но в нём:

нет знания, какой у него тип (Leaf, Single, Multi);

приходится делать ctx.children().first() или ctx.children() — оба случая compile-time не проверяются;

RenderContext не может иметь generic-bound, потому что все рендеры динамические.

🧠 1.4. Эволюция проблемы
Этап	Что было сделано	Что получилось
1	Простые fn layout(&self, constraints: BoxConstraints)	не знает про дерево, не может layoutить детей
2	Добавлен RenderContext	знает про ElementTree, но не знает тип арности
3	Добавлен RenderState	теперь можно кешировать layout, но всё равно нет compile-type связи
4	Появились Leaf/Single/Multi-виджеты	но RenderObject не знает, к какому семейству он относится
5	Начало роста boilerplate и downcast	runtime-проверки вместо compile-time контрактов
🚫 1.5. Почему простые решения не работают
🧩 Marker Traits

Можно было бы добавить:

trait LeafRenderObject {}
trait SingleRenderObject {}


Но это не даёт compile-time связи:
компилятор не сможет вывести children: None или children: Vec<ElementId> без generic-связи.
Кроме того, все функции layout, paint, hit_test всё равно имеют одинаковую сигнатуру.

🧩 Enum Arity
enum Arity { Leaf, Single, Multi }


Тогда RenderContext хранит Vec<ElementId> и pattern-matching.
➡️ Это всё ещё runtime, а не compile-time;
и мы теряем zero-cost generic.

💡 1.6. Rust-подход

Rust даёт куда больше возможностей, чем классическая ООП-система Flutter.
Можно использовать ассоциированные типы, generic trait bounds, и GAT (Generalized Associated Types) для точной типовой связи:

pub trait RenderObject {
type Arity: RenderArity;
fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size;
}


Теперь RenderObject знает:

свой тип арности (Leaf / Single / Multi),

контекст, который предоставляет только допустимые операции (в Leaf нет .children(), в Single есть .child(), в Multi — итератор).

🧱 1.7. Цель новой системы
Цель	Описание
Compile-time безопасность	Запрещено вызывать .children() у Leaf-рендера.
Zero-cost абстракции	Без Box<dyn>, без downcast. Всё через impl Trait.
Generic оптимизация	Компилятор видит конкретный тип RenderObject и inline-ит layout.
Единый контракт Widget ↔ RenderObject	type Render связывает их на уровне типов.
Эргономичный API	Разработчик пишет fn layout(&mut self, cx: &mut LayoutCx<Self>), без ручных проверок.
Совместимость с RenderContext и PainterContext	Типы контекстов разделены, но унифицированы по generic.

Глава 2: Типовая архитектура RenderObject с Arity-контрактом
🧩 2.1. Идея: Arity как тип, а не runtime флаг

Во всех традиционных UI-фреймворках (Flutter, React, Qt) информация о детях хранится динамически:
список детей, проверки if len == 0 или 1, или > 1.

Rust же позволяет это выразить на уровне типов, то есть:

pub trait RenderObject {
type Arity: RenderArity;
fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size;
}


Теперь компилятор знает, какие методы у контекста доступны для конкретного рендера:

Leaf → нет детей;

Single → один ребёнок через cx.child();

Multi → итератор cx.children().

🧠 2.2. Тип RenderArity

Начнём с базового контракта:

/// Тип-контракт, определяющий арность RenderObject
pub trait RenderArity {
/// Тип итератора по детям в layout-фазе
type LayoutChildren<'a>: Iterator<Item = ElementId>
where Self: 'a;

    /// Тип итератора по детям в paint-фазе
    type PaintChildren<'a>: Iterator<Item = ElementId>
    where Self: 'a;

    /// Число детей (compile-time const, если известно)
    const CHILD_COUNT: Option<usize> = None;

    /// Помечаем арность для человеческого чтения
    fn name() -> &'static str;
}


Теперь реализуем три основных арности:

pub struct LeafArity;
pub struct SingleArity;
pub struct MultiArity;

🔹 LeafArity
impl RenderArity for LeafArity {
type LayoutChildren<'a> = std::iter::Empty<ElementId>;
type PaintChildren<'a>  = std::iter::Empty<ElementId>;
const CHILD_COUNT: Option<usize> = Some(0);
fn name() -> &'static str { "Leaf" }
}


➡️ Нет детей → контекст не даёт методов для layout дочерних элементов.

🔹 SingleArity
impl RenderArity for SingleArity {
type LayoutChildren<'a> = std::iter::Once<ElementId>;
type PaintChildren<'a>  = std::iter::Once<ElementId>;
const CHILD_COUNT: Option<usize> = Some(1);
fn name() -> &'static str { "Single" }
}


➡️ Один ребёнок → контекст предоставляет cx.child() и блокирует cx.children().

🔹 MultiArity
impl RenderArity for MultiArity {
type LayoutChildren<'a> = std::slice::Iter<'a, ElementId>;
type PaintChildren<'a>  = std::slice::Iter<'a, ElementId>;
const CHILD_COUNT: Option<usize> = None;
fn name() -> &'static str { "Multi" }
}


➡️ Любое число детей → контекст даёт итератор, но запрещает .child().

⚙️ 2.3. Тип RenderObject

Теперь RenderObject становится generic-контрактом с ассоциированными типами:

pub trait RenderObject: Send + Sync + 'static {
/// Тип арности
type Arity: RenderArity;

    /// Основная функция layout
    fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size;

    /// Основная функция paint
    fn paint<'a>(&self, cx: &mut PaintCx<'a, Self>);
}

💡 Пример реализации
#[derive(Debug, Clone)]
pub struct RenderOpacity {
pub opacity: f32,
}

impl RenderObject for RenderOpacity {
type Arity = SingleArity;

    fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
        let child = cx.child();
        cx.layout_child(child, cx.constraints())
    }

    fn paint<'a>(&self, cx: &mut PaintCx<'a, Self>) {
        let child = cx.child();
        if self.opacity > 0.0 {
            cx.paint_child(child);
        }
    }
}


💬 Здесь компилятор знает, что у RenderOpacity всего один ребёнок;
если вызвать cx.children() — ошибка на этапе компиляции.

🧩 2.4. Типизированные контексты: LayoutCx и PaintCx
LayoutCx
pub struct LayoutCx<'a, O: RenderObject> {
pub(crate) tree: &'a ElementTree,
pub(crate) constraints: BoxConstraints,
_phantom: std::marker::PhantomData<O>,
}


Generic по O: RenderObject → контекст знает, какая арность ему доступна.
Теперь реализуем helper-методы в зависимости от арности через специализацию.

🔹 LayoutCx для LeafArity
impl<'a, O> LayoutCx<'a, O>
where
O: RenderObject<Arity = LeafArity>,
{
pub fn constraints(&self) -> BoxConstraints { self.constraints }
pub fn layout_child(&mut self, _child: ElementId, _c: BoxConstraints) -> Size {
panic!("Leaf elements cannot layout children");
}
}

🔹 LayoutCx для SingleArity
impl<'a, O> LayoutCx<'a, O>
where
O: RenderObject<Arity = SingleArity>,
{
pub fn child(&self) -> ElementId {
self.tree.first_child()
}

    pub fn layout_child(&mut self, child: ElementId, constraints: BoxConstraints) -> Size {
        self.tree.layout(child, constraints)
    }
}

🔹 LayoutCx для MultiArity
impl<'a, O> LayoutCx<'a, O>
where
O: RenderObject<Arity = MultiArity>,
{
pub fn children(&self) -> &[ElementId] {
self.tree.children()
}

    pub fn layout_child(&mut self, child: ElementId, constraints: BoxConstraints) -> Size {
        self.tree.layout(child, constraints)
    }
}

PaintCx (аналогичная структура)
pub struct PaintCx<'a, O: RenderObject> {
pub(crate) painter: &'a egui::Painter,
pub(crate) tree: &'a ElementTree,
pub(crate) offset: Offset,
_phantom: std::marker::PhantomData<O>,
}


и такие же специализации для LeafArity, SingleArity, MultiArity.

📐 2.5. Компиляция против ошибок

Теперь компилятор отлавливает логические ошибки:

impl RenderObject for RenderPadding {
type Arity = SingleArity;

    fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
        let children = cx.children(); // ❌ compile error: no method `children` for `Single`
        ...
    }
}


Ошибка вида:

error[E0599]: no method named `children` found for struct `LayoutCx<'_, RenderPadding>` in the current scope


✅ Больше никаких runtime-if-ов, никаких ctx.children().first().

🧠 2.6. Arity и Widget связываются типами

Теперь Widget тоже знает свой RenderObject:

pub trait RenderObjectWidget: Widget {
type Render: RenderObject;
fn create_render_object(&self) -> Self::Render;
fn update_render_object(&self, render: &mut Self::Render);
}

Пример
pub struct Opacity {
pub opacity: f32,
pub child: Box<dyn DynWidget>,
}

impl RenderObjectWidget for Opacity {
type Render = RenderOpacity;

    fn create_render_object(&self) -> Self::Render {
        RenderOpacity { opacity: self.opacity }
    }

    fn update_render_object(&self, render: &mut Self::Render) {
        render.opacity = self.opacity;
    }
}


➡️ Больше нет downcast_mut.
Компилятор знает точный тип рендера.

🧩 2.7. Связь через Element

Теперь Element тоже может быть типизирован:

pub struct RenderObjectElement<W: RenderObjectWidget> {
widget: W,
render: W::Render,
}


Всё типобезопасно и inline-able.

🚀 2.8. Сводка
Компонент	Было	Стало
RenderObjectWidget	Box + downcast	типизированный Render
RenderObject	без связи с Widget	type Arity + compile-context
RenderContext	общий для всех	generic LayoutCx<'a, O> и PaintCx<'a, O>
Проверка детей	runtime	compile-time
Boilerplate	высокий	низкий
Inline возможности	отсутствуют	полные

Глава 3: Типизированные контексты LayoutCx и PaintCx в действии
🧩 3.1. Концепция: LayoutCx и PaintCx — это не просто "ctx", а типовые DSL-интерфейсы

Теперь каждый RenderObject вызывает не просто layout() и paint() с сырыми аргументами,
а работает с контекстами, которые знают допустимые действия, основанные на Arity.

fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size;
fn paint<'a>(&self, cx: &mut PaintCx<'a, Self>);


Именно в этом — главное преимущество typed-архитектуры:
никаких Option<ElementId>, Vec<ElementId>, проверок длины, ни одного .first().

⚙️ 3.2. Пример 1: RenderParagraph (Leaf Arity)

Текст — чистый leaf-рендер, не имеющий дочерних элементов.
Контекст LayoutCx не предоставляет ничего, кроме constraints() и доступа к state.

use flui_types::{Color, Rect, Size, BoxConstraints};
use crate::render::{RenderObject, LayoutCx, PaintCx};
use crate::arity::LeafArity;

#[derive(Debug, Clone)]
pub struct RenderParagraph {
pub text: String,
pub font_size: f32,
pub color: Color,
}

impl RenderParagraph {
pub fn new(text: impl Into<String>, font_size: f32, color: Color) -> Self {
Self { text: text.into(), font_size, color }
}
}

impl RenderObject for RenderParagraph {
type Arity = LeafArity;

    fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
        let constraints = cx.constraints();
        let text_len = self.text.len() as f32;
        let width = (text_len * self.font_size * 0.6).min(constraints.max_width);
        let height = self.font_size * 1.2;
        constraints.constrain(Size::new(width, height))
    }

    fn paint<'a>(&self, cx: &mut PaintCx<'a, Self>) {
        let rect = cx.bounds();
        cx.text_painter().paint_text(rect, &self.text, self.font_size, self.color);
    }
}


💡 Заметь: Leaf не имеет .child() или .children() — это compile-time-ограничение.
Если попытаться вызвать cx.child(), Rust выдаст ошибку:
method not found in LayoutCx<'_, RenderParagraph>.

⚙️ 3.3. Пример 2: RenderOpacity (Single Arity)

RenderOpacity — типичный пример Single-child рендера.
Его layout → просто проксирует constraints ребёнку,
а paint → применяет opacity к детям.

use flui_types::{Offset, Size, BoxConstraints};
use crate::render::{RenderObject, LayoutCx, PaintCx};
use crate::arity::SingleArity;

#[derive(Debug, Clone)]
pub struct RenderOpacity {
pub opacity: f32,
}

impl RenderObject for RenderOpacity {
type Arity = SingleArity;

    fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
        let child = cx.child();
        let child_size = cx.layout_child(child, cx.constraints());
        child_size
    }

    fn paint<'a>(&self, cx: &mut PaintCx<'a, Self>) {
        if self.opacity <= 0.0 { return; }

        let child = cx.child();

        cx.with_opacity(self.opacity, |cx| {
            cx.paint_child(child);
        });
    }
}


🔍 Здесь компилятор уже знает, что RenderOpacity имеет ровно одного ребёнка.
Если попробовать вызвать cx.children(), получим compile-error.

⚙️ 3.4. Пример 3: RenderFlex (Multi Arity)

RenderFlex — пример многодочернего layout-рендера, который вычисляет размеры
на основе flex-align-логики (упрощённой здесь для примера).

use flui_types::{Size, BoxConstraints, Offset};
use crate::render::{RenderObject, LayoutCx, PaintCx};
use crate::arity::MultiArity;

#[derive(Debug, Clone, Copy)]
pub enum Axis { Horizontal, Vertical }

#[derive(Debug, Clone)]
pub struct RenderFlex {
pub axis: Axis,
pub spacing: f32,
}

impl RenderFlex {
pub fn new(axis: Axis, spacing: f32) -> Self {
Self { axis, spacing }
}
}

impl RenderObject for RenderFlex {
type Arity = MultiArity;

    fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>) -> Size {
        let mut total = 0.0;
        let mut max_cross = 0.0;
        let constraints = cx.constraints();

        for child in cx.children() {
            let size = cx.layout_child(*child, constraints);
            match self.axis {
                Axis::Horizontal => {
                    total += size.width + self.spacing;
                    max_cross = max_cross.max(size.height);
                }
                Axis::Vertical => {
                    total += size.height + self.spacing;
                    max_cross = max_cross.max(size.width);
                }
            }
        }

        match self.axis {
            Axis::Horizontal => constraints.constrain(Size::new(total, max_cross)),
            Axis::Vertical => constraints.constrain(Size::new(max_cross, total)),
        }
    }

    fn paint<'a>(&self, cx: &mut PaintCx<'a, Self>) {
        let mut offset = Offset::zero();

        for child in cx.children() {
            cx.paint_child_at(*child, offset);
            offset = match self.axis {
                Axis::Horizontal => offset + Offset::new(cx.child_size(*child).width + self.spacing, 0.0),
                Axis::Vertical => offset + Offset::new(0.0, cx.child_size(*child).height + self.spacing),
            };
        }
    }
}


🧠 Здесь cx.children() возвращает итератор по ElementId,
но только потому, что type Arity = MultiArity.

🧩 3.5. LayoutCx — практическая структура
pub struct LayoutCx<'a, O: RenderObject> {
tree: &'a ElementTree,
constraints: BoxConstraints,
phantom: std::marker::PhantomData<O>,
}

impl<'a, O: RenderObject> LayoutCx<'a, O> {
pub fn constraints(&self) -> BoxConstraints { self.constraints }
}

Для LeafArity:
impl<'a, O: RenderObject<Arity = LeafArity>> LayoutCx<'a, O> {
// нет доступа к детям
}

Для SingleArity:
impl<'a, O: RenderObject<Arity = SingleArity>> LayoutCx<'a, O> {
pub fn child(&self) -> ElementId { self.tree.child(self) }
pub fn layout_child(&mut self, id: ElementId, c: BoxConstraints) -> Size {
self.tree.layout(id, c)
}
}

Для MultiArity:
impl<'a, O: RenderObject<Arity = MultiArity>> LayoutCx<'a, O> {
pub fn children(&self) -> &'a [ElementId] { self.tree.children(self) }
pub fn layout_child(&mut self, id: ElementId, c: BoxConstraints) -> Size {
self.tree.layout(id, c)
}
}

🎨 3.6. PaintCx — похожая структура, но с painter-операциями
pub struct PaintCx<'a, O: RenderObject> {
painter: &'a egui::Painter,
tree: &'a ElementTree,
offset: Offset,
phantom: std::marker::PhantomData<O>,
}


Пример API:

impl<'a, O: RenderObject<Arity = LeafArity>> PaintCx<'a, O> {
pub fn bounds(&self) -> Rect { self.tree.bounds(self) }
pub fn text_painter(&self) -> TextPainter<'a> { TextPainter::new(self.painter) }
}

impl<'a, O: RenderObject<Arity = SingleArity>> PaintCx<'a, O> {
pub fn child(&self) -> ElementId { self.tree.child(self) }
pub fn paint_child(&self, child: ElementId) {
self.tree.paint(child, self.painter, self.offset);
}
pub fn with_opacity<F: FnOnce(&mut PaintCx<'a, O>)>(&self, opacity: f32, f: F) {
self.painter.add_opacity(opacity);
f(&mut PaintCx { painter: self.painter, ..*self });
self.painter.reset_opacity();
}
}

🧩 3.7. Как выглядит RenderTree traversal

В новой системе дерево можно проходить обобщённо и безопасно:

fn perform_layout<O: RenderObject>(object: &mut O, tree: &ElementTree, c: BoxConstraints) -> Size {
let mut cx = LayoutCx::<O> { tree, constraints: c, phantom: std::marker::PhantomData };
object.layout(&mut cx)
}

fn perform_paint<O: RenderObject>(object: &O, tree: &ElementTree, painter: &egui::Painter, offset: Offset) {
let mut cx = PaintCx::<O> { painter, tree, offset, phantom: std::marker::PhantomData };
object.paint(&mut cx);
}

⚡ 3.8. Переход от старой архитектуры к новой
Было	Стало
fn layout(&self, state, constraints, ctx)	fn layout<'a>(&mut self, cx: &mut LayoutCx<'a, Self>)
ctx.children().first()	cx.child() или cx.children()
Box<dyn DynRenderObject>	impl RenderObject<Arity = _>
Downcast runtime	Compile-time generic
Manual RenderContext logic	Zero-cost typed LayoutCx
Общие painter-вызовы	Специализированный PaintCx


Глава 4: Typed Element и Typed Widget — мост между UI и Render-системой
🧩 4.1. Общая идея

В классическом Flutter:

Widget  →  Element  →  RenderObject
(immutable)   (stateful)   (layout/paint)


В FLUI typed-архитектуре:

Widget<W: RenderObjectWidget>
──▶ Element<W>
└─▶ W::Render : RenderObject

⚙️ Особенности:

Widget знает, какой тип RenderObject он создаёт;

Element параметризуется этим Widget и хранит W::Render;

всё типизировано — никаких Box<dyn> или Rc<dyn Any>.

🧱 4.2. Контракт Widget и RenderObjectWidget
pub trait Widget: Clone + Send + Sync + 'static {
fn key(&self) -> Option<Key> { None }
}


Добавляем производный тип:

pub trait RenderObjectWidget: Widget {
type Render: RenderObject;
fn create_render_object(&self) -> Self::Render;
fn update_render_object(&self, render: &mut Self::Render);
}


👉 Теперь Widget имеет ассоциированный тип Render.
компилятор знает: если W::Render = RenderOpacity, то элемент точно работает с ним.

🧩 4.3. Typed Element
pub struct Element<W: RenderObjectWidget> {
widget: W,
render: W::Render,
parent_id: Option<ElementId>,
id: ElementId,
}

🧠 Гарантии:

render — конкретный тип (RenderOpacity, RenderParagraph и т.д.),

никаких downcast,

все связи через типовую систему.

🌀 4.4. Жизненный цикл Element
Стадия	Действие
Mount	создаёт RenderObject через W::create_render_object()
Update	вызывает update_render_object() с новым Widget
Layout	вызывает render.layout() через LayoutCx
Paint	вызывает render.paint() через PaintCx
Unmount	удаляет state и рендер
⚙️ 4.5. Реализация Typed Element
impl<W: RenderObjectWidget> Element<W> {
pub fn new(widget: W, id: ElementId, parent: Option<ElementId>) -> Self {
let render = widget.create_render_object();
Self { widget, render, id, parent_id: parent }
}

    pub fn update(&mut self, new_widget: W) {
        self.widget = new_widget.clone();
        self.widget.update_render_object(&mut self.render);
    }

    pub fn layout(&mut self, tree: &ElementTree, constraints: BoxConstraints) -> Size {
        let mut cx = LayoutCx::<W::Render> { tree, constraints, phantom: std::marker::PhantomData };
        self.render.layout(&mut cx)
    }

    pub fn paint(&self, tree: &ElementTree, painter: &egui::Painter, offset: Offset) {
        let mut cx = PaintCx::<W::Render> { tree, painter, offset, phantom: std::marker::PhantomData };
        self.render.paint(&mut cx);
    }
}


💡 ни одного dyn — всё через ассоциированные типы.

🎨 4.6. Пример: Opacity Widget ↔ RenderOpacity
#[derive(Debug, Clone)]
pub struct Opacity {
pub opacity: f32,
pub child: Box<dyn Widget>,
}

impl RenderObjectWidget for Opacity {
type Render = RenderOpacity;

    fn create_render_object(&self) -> Self::Render {
        RenderOpacity { opacity: self.opacity }
    }

    fn update_render_object(&self, render: &mut Self::Render) {
        render.opacity = self.opacity;
    }
}


Теперь при layout-фазе:

let mut el = Element::new(Opacity { opacity: 0.8, child }, id, None);
el.layout(&tree, constraints);


💬 Компилятор сам понимает: у el.render — тип RenderOpacity с Arity = SingleArity.

🧠 4.7. Типовые Arity в элементах

Можно добавить type-alias’ы:

pub type LeafElement<W>   = Element<W>;
pub type SingleElement<W> = Element<W>;
pub type MultiElement<W>  = Element<W>;


или ввести глобальные типажи:

pub trait LeafElementExt {}
pub trait SingleElementExt {}
pub trait MultiElementExt {}


где возможны методы для работы с детьми, специализированные по арности.

🔄 4.8. RenderTree и Typed Traversal
pub struct RenderTree {
pub elements: HashMap<ElementId, Box<dyn Any>>,
}

impl RenderTree {
pub fn layout<W: RenderObjectWidget>(&mut self, id: ElementId, constraints: BoxConstraints) -> Size {
let element = self.elements.get_mut(&id).unwrap();
let el = element.downcast_mut::<Element<W>>().unwrap();
el.layout(self, constraints)
}

    pub fn paint<W: RenderObjectWidget>(&self, id: ElementId, painter: &egui::Painter, offset: Offset) {
        let el = self.elements[&id].downcast_ref::<Element<W>>().unwrap();
        el.paint(self, painter, offset);
    }
}


➡️ В производственной версии вместо Any можно использовать typed-эргономику через generic сборку дерева.

⚙️ 4.9. RenderObjectElement — alias-версия
pub type RenderObjectElement<W> = Element<W>;


такое имя сохраняет совместимость со стилем Flutter, но при этом в Rust типизация жёсткая и inline-friendly.

🧱 4.10. Zero-cost generic path
Операция	Старый runtime	Новый typed
layout	virtual call через dyn	monomorphized call
paint	downcast + if chains	прямой вызов
update	downcast runtime	compile-checked
child access	Option / Vec runtime	compile-checked via Arity
🧩 4.11. Бонус: автоматическая дерева-постройка

можно ввести helper builder:

pub trait WidgetExt: Widget {
fn mount(self, tree: &mut ElementTree, parent: Option<ElementId>) -> ElementId
where
Self: RenderObjectWidget,
{
let id = ElementId::new();
let element = Element::new(self, id, parent);
tree.insert(id, Box::new(element));
id
}
}


Теперь создание узла одной строкой:

let root_id = Opacity { opacity: 1.0, child: Box::new(Text::new("Hello")) }
.mount(&mut tree, None);

⚡ 4.12. Связка Widget → RenderObject через типизацию
Уровень	Тип	Назначение
Widget	RenderObjectWidget	описывает конфигурацию
Element<W>	хранит W и W::Render	связывает UI и Render
RenderObject	RenderObject<Arity>	выполняет layout и paint
LayoutCx / PaintCx	generic контексты	обеспечивают compile-гарантии
📘 4.13. Ключевые итоги

✅ Больше нет Box<dyn> и downcast;
✅ Widget, Element, RenderObject связаны типами;
✅ Ошибки арности ловятся компилятором;
✅ Монореференции zero-cost и inline;
✅ Подготовлено основание для layout-cache и diffing-оптимизаций.


🧱 Глава 4.5 — Где находится backend во FLUI-архитектуре
⚙️ 1. Что значит “backend” в контексте FLUI

В FLUI “backend” ≠ сервер.
Это скорее нижний слой движка, отвечающий за:

Задача	Компонент
🧩 управление деревом элементов	ElementTree / RenderTree
📏 планировщик layout/paint фаз	RenderPipeline
🖥 интеграция с оконной системой (egui / winit / etc.)	BackendHost
🧠 хранение состояния рендеров	RenderState
⚡ отложенные операции / input-события	TaskQueue, EventLoop
🧰 системные ресурсы (шрифты, изображения, GPU)	BackendContext
🧩 2. Базовая архитектура уровня backend
+-------------------------------------------------------------+
|                         BackendHost                         |
|-------------------------------------------------------------|
| - держит EventLoop                                          |
| - управляет RenderPipeline                                  |
| - интегрируется с egui/wgpu                                 |
| - выполняет redraw/layout                                   |
+-------------------------------------------------------------+
│
▼
+-------------------------------------------------------------+
|                     RenderPipeline                          |
|-------------------------------------------------------------|
| - планировщик layout/paint                                  |
| - кеш LayoutCache                                            |
| - хранит RenderTree и RenderState                           |
| - вызывает RenderObject::layout / paint                     |
+-------------------------------------------------------------+
│
▼
+-------------------------------------------------------------+
|                        RenderTree                           |
|-------------------------------------------------------------|
| - хранит Element<W>                                         |
| - управляет иерархией                                       |
| - предоставляет LayoutCx / PaintCx                          |
+-------------------------------------------------------------+

🧠 3. BackendHost — точка входа движка
pub struct BackendHost {
pipeline: RenderPipeline,
event_loop: EventLoop,
}

impl BackendHost {
pub fn new() -> Self {
Self {
pipeline: RenderPipeline::new(),
event_loop: EventLoop::new(),
}
}

    pub fn run(&mut self) {
        loop {
            self.event_loop.poll();
            if self.pipeline.needs_layout() {
                self.pipeline.perform_layout();
            }
            if self.pipeline.needs_paint() {
                self.pipeline.perform_paint();
            }
        }
    }
}


👉 Здесь происходит “heartbeat” фреймворка.
Каждый кадр:

собирает input-события;

запускает фазу layout → paint;

передаёт painter (например egui::Painter) в рендер-объекты.

🎨 4. RenderPipeline — планировщик фаз
pub struct RenderPipeline {
tree: ElementTree,
layout_cache: LayoutCache,
dirty_list: Vec<ElementId>,
frame: u64,
}

impl RenderPipeline {
pub fn new() -> Self {
Self {
tree: ElementTree::new(),
layout_cache: LayoutCache::new(),
dirty_list: Vec::new(),
frame: 0,
}
}

    pub fn mark_dirty(&mut self, id: ElementId) {
        self.dirty_list.push(id);
    }

    pub fn perform_layout(&mut self) {
        for id in std::mem::take(&mut self.dirty_list) {
            self.tree.layout(id);
        }
    }

    pub fn perform_paint(&mut self) {
        self.tree.paint_all();
        self.frame += 1;
    }

    pub fn needs_layout(&self) -> bool { !self.dirty_list.is_empty() }
    pub fn needs_paint(&self) -> bool { true }
}

🧩 5. ElementTree / RenderTree — ядро “backend-хранилища”
pub struct ElementTree {
elements: HashMap<ElementId, Box<dyn Any>>,
}

impl ElementTree {
pub fn new() -> Self { Self { elements: HashMap::new() } }

    pub fn insert<W: RenderObjectWidget>(&mut self, el: Element<W>) {
        self.elements.insert(el.id(), Box::new(el));
    }

    pub fn layout<W: RenderObjectWidget>(&mut self, id: ElementId) {
        let el = self.elements.get_mut(&id).unwrap();
        let el = el.downcast_mut::<Element<W>>().unwrap();
        el.layout(self, BoxConstraints::loose(Size::infinity()));
    }

    pub fn paint_all(&self) {
        for (_, any_el) in &self.elements {
            // типизация может быть решена заранее (через registry)
            // чтобы избежать downcast
        }
    }
}


🧱 ElementTree — это backend-хранилище всего визуального дерева.
Сами виджеты (Widget) живут выше, а в backend остаются элементы и рендеры.

⚙️ 6. RenderState — “RAM” слоя RenderObject

RenderState — это состояние layout/paint, которое живёт в backend.

pub struct RenderState {
pub constraints: RwLock<Option<BoxConstraints>>,
pub size: RwLock<Option<Size>>,
pub flags: AtomicRenderFlags,
}


Этим состоянием управляет pipeline через LayoutCx и PaintCx,
но хранится оно в backend-дереве (ElementTree).

🧩 7. BackendContext — мост к платформе
pub struct BackendContext {
pub painter: egui::Painter,
pub font_system: FontSystem,
pub time: Instant,
pub dpi: f32,
}


это слой, который знает:

о конкретной платформе (Egui/WGPU/Winit),

как получить Painter,

как обрабатывать input-события.

🧠 8. Сводка архитектуры уровней
Уровень	Компоненты	Описание
UI	Widgets, WidgetTree	Пользовательская логика
Element Layer	Elements, ElementTree	State + связь Widget ↔ Render
Render Layer	RenderObjects, LayoutCx, PaintCx	Layout, Paint
Backend Layer	RenderPipeline, RenderState, BackendContext	Оркестрация и низкий уровень
Platform Layer	egui, winit, GPU	Реальный рендеринг, input


🧠 Глава 5: RenderPipeline и RenderState — сердце backend-планировщика
⚙️ 5.1 Общая цель RenderPipeline

RenderPipeline — это backend-планировщик, который:

Задача	Компонент
хранит дерево	ElementTree
отслеживает dirty-узлы	DirtyList
управляет layout/paint фазами	RenderPhase
кеширует результаты	LayoutCache
синхронизирует флаги состояния	RenderState
знает, когда требуется перерисовка	needs_layout / needs_paint
🧱 5.2 Типизированная структура
use std::collections::HashMap;
use std::time::Instant;

use flui_core::{ElementId, BoxConstraints, Size};
use flui_rendering::cache::{LayoutCache, LayoutCacheKey, LayoutResult};
use flui_rendering::context::{LayoutCx, PaintCx};
use flui_rendering::RenderObject;

pub struct RenderPipeline {
tree: ElementTree,
cache: LayoutCache,
dirty_layout: Vec<ElementId>,
dirty_paint: Vec<ElementId>,
frame_start: Instant,
frame_number: u64,
}

🧩 5.3 RenderState — память одного рендера

Каждый RenderObject имеет RenderState (хранится в дереве).

use std::sync::RwLock;
use flui_core::{BoxConstraints, Size};

bitflags::bitflags! {
pub struct RenderFlags: u32 {
const NEEDS_LAYOUT = 1 << 0;
const NEEDS_PAINT  = 1 << 1;
const DIRTY        = 1 << 2;
}
}

#[derive(Default)]
pub struct RenderState {
pub constraints: RwLock<Option<BoxConstraints>>,
pub size: RwLock<Option<Size>>,
pub flags: RwLock<RenderFlags>,
}


constraints — вход layout

size — результат

flags — dirty-флаги

⚙️ 5.4 RenderPipeline::new и reset
impl RenderPipeline {
pub fn new() -> Self {
Self {
tree: ElementTree::new(),
cache: LayoutCache::new(),
dirty_layout: Vec::new(),
dirty_paint: Vec::new(),
frame_start: Instant::now(),
frame_number: 0,
}
}

    pub fn reset(&mut self) {
        self.dirty_layout.clear();
        self.dirty_paint.clear();
        self.cache.clear();
    }
}

🧠 5.5 Lifecycle: layout → paint
impl RenderPipeline {
pub fn begin_frame(&mut self) {
self.frame_start = Instant::now();
self.frame_number += 1;
}

    pub fn end_frame(&mut self) {
        let elapsed = self.frame_start.elapsed();
        tracing::info!("Frame {} took {:?}", self.frame_number, elapsed);
    }
}

🧩 5.6 layout phase
impl RenderPipeline {
pub fn perform_layout(&mut self) {
for id in std::mem::take(&mut self.dirty_layout) {
self.layout_element(id);
}
}

    fn layout_element(&mut self, id: ElementId) {
        let element = self.tree.get_mut(id);
        let constraints = BoxConstraints::tight(Size::infinity());

        let key = LayoutCacheKey::new(id, constraints);
        if let Some(result) = self.cache.get(&key) {
            if !result.needs_layout {
                element.state_mut().size.write().unwrap().replace(result.size);
                return;
            }
        }

        let size = element.render_mut().layout(&mut LayoutCx::new(&self.tree, constraints, id));
        self.cache.insert(key, LayoutResult::new(size));
        *element.state_mut().size.write().unwrap() = Some(size);
    }
}

🎨 5.7 paint phase
impl RenderPipeline {
pub fn perform_paint(&mut self, painter: &egui::Painter) {
for id in std::mem::take(&mut self.dirty_paint) {
self.paint_element(id, painter);
}
}

    fn paint_element(&self, id: ElementId, painter: &egui::Painter) {
        let element = self.tree.get(id);
        let offset = element.offset();

        let mut cx = PaintCx::new(&self.tree, painter, offset, id);
        element.render().paint(&mut cx);
    }
}

🧩 5.8 dirty tracking
impl RenderPipeline {
pub fn mark_needs_layout(&mut self, id: ElementId) {
self.dirty_layout.push(id);
}

    pub fn mark_needs_paint(&mut self, id: ElementId) {
        self.dirty_paint.push(id);
    }

    pub fn needs_layout(&self) -> bool {
        !self.dirty_layout.is_empty()
    }

    pub fn needs_paint(&self) -> bool {
        !self.dirty_paint.is_empty()
    }
}

⚡ 5.9 Интеграция с RenderState и Arity

Типизация сохраняется:

fn layout_typed<O: RenderObject>(&mut self, id: ElementId, obj: &mut O, constraints: BoxConstraints) -> Size {
let mut cx = LayoutCx::<O>::new(&self.tree, constraints, id);
obj.layout(&mut cx)
}


компилятор гарантирует, что O::Arity соответствует структуре дочерних элементов.

🧠 5.10 LayoutCache во встроенной pipeline

LayoutCacheKey включает element_id + constraints + child_count;

TTL = 60 секунд;

get_or_compute() используется в layout_element.

Благодаря этому, повторные layout-вызовы для тех же узлов становятся O(1).

🔄 5.11 Типизированный LayoutPass
pub struct LayoutPass<'a> {
pub pipeline: &'a mut RenderPipeline,
}

impl<'a> LayoutPass<'a> {
pub fn run(&mut self) {
for id in self.pipeline.tree.iter_dirty() {
self.pipeline.layout_element(id);
}
}
}


Можно иметь отдельные passes: LayoutPass, PaintPass, CompositePass.

🧩 5.12 RenderPipeline в BackendHost
pub struct BackendHost {
pub pipeline: RenderPipeline,
}

impl BackendHost {
pub fn run(&mut self, painter: &egui::Painter) {
self.pipeline.begin_frame();
if self.pipeline.needs_layout() {
self.pipeline.perform_layout();
}
if self.pipeline.needs_paint() {
self.pipeline.perform_paint(painter);
}
self.pipeline.end_frame();
}
}

🧠 5.13 Преимущества типизированного RenderPipeline
Особенность	Старый runtime	Новый typed
Downcast	при каждом layout	отсутствует
Cache-ключи	вручную	через тип Arity
State	глобальные RwLock	структурно разделён
LayoutCx / PaintCx	object-safe dyn	generic типизация
Compile safety	отсутствует	гарантирована
🧱 5.14 Расширение: Async / Offscreen Pipeline

Поскольку pipeline типизирован, можно легко внедрить offscreen-потоки:

pub fn perform_layout_parallel(&mut self) {
use rayon::prelude::*;
self.dirty_layout.par_iter()
.for_each(|&id| self.layout_element(id));
}


или offscreen-paint в GPU-буферы (через PaintCx + wgpu::CommandEncoder).

⚡ 5.15 Резюме

✅ RenderPipeline — центральный backend-планировщик;
✅ RenderState — минимальная ячейка layout/paint состояния;
✅ LayoutCache встроен в pipeline;
✅ Типизированные LayoutCx / PaintCx создаются внутри pipeline;
✅ Готово основание для многопоточности и async layout.


🖼️ Глава 5.5 — Где живут Layers и Painters в архитектуре FLUI
🧩 1. Слои уровня архитектуры (где кто живёт)

Вот полный “разрез” архитектуры движка:

┌──────────────────────────────────────────────┐
│                    APP                       │
│  ──────────────────────────────────────────  │
│  Widgets  →  Elements  →  RenderObjects      │
│                 (UI Core Layer)              │
└──────────────────────────────────────────────┘
│
▼
┌──────────────────────────────────────────────┐
│                RENDER BACKEND                │
│ ──────────────────────────────────────────── │
│ RenderPipeline, RenderState, LayoutCache     │
│ LayoutCx, PaintCx                            │
│  → orchestrates passes (layout, paint)       │
└──────────────────────────────────────────────┘
│
▼
┌──────────────────────────────────────────────┐
│          RENDER ENGINE / COMPOSITOR          │
│ ──────────────────────────────────────────── │
│  Layers (ContainerLayer, OpacityLayer, etc.) │
│  Painters (BoxPainter, ShadowPainter, etc.)  │
│  Scene, Surface, Compositor, GPUBackend      │
│  → builds render tree for final composition  │
└──────────────────────────────────────────────┘
│
▼
┌──────────────────────────────────────────────┐
│               PLATFORM IO LAYER              │
│ ──────────────────────────────────────────── │
│ Egui, Winit, WGPU, Vulkan, Metal, Skia, etc. │
│ Handles input, textures, windowing           │
└──────────────────────────────────────────────┘

🧱 2. Разделение по crate’ам (в реальном проекте)
flui/
├─ core/                // базовые типы (Size, Rect, Color, Offset)
├─ rendering/           // RenderObjects + RenderContext
│   ├─ context.rs       // LayoutCx / PaintCx
│   ├─ object.rs        // RenderObject, Arity
│   ├─ cache.rs         // LayoutCache
│   └─ pipeline.rs      // RenderPipeline
│
├─ engine/              // ← ЛОГИЧЕСКОЕ МЕСТО ДЛЯ LAYERS / PAINTERS
│   ├─ layer/
│   │   ├─ mod.rs
│   │   ├─ container.rs
│   │   ├─ opacity.rs
│   │   ├─ clip.rs
│   │   └─ image.rs
│   │
│   ├─ painter/
│   │   ├─ mod.rs
│   │   ├─ box_painter.rs
│   │   ├─ shadow_painter.rs
│   │   ├─ border_painter.rs
│   │   ├─ text_painter.rs
│   │   └─ image_painter.rs
│   │
│   ├─ compositor.rs    // сборщик слоёв в сцену
│   ├─ scene.rs         // SceneGraph (root layer)
│   ├─ surface.rs       // GPU / CPU surface
│   └─ backend.rs       // интеграция с egui / wgpu
│
├─ backend/             // событийный цикл, RenderPipeline
└─ ui/                  // Widgets, Elements

🧠 3. Кто кому принадлежит
Модуль	Ответственность	Владелец
RenderPipeline	layout/paint orchestration	backend
PaintCx	API для рендеров	rendering
Painter	низкоуровневый отрисовщик примитивов	engine
Layer	структура сцены (композитный объект)	engine
Compositor	собирает дерево слоёв в сцену	engine
Surface	реальный GPU-буфер	engine/backend совместно
🎨 4. Как это стыкуется с PaintCx

В типизированной архитектуре мы разделяем контексты:

pub struct LayoutCx<'a, R: RenderObject> {
pub tree: &'a ElementTree,
pub constraints: BoxConstraints,
pub id: ElementId,
pub phantom: std::marker::PhantomData<R>,
}

pub struct PaintCx<'a, R: RenderObject> {
pub tree: &'a ElementTree,
pub painter: &'a mut dyn Painter,
pub offset: Offset,
pub id: ElementId,
}


где Painter — уже абстракция над конкретной платформой,
которая может быть реализована через egui::Painter, wgpu, skia, и т.д.

🧩 5. Trait Painter (универсальный API для рисовальщиков)
pub trait Painter: Send {
fn rect(&mut self, rect: Rect, color: Color, radius: f32);
fn shadow(&mut self, rect: Rect, shadow: &BoxShadow);
fn text(&mut self, rect: Rect, text: &str, font_size: f32, color: Color);
fn image(&mut self, rect: Rect, texture_id: TextureId);
}


и адаптер для egui:

pub struct EguiPainter<'a> {
pub inner: &'a egui::Painter,
}

impl<'a> Painter for EguiPainter<'a> {
fn rect(&mut self, rect: Rect, color: Color, radius: f32) {
self.inner.rect_filled(rect.into(), egui::Rounding::same(radius), color.into());
}
fn shadow(&mut self, rect: Rect, shadow: &BoxShadow) {
ShadowPainter::paint(&self.inner, rect, &[shadow.clone()], Some(0.0));
}
fn text(&mut self, rect: Rect, text: &str, size: f32, color: Color) {
TextPainter::paint(&self.inner, rect, text, size, color);
}
fn image(&mut self, rect: Rect, texture_id: TextureId) {
self.inner.image(texture_id, rect.into(), egui::Color32::WHITE);
}
}

🧱 6. Layer — визуальные объекты сцены
pub trait Layer {
fn paint(&self, painter: &mut dyn Painter);
}

pub struct ContainerLayer {
pub children: Vec<Box<dyn Layer>>,
}

pub struct OpacityLayer {
pub opacity: f32,
pub child: Box<dyn Layer>,
}


каждый RenderObject::paint() теперь возвращает не просто рисует, а заполняет сцену:

fn paint(&self, state: &RenderState, cx: &mut PaintCx<Self>) {
cx.scene.push_layer(OpacityLayer {
opacity: self.opacity,
child: cx.scene.capture_child_layer(cx.id),
});
}

🧠 7. Compositor и Scene
pub struct Scene {
pub root: ContainerLayer,
}

pub struct Compositor {
pub root_scene: Scene,
}

impl Compositor {
pub fn composite(&self, painter: &mut dyn Painter) {
self.root_scene.paint(painter);
}
}

⚙️ 8. Где в цепочке живёт Painter и Layer
RenderObject.paint()
↓
PaintCx
↓
Scene.push_layer(...)
↓
Compositor.composite()
↓
Painter.rect(), text(), image()
↓
GPU / Egui / CPU surface

🔬 9. Почему это важно
Без этого:

RenderObject::paint() напрямую зовёт egui::Painter,
что делает систему негибкой, непереносимой.

С этим:

RenderObject → Scene → Layer → Compositor → Painter
можно:

отрисовать offscreen;

экспортировать в SVG/PNG;

делать эффекты (blur, transform, shader);

композировать transparency и stacking;

легко заменить backend (egui, wgpu, skia, vello, etc).

📦 10. Где реально лежат файлы
flui/
├─ engine/
│   ├─ painter/
│   │   ├─ mod.rs              // defines Painter trait
│   │   ├─ shadow.rs
│   │   ├─ border.rs
│   │   ├─ text.rs
│   │   └─ image.rs
│   │
│   ├─ layer/
│   │   ├─ mod.rs
│   │   ├─ container.rs
│   │   ├─ opacity.rs
│   │   ├─ clip.rs
│   │   └─ image.rs
│   │
│   ├─ scene.rs
│   ├─ compositor.rs
│   └─ surface.rs

⚡ 11. Концептуальное API: RenderObject → Layer
pub trait RenderObject {
type Layer: Layer;
fn layout(&mut self, cx: &mut LayoutCx<Self>) -> Size;
fn paint(&self, cx: &mut PaintCx<Self>) -> Self::Layer;
}


То есть RenderObject::paint теперь возвращает слой, а не напрямую рисует.

RenderPipeline собирает все Layer и передаёт их в Compositor.

🧩 12. Пример “в живую”
fn paint(&self, _state: &RenderState, cx: &mut PaintCx<Self>) -> Box<dyn Layer> {
let rect = cx.bounds();
let mut container = ContainerLayer::new();

    container.add(Box::new(ShadowLayer::new(rect, self.shadow.clone())));
    container.add(Box::new(BorderLayer::new(rect, self.border.clone())));

    if let Some(&child_id) = cx.children().first() {
        container.add(cx.capture_child_layer(child_id));
    }

    Box::new(container)
}

🧱 13. Кратко: кто отвечает за что
Модуль	Назначение	Пример
RenderObject	создаёт layout и scene-слой	RenderPadding
PaintCx	передаёт доступ к сцене	cx.scene.push_layer()
Layer	описывает визуальный узел	OpacityLayer, ClipLayer
Compositor	объединяет слои в кадр	scene.paint()
Painter	рисует на surface	EguiPainter, WgpuPainter
🔮 14. Итого

✅ Painters и Layers живут в отдельном crate flui-engine;
✅ RenderObject::paint() теперь строит Layer, а не рисует напрямую;
✅ Compositor превращает дерево слоёв в draw calls;
✅ Painter — адаптер к платформе (egui, wgpu, skia, etc.);
✅ всё это остаётся типизированным и безопасным.

Глава 6 — Typed RenderBackend + Layered Compositor Pipeline

ниже — «скелет» полноценно типизированного бэкенда, где RenderPipeline (layout/paint-проходы) собирает дерево слоёв (Layer), а Compositor композитит их в Surface через абстрактный Painter. всё обозначено компактно, но достаточно, чтобы собрать рабочий прототип и встроить в твои текущие RenderObject/Context.

6.1. Контракты backend-уровня
6.1.1 Surface, Frame, Backend
// flui/engine/surface.rs
use flui_types::{Rect, Size};

pub trait Surface: Send {
fn size(&self) -> Size;
fn begin_frame(&mut self) -> Box<dyn Frame>;
fn present(&mut self);
}

pub trait Frame: Send {
/// Выдаёт «рисовальщик» для этого кадра
fn painter(&mut self) -> &mut dyn crate::engine::painter::Painter;
/// Ограничение активной области (опционально)
fn set_clip(&mut self, _rect: Rect) {}
}

// Пример backend-адаптера (egui / wgpu / skia и т.д.)
pub trait RenderBackend: Send + Sync + 'static {
type Surface: Surface;

    fn create_surface(&self, width: u32, height: u32) -> Self::Surface;
}

6.1.2 Painter (унифицированный API рисования)
// flui/engine/painter/mod.rs
use flui_types::{Rect, Color, styling::BoxShadow, image::TextureId};

pub trait Painter: Send {
fn rect(&mut self, rect: Rect, color: Color, radius: f32);
fn shadow(&mut self, rect: Rect, shadow: &BoxShadow);
fn text(&mut self, rect: Rect, text: &str, size: f32, color: Color);
fn image(&mut self, rect: Rect, texture: TextureId);
}


у тебя уже есть ShadowPainter — его легко «вложить» внутрь реализации Painter (через адаптер к egui), или оставить utility и вызывать из реализации.

6.2. SceneGraph и Layers (визуальная часть)
6.2.1 Базовый слой
// flui/engine/layer/mod.rs
use crate::engine::painter::Painter;

pub trait Layer: Send {
fn paint(&self, p: &mut dyn Painter);
}

pub struct ContainerLayer {
pub children: Vec<Box<dyn Layer>>,
}

impl ContainerLayer {
pub fn new() -> Self { Self { children: Vec::new() } }
pub fn push(&mut self, layer: Box<dyn Layer>) { self.children.push(layer); }
}

impl Layer for ContainerLayer {
fn paint(&self, p: &mut dyn Painter) {
for child in &self.children {
child.paint(p);
}
}
}

6.2.2 Пара примеров слоёв
// flui/engine/layer/opacity.rs
use super::Layer;
use crate::engine::painter::Painter;

pub struct OpacityLayer {
pub opacity: f32,
pub child: Box<dyn Layer>,
}

impl Layer for OpacityLayer {
fn paint(&self, p: &mut dyn Painter) {
// В простом варианте: просто рисуем ребёнка.
// Для реального блендинга нужен offscreen pass (см. §6.5).
self.child.paint(p);
}
}

// flui/engine/layer/rect.rs
use super::Layer;
use flui_types::{Rect, Color};
use crate::engine::painter::Painter;

pub struct RectLayer {
pub rect: Rect,
pub color: Color,
pub radius: f32,
}

impl Layer for RectLayer {
fn paint(&self, p: &mut dyn Painter) {
p.rect(self.rect, self.color, self.radius);
}
}

6.3. Scene и Compositor
// flui/engine/scene.rs
use super::layer::{Layer, ContainerLayer};

pub struct Scene {
pub root: ContainerLayer,
}

impl Scene {
pub fn new() -> Self { Self { root: ContainerLayer::new() } }
pub fn push(&mut self, l: Box<dyn Layer>) { self.root.push(l); }
}

// flui/engine/compositor.rs
use super::{scene::Scene, surface::Surface};

pub struct Compositor;

impl Compositor {
pub fn composite(&mut self, scene: &Scene, surface: &mut dyn Surface) {
let mut frame = surface.begin_frame();
let painter = frame.painter();
scene.root.paint(painter);
drop(painter);
surface.present();
}
}

6.4. Точка сборки: RenderPipeline ↔ Scene

Здесь мы стыкуем твои RenderObject/RenderState/RenderContext c построением Scene.

// flui/rendering/pipeline.rs
use std::sync::Arc;
use parking_lot::RwLock;

use crate::{ElementTree, ElementId, BoxConstraints, RenderFlags};
use crate::render::{LayoutCx, PaintCx}; // типизированные контексты из предыдущей главы

use flui_engine::{
scene::Scene,
compositor::Compositor,
surface::Surface,
};

pub struct RenderPipeline {
pub tree: Arc<RwLock<ElementTree>>,
pub compositor: Compositor,
}

impl RenderPipeline {
pub fn new(tree: Arc<RwLock<ElementTree>>) -> Self {
Self { tree, compositor: Compositor }
}

    /// 1) layout-проход
    pub fn layout(&mut self, root: ElementId, constraints: BoxConstraints) {
        let tree = self.tree.clone();
        let mut guard = tree.write();
        // рекурсивный layout сверху вниз; твой RenderContext уже умеет
        guard.layout_subtree(root, constraints);
        // выставление флагов, кэш, и т.д.
    }

    /// 2) paint-проход: строим сцену
    pub fn build_scene(&mut self, root: ElementId) -> Scene {
        let tree = self.tree.clone();
        let guard = tree.read();

        let mut scene = Scene::new();
        // Реальный обход: DFS/stack; собираем из каждого RenderObject слой(я)
        guard.paint_into_scene(root, &mut scene);
        scene
    }

    /// 3) composite в Surface
    pub fn render_frame(&mut self, surface: &mut dyn Surface, root: ElementId, constraints: BoxConstraints) {
        self.layout(root, constraints);
        let scene = self.build_scene(root);
        self.compositor.composite(&scene, surface);
    }
}


ElementTree::{layout_subtree, paint_into_scene} — это удобные методы-обходы твоего уже существующего дерева элементов. Внутри они создают LayoutCx/PaintCx и вызывают RenderObject::layout(...) / RenderObject::paint(...) -> Box<dyn Layer> (см. ниже).

6.5. Изменение сигнатуры paint (типобезопасно)

В прошлой главе мы обсудили, что RenderObject::paint лучше возвращать слой, а не напрямую звать egui. Теперь покажу, как это выглядит в коде:

// flui/rendering/object.rs (ядро RenderObject)
use flui_types::Size;
use crate::render::{LayoutCx, PaintCx};

pub trait RenderObject: 'static + Send {
type Layer: flui_engine::layer::Layer;

    fn layout(&mut self, cx: &mut LayoutCx<Self>) -> Size;

    /// Новый контракт: строим слой
    fn paint(&self, cx: &mut PaintCx<Self>) -> Box<Self::Layer>;
}


В ElementTree:

// flui/rendering/tree_paint.rs (псевдокод адаптера)
use flui_engine::{scene::Scene, layer::ContainerLayer};

impl ElementTree {
pub fn paint_into_scene(&self, root: ElementId, scene: &mut Scene) {
let mut stack: Vec<ElementId> = vec![root];

        while let Some(id) = stack.pop() {
            let elem = self.get(id).unwrap();
            if let Some(ro) = elem.render_object() {
                // Сначала дети → потом текущий, или наоборот (в зависимости от твоей модели)
                // Собираем слой текущего узла
                let mut cx = PaintCx::new(self, id);
                let layer = ro.paint(&mut cx); // -> Box<dyn Layer>

                // Добавляем в сцену/контейнер
                scene.push(layer);

                // Обойти детей:
                for c in elem.children_iter() {
                    stack.push(c);
                }
            }
        }
    }
}


можно строить иерархию слоёв на лету (например, ContainerLayer на каждый RenderObject и вкладывать туда слои детей), либо делать двухфазную сборку: сперва дети возвращают слои, потом родитель их оборачивает. Выбирай в зависимости от того, как у тебя организована z-order/clip/transform.

6.6. Пример: RenderPadding → Layer
// у тебя уже есть RenderPadding c layout; добавим paint:
use flui_engine::layer::{Layer, ContainerLayer, rect::RectLayer};
use flui_types::{Rect, Color, Size};

impl RenderObject for RenderPadding {
type Layer = dyn Layer;

    fn layout(&mut self, cx: &mut LayoutCx<Self>) -> Size {
        // как у тебя: deflate constraints → layout child → size = child + padding
        // ...
        cx.set_size(size);
        size
    }

    fn paint(&self, cx: &mut PaintCx<Self>) -> Box<Self::Layer> {
        // вариант 1: просто вернуть слой ребёнка, сдвинув offset через PaintCx API
        // вариант 2: вернуть ContainerLayer с декоративными подслоями
        let mut container = ContainerLayer::new();

        // например: фон/рамка/тень (если надо)
        if let Some(bg) = cx.style().background_color {
            container.push(Box::new(RectLayer {
                rect: cx.bounds(), color: bg, radius: cx.style().border_radius,
            }));
        }

        // затем — ребёнок
        if let Some(child_id) = cx.single_child() {
            let child_layer = cx.capture_child_layer(child_id); // API PaintCx
            container.push(child_layer);
        }

        Box::new(container)
    }
}

6.7. Пример: RenderOpacity → OpacityLayer
use flui_engine::layer::{Layer, opacity::OpacityLayer};

impl RenderObject for RenderOpacity {
type Layer = dyn Layer;

    fn layout(&mut self, cx: &mut LayoutCx<Self>) -> Size {
        // просто прокинуть constraints ребёнку; size = child
        cx.layout_single_child_passthrough()
    }

    fn paint(&self, cx: &mut PaintCx<Self>) -> Box<Self::Layer> {
        if let Some(child) = cx.single_child_layer() {
            Box::new(OpacityLayer { opacity: self.opacity(), child })
        } else {
            // пустой контейнер
            Box::new(crate::engine::layer::ContainerLayer::new())
 
        }
    }
}


когда появится реальный offscreen-композит, внутри OpacityLayer::paint ты переключишься на offscreen/alpha-blend (с Surface/Frame), не меняя контрактов.

6.8. Где подключается твой LayoutCache

остаётся в rendering/cache.rs

LayoutCx при старте layout-узла собирает ключ (включая child_count для multi-child), пробует layout_cache().get(key), иначе — вычисляет и insert.

никаких изменений в backend/engine не нужно.

6.9. Типобезопасность детей (Arity) — краткий рекап

из предыдущей главы:

pub trait SingleChild {}
pub trait MultiChild {}
pub trait Leaf {}

pub trait RenderObject: Send + 'static {
type Arity: ?Sized; // SingleChild/MultiChild/Leaf
type Layer: Layer;
fn layout(&mut self, cx: &mut LayoutCx<Self>) -> Size;
fn paint(&self, cx: &mut PaintCx<Self>) -> Box<Self::Layer>;
}


LayoutCx<RO: RenderObject<Arity = SingleChild>> даёт только single_child()/set_single_child_layout(...).
PaintCx — аналогично. Это снимает риск «по ошибке обойти всех детей» у single-child рендера.