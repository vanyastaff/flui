# Enum Widget Architecture Design

## 🎯 Цели

1. ✅ Устранить blanket impl конфликты
2. ✅ Сохранить Flutter-like API (StatelessWidget, StatefulWidget, etc.)
3. ✅ Консистентность с enum Element
4. ✅ Ясная семантика для разных типов виджетов
5. ✅ Простой API для пользователей

## 📊 Архитектура

### Core Enum

```rust
/// Widget - unified enum для всех типов виджетов
///
/// Это основной тип для виджетов в Flui. Вместо trait hierarchy,
/// мы используем enum с разными вариантами для разных типов виджетов.
#[derive(Debug)]
pub enum Widget {
    /// Stateless widget - чистая функция от конфигурации к UI
    Stateless(Box<dyn StatelessWidget>),

    /// Stateful widget - имеет изменяемое состояние
    Stateful(Box<dyn StatefulWidget>),

    /// Inherited widget - предоставляет данные потомкам
    Inherited(Box<dyn InheritedWidget>),

    /// RenderObject widget - прямое управление layout/paint
    RenderObject(Box<dyn RenderObjectWidget>),

    /// ParentData widget - прикрепляет метаданные к потомкам
    ParentData(Box<dyn ParentDataWidget>),
}
```

### Widget Traits (object-safe)

```rust
/// StatelessWidget - виджет без изменяемого состояния
pub trait StatelessWidget: Debug + Send + Sync + 'static {
    /// Построить дерево виджетов
    fn build(&self, ctx: &BuildContext) -> Widget;

    /// Опциональный ключ для идентификации
    fn key(&self) -> Option<Key> {
        None
    }

    /// Клонировать в Box
    fn clone_boxed(&self) -> Box<dyn StatelessWidget>;

    /// Проверка возможности обновления
    fn can_update(&self, other: &dyn StatelessWidget) -> bool {
        self.type_id() == other.type_id()
    }

    /// Downcast support
    fn as_any(&self) -> &dyn Any;
    fn type_id(&self) -> TypeId {
        self.as_any().type_id()
    }
}

/// StatefulWidget - виджет с изменяемым состоянием
pub trait StatefulWidget: Debug + Send + Sync + 'static {
    /// Создать начальное состояние
    fn create_state(&self) -> Box<dyn State>;

    /// Опциональный ключ
    fn key(&self) -> Option<Key> {
        None
    }

    /// Клонировать в Box
    fn clone_boxed(&self) -> Box<dyn StatefulWidget>;

    /// Downcast support
    fn as_any(&self) -> &dyn Any;
    fn type_id(&self) -> TypeId {
        self.as_any().type_id()
    }
}

/// State - состояние для StatefulWidget
pub trait State: Debug + Send + Sync + 'static {
    /// Построить UI с доступом к состоянию
    fn build(&mut self, ctx: &BuildContext) -> Widget;

    /// Жизненный цикл: инициализация
    fn init_state(&mut self, ctx: &BuildContext) {}

    /// Жизненный цикл: widget обновился
    fn did_update_widget(&mut self, old_widget: &dyn StatefulWidget, ctx: &BuildContext) {}

    /// Жизненный цикл: очистка
    fn dispose(&mut self) {}

    /// Пометить для пересборки
    fn set_state(&mut self, f: impl FnOnce(&mut Self)) {
        f(self);
        // TODO: mark dirty
    }

    /// Downcast support
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// InheritedWidget - предоставляет данные вниз по дереву
pub trait InheritedWidget: Debug + Send + Sync + 'static {
    /// Дочерний виджет
    fn child(&self) -> &Widget;

    /// Проверка, нужно ли уведомлять зависимых
    fn update_should_notify(&self, old: &dyn InheritedWidget) -> bool;

    fn key(&self) -> Option<Key> {
        None
    }

    fn clone_boxed(&self) -> Box<dyn InheritedWidget>;
    fn as_any(&self) -> &dyn Any;
}

/// RenderObjectWidget - создает RenderObject
pub trait RenderObjectWidget: Debug + Send + Sync + 'static {
    /// Создать RenderObject
    fn create_render_object(&self, ctx: &BuildContext) -> Box<dyn RenderObject>;

    /// Обновить существующий RenderObject
    fn update_render_object(&self, ctx: &BuildContext, render_object: &mut dyn RenderObject);

    /// Дочерние виджеты (для MultiChildRenderObjectWidget)
    fn children(&self) -> Option<&[Widget]> {
        None
    }

    /// Один дочерний виджет (для SingleChildRenderObjectWidget)
    fn child(&self) -> Option<&Widget> {
        None
    }

    fn key(&self) -> Option<Key> {
        None
    }

    fn clone_boxed(&self) -> Box<dyn RenderObjectWidget>;
    fn as_any(&self) -> &dyn Any;
}

/// ParentDataWidget - прикрепляет метаданные к потомкам
pub trait ParentDataWidget: Debug + Send + Sync + 'static {
    fn child(&self) -> &Widget;
    fn apply_parent_data(&self, render_object: &mut dyn RenderObject);

    fn key(&self) -> Option<Key> {
        None
    }

    fn clone_boxed(&self) -> Box<dyn ParentDataWidget>;
    fn as_any(&self) -> &dyn Any;
}
```

### Widget Enum Implementation

```rust
impl Widget {
    /// Создать Stateless widget
    pub fn stateless(widget: impl StatelessWidget) -> Self {
        Widget::Stateless(Box::new(widget))
    }

    /// Создать Stateful widget
    pub fn stateful(widget: impl StatefulWidget) -> Self {
        Widget::Stateful(Box::new(widget))
    }

    /// Создать Inherited widget
    pub fn inherited(widget: impl InheritedWidget) -> Self {
        Widget::Inherited(Box::new(widget))
    }

    /// Создать RenderObject widget
    pub fn render_object(widget: impl RenderObjectWidget) -> Self {
        Widget::RenderObject(Box::new(widget))
    }

    /// Создать ParentData widget
    pub fn parent_data(widget: impl ParentDataWidget) -> Self {
        Widget::ParentData(Box::new(widget))
    }

    /// Получить ключ виджета
    pub fn key(&self) -> Option<Key> {
        match self {
            Widget::Stateless(w) => w.key(),
            Widget::Stateful(w) => w.key(),
            Widget::Inherited(w) => w.key(),
            Widget::RenderObject(w) => w.key(),
            Widget::ParentData(w) => w.key(),
        }
    }

    /// Проверка возможности обновления
    pub fn can_update(&self, other: &Widget) -> bool {
        match (self, other) {
            (Widget::Stateless(a), Widget::Stateless(b)) => a.can_update(&**b),
            (Widget::Stateful(a), Widget::Stateful(b)) => a.type_id() == b.type_id(),
            (Widget::Inherited(a), Widget::Inherited(b)) => a.type_id() == b.type_id(),
            (Widget::RenderObject(a), Widget::RenderObject(b)) => a.type_id() == b.type_id(),
            (Widget::ParentData(a), Widget::ParentData(b)) => a.type_id() == b.type_id(),
            _ => false,
        }
    }

    /// Клонировать виджет
    pub fn clone_widget(&self) -> Widget {
        match self {
            Widget::Stateless(w) => Widget::Stateless(w.clone_boxed()),
            Widget::Stateful(w) => Widget::Stateful(w.clone_boxed()),
            Widget::Inherited(w) => Widget::Inherited(w.clone_boxed()),
            Widget::RenderObject(w) => Widget::RenderObject(w.clone_boxed()),
            Widget::ParentData(w) => Widget::ParentData(w.clone_boxed()),
        }
    }

    /// Downcast к конкретному типу
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        match self {
            Widget::Stateless(w) => w.as_any().downcast_ref(),
            Widget::Stateful(w) => w.as_any().downcast_ref(),
            Widget::Inherited(w) => w.as_any().downcast_ref(),
            Widget::RenderObject(w) => w.as_any().downcast_ref(),
            Widget::ParentData(w) => w.as_any().downcast_ref(),
        }
    }

    /// Проверка типа
    pub fn is<T: 'static>(&self) -> bool {
        self.downcast_ref::<T>().is_some()
    }
}

impl Clone for Widget {
    fn clone(&self) -> Self {
        self.clone_widget()
    }
}
```

## 🔧 Использование

### Простой Stateless Widget

```rust
#[derive(Debug, Clone)]
struct HelloWorld {
    name: String,
}

impl StatelessWidget for HelloWorld {
    fn build(&self, ctx: &BuildContext) -> Widget {
        Widget::render_object(Text::new(format!("Hello, {}!", self.name)))
    }

    fn clone_boxed(&self) -> Box<dyn StatelessWidget> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// Использование:
let widget = Widget::stateless(HelloWorld { name: "Flui".into() });
```

### Stateful Widget

```rust
#[derive(Debug, Clone)]
struct Counter {
    initial: i32,
}

#[derive(Debug)]
struct CounterState {
    count: i32,
}

impl StatefulWidget for Counter {
    fn create_state(&self) -> Box<dyn State> {
        Box::new(CounterState { count: self.initial })
    }

    fn clone_boxed(&self) -> Box<dyn StatefulWidget> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl State for CounterState {
    fn build(&mut self, ctx: &BuildContext) -> Widget {
        Widget::stateless(Column::new(vec![
            Widget::render_object(Text::new(format!("Count: {}", self.count))),
            Widget::stateless(Button::new("Increment", |state: &mut CounterState| {
                state.count += 1;
            })),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// Использование:
let widget = Widget::stateful(Counter { initial: 0 });
```

### Inherited Widget

```rust
#[derive(Debug, Clone)]
struct Theme {
    primary_color: Color,
    child: Widget,
}

impl InheritedWidget for Theme {
    fn child(&self) -> &Widget {
        &self.child
    }

    fn update_should_notify(&self, old: &dyn InheritedWidget) -> bool {
        if let Some(old_theme) = old.as_any().downcast_ref::<Theme>() {
            self.primary_color != old_theme.primary_color
        } else {
            true
        }
    }

    fn clone_boxed(&self) -> Box<dyn InheritedWidget> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// Helper для доступа:
impl Theme {
    pub fn of(ctx: &BuildContext) -> Color {
        ctx.depend_on_inherited_widget::<Theme>()
            .map(|theme| theme.primary_color)
            .unwrap_or(Color::BLACK)
    }
}
```

## 🎨 Helper Macros (опционально)

Для уменьшения boilerplate можно добавить макросы:

```rust
/// Автоматически реализует методы для StatelessWidget
#[macro_export]
macro_rules! impl_stateless_widget {
    ($type:ty) => {
        impl StatelessWidget for $type {
            fn clone_boxed(&self) -> Box<dyn StatelessWidget> {
                Box::new(self.clone())
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
    };
}

// Использование:
#[derive(Debug, Clone)]
struct MyWidget { /* ... */ }

impl MyWidget {
    fn build(&self, ctx: &BuildContext) -> Widget {
        // ...
    }
}

impl_stateless_widget!(MyWidget);
```

## 📝 Миграционный путь

### Этап 1: Новый enum Widget
1. Создать новый файл `widget_enum.rs` с enum Widget
2. Оставить старые traits как deprecated
3. Добавить `#[allow(deprecated)]` в старый код

### Этап 2: Обновить core widgets
1. Migрировать базовые виджеты (Text, Container, Row, Column)
2. Обновить examples
3. Протестировать

### Этап 3: Обновить Element
1. Element уже enum, обновить для работы с enum Widget
2. Обновить ElementTree

### Этап 4: Cleanup
1. Удалить deprecated traits
2. Удалить старые derive macros
3. Финальное тестирование

## ✅ Преимущества

1. **Нет blanket impl конфликтов** - enum не trait
2. **Консистентность** - Widget и Element оба enum
3. **Exhaustive matching** - компилятор проверяет все варианты
4. **Семантическая ясность** - Widget::Stateless vs Widget::Stateful
5. **Простой downcast** - встроенный в enum
6. **Клонирование** - явный метод clone_widget()
7. **Type safety** - match гарантирует обработку всех вариантов

## ⚠️ Trade-offs

1. **Dynamic dispatch** - все через dyn Trait (но это уже было в DynWidget)
2. **Box allocation** - каждый виджет в Box (но это уже было в BoxedWidget)
3. **Clone требует clone_boxed()** - но это явно и понятно

## 🚀 Производительность

- **Сравнимо с текущим DynWidget** - тот же dynamic dispatch
- **Лучше чем Box<dyn Widget>** - enum меньше indirection
- **Match оптимизируется** - компилятор оптимизирует exhaustive match
- **Element уже enum** - такой же подход уже работает

## 📚 Примеры из других фреймворков

- **Yew (Rust)**: VNode enum для разных типов узлов
- **Dioxus (Rust)**: VNode enum
- **React (TS)**: ReactElement тип с разными вариантами

Enum Widget - это стандартный pattern для UI фреймворков!
