//! Basic Widget Implementations
//!
//! Практические примеры реализации базовых виджетов.
//! Эти примеры показывают реальную структуру виджетов.

use flui_core::{BuildContext, Element};
use flui_core::view::{View, ChangeFlags};
use flui_core::hooks::{use_signal, Signal};

use super::mock_render::*;

// ============================================================================
// Button - Интерактивная кнопка
// ============================================================================

/// Простая кнопка с callback
///
/// # Example
///
/// ```rust,ignore
/// Button::new("Click me", |_| {
///     println!("Button clicked!");
/// })
/// ```
#[derive(Clone)]
pub struct Button {
    text: String,
    on_click: Option<Box<dyn Fn() + 'static>>,
    enabled: bool,
}

impl Button {
    pub fn new(text: impl Into<String>, on_click: impl Fn() + 'static) -> Self {
        Self {
            text: text.into(),
            on_click: Some(Box::new(on_click)),
            enabled: true,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl std::fmt::Debug for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Button")
            .field("text", &self.text)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl PartialEq for Button {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text && self.enabled == other.enabled
    }
}

impl View for Button {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // В реальной реализации:
        // 1. Создаём ButtonRenderElement с текстом и стилем
        // 2. Регистрируем on_click handler
        // 3. Применяем enabled состояние

        // let render = ButtonRenderElement::new()
        //     .text(self.text)
        //     .enabled(self.enabled)
        //     .on_click(self.on_click);

        let element = create_button_element(self.text.clone(), self.enabled);
        (element, ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Пересобираем если текст или enabled изменились
        if self != *prev {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE
        }
    }
}

// ============================================================================
// TextField - Поле ввода текста
// ============================================================================

/// Текстовое поле с двусторонней привязкой
///
/// # Example
///
/// ```rust,ignore
/// let text = use_signal(ctx, String::new());
/// TextField::new("Enter name", text)
/// ```
#[derive(Clone)]
pub struct TextField {
    label: String,
    value: Signal<String>,
    placeholder: Option<String>,
}

impl TextField {
    pub fn new(label: impl Into<String>, value: Signal<String>) -> Self {
        Self {
            label: label.into(),
            value,
            placeholder: None,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }
}

impl std::fmt::Debug for TextField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextField")
            .field("label", &self.label)
            .field("placeholder", &self.placeholder)
            .finish()
    }
}

impl View for TextField {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // В реальной реализации:
        // 1. Создаём TextFieldRenderElement
        // 2. Показываем label
        // 3. Устанавливаем placeholder
        // 4. Подключаем двустороннюю привязку к signal

        // let value_clone = self.value.clone();
        // let render = TextFieldRenderElement::new()
        //     .label(self.label)
        //     .placeholder(self.placeholder)
        //     .value(self.value.get())
        //     .on_change(move |new_value| {
        //         value_clone.set(new_value);
        //     });

        let element = create_textfield_element(
            self.label.clone(),
            self.value.get(),
            self.placeholder.clone()
        );
        (element, ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // Пересобираем если изменился label или placeholder
        if self.label != prev.label || self.placeholder != prev.placeholder {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            // Signal сам отследит изменения value
            ChangeFlags::NONE
        }
    }
}

// ============================================================================
// Checkbox - Переключатель
// ============================================================================

/// Checkbox с label
///
/// # Example
///
/// ```rust,ignore
/// let checked = use_signal(ctx, false);
/// Checkbox::new("Accept terms", checked)
/// ```
#[derive(Clone)]
pub struct Checkbox {
    label: String,
    checked: Signal<bool>,
}

impl Checkbox {
    pub fn new(label: impl Into<String>, checked: Signal<bool>) -> Self {
        Self {
            label: label.into(),
            checked,
        }
    }
}

impl std::fmt::Debug for Checkbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checkbox")
            .field("label", &self.label)
            .finish()
    }
}

impl View for Checkbox {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // let checked_clone = self.checked.clone();
        // let render = CheckboxRenderElement::new()
        //     .label(self.label)
        //     .checked(self.checked.get())
        //     .on_toggle(move || {
        //         checked_clone.update(|v| !v);
        //     });

        let element = create_checkbox_element(self.label.clone(), self.checked.get());
        (element, ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        if self.label != prev.label {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE
        }
    }
}

// ============================================================================
// Padding - Отступы вокруг ребёнка
// ============================================================================

/// Виджет, добавляющий отступы вокруг дочернего виджета
///
/// # Example
///
/// ```rust,ignore
/// Padding::all(10.0)
///     .child(Button::new("Click", || {}))
/// ```
#[derive(Clone)]
pub struct Padding {
    padding: f32,
    child: Option<Box<dyn View<Element = Element, State = ()>>>,
}

impl Padding {
    pub fn all(padding: f32) -> Self {
        Self {
            padding,
            child: None,
        }
    }

    pub fn child(mut self, child: impl View<Element = Element, State = ()> + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }
}

impl std::fmt::Debug for Padding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Padding")
            .field("padding", &self.padding)
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

impl View for Padding {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // let child_element = self.child
        //     .map(|child| child.build(ctx).0);

        // let render = PaddingRenderElement::new()
        //     .padding(self.padding)
        //     .child(child_element);

        let element = create_padding_element(self.padding, None);
        (element, ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        if self.padding != prev.padding {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE
        }
    }
}

// ============================================================================
// Row - Горизонтальный контейнер
// ============================================================================

/// Горизонтальное расположение виджетов
///
/// # Example
///
/// ```rust,ignore
/// Row::new()
///     .spacing(5.0)
///     .child(Button::new("Yes", || {}))
///     .child(Button::new("No", || {}))
/// ```
#[derive(Clone)]
pub struct Row {
    children: Vec<Box<dyn View<Element = Element, State = ()>>>,
    spacing: f32,
}

impl Row {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            spacing: 0.0,
        }
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn child(mut self, child: impl View<Element = Element, State = ()> + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn children(
        mut self,
        children: impl IntoIterator<Item = impl View<Element = Element, State = ()> + 'static>
    ) -> Self {
        self.children.extend(
            children.into_iter().map(|c| Box::new(c) as Box<dyn View<Element = Element, State = ()>>)
        );
        self
    }
}

impl std::fmt::Debug for Row {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Row")
            .field("spacing", &self.spacing)
            .field("child_count", &self.children.len())
            .finish()
    }
}

impl View for Row {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // let child_elements: Vec<_> = self.children
        //     .into_iter()
        //     .map(|child| child.build(ctx).0)
        //     .collect();

        // let render = RowRenderElement::new()
        //     .spacing(self.spacing)
        //     .children(child_elements);

        let element = create_row_element(self.spacing, self.children.len());
        (element, ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        if self.spacing != prev.spacing || self.children.len() != prev.children.len() {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE
        }
    }
}

// ============================================================================
// Column - Вертикальный контейнер
// ============================================================================

/// Вертикальное расположение виджетов
///
/// # Example
///
/// ```rust,ignore
/// Column::new()
///     .spacing(10.0)
///     .child(TextField::new("Name", name))
///     .child(TextField::new("Email", email))
///     .child(Button::new("Submit", || {}))
/// ```
#[derive(Clone)]
pub struct Column {
    children: Vec<Box<dyn View<Element = Element, State = ()>>>,
    spacing: f32,
}

impl Column {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            spacing: 0.0,
        }
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn child(mut self, child: impl View<Element = Element, State = ()> + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn children(
        mut self,
        children: impl IntoIterator<Item = impl View<Element = Element, State = ()> + 'static>
    ) -> Self {
        self.children.extend(
            children.into_iter().map(|c| Box::new(c) as Box<dyn View<Element = Element, State = ()>>)
        );
        self
    }
}

impl std::fmt::Debug for Column {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Column")
            .field("spacing", &self.spacing)
            .field("child_count", &self.children.len())
            .finish()
    }
}

impl View for Column {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // let child_elements: Vec<_> = self.children
        //     .into_iter()
        //     .map(|child| child.build(ctx).0)
        //     .collect();

        // let render = ColumnRenderElement::new()
        //     .spacing(self.spacing)
        //     .children(child_elements);

        let element = create_column_element(self.spacing, self.children.len());
        (element, ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        if self.spacing != prev.spacing || self.children.len() != prev.children.len() {
            element.mark_dirty();
            ChangeFlags::NEEDS_BUILD
        } else {
            ChangeFlags::NONE
        }
    }
}

// ============================================================================
// Практический пример: Форма логина
// ============================================================================

/// Полноценная форма логина, использующая все базовые виджеты
///
/// # Example
///
/// ```rust,ignore
/// let login_form = LoginForm::new();
/// ```
#[derive(Debug, Clone)]
pub struct LoginForm;

impl View for LoginForm {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Состояние формы
        let _email = use_signal(ctx, String::new());
        let _password = use_signal(ctx, String::new());
        let _remember_me = use_signal(ctx, false);

        // Валидация
        let _is_valid = use_signal(ctx, false);

        // Обработчик отправки
        // let email_clone = email.clone();
        // let password_clone = password.clone();

        // Создаём UI:
        // Column::new()
        //     .spacing(10.0)
        //     .child(TextField::new("Email", email)
        //         .placeholder("user@example.com"))
        //     .child(TextField::new("Password", password)
        //         .placeholder("••••••••"))
        //     .child(Checkbox::new("Remember me", remember_me))
        //     .child(Row::new()
        //         .spacing(5.0)
        //         .child(Button::new("Login", move || {
        //             println!("Login: {}, {}", email_clone.get(), password_clone.get());
        //         }).enabled(is_valid.get()))
        //         .child(Button::new("Cancel", || {
        //             println!("Cancelled");
        //         })))

        let element = create_column_element(10.0, 4);
        (element, ())
    }
}

// ============================================================================
// Практический пример: Счётчик с историей
// ============================================================================

/// Счётчик, который показывает историю изменений
///
/// Демонстрирует:
/// - Множественные signals
/// - use_effect для обновления истории
/// - Динамический список
#[derive(Debug, Clone)]
pub struct CounterWithHistory;

impl View for CounterWithHistory {
    type Element = flui_core::element::ComponentElement;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        let _count = use_signal(ctx, 0);
        let history = use_signal(ctx, Vec::<i32>::new());

        // Обновляем историю при изменении счётчика
        // use_effect(ctx, move || {
        //     let current = count.get();
        //     history.update(|h| {
        //         h.push(current);
        //         if h.len() > 10 {
        //             h.remove(0);
        //         }
        //     });
        //     None
        // });

        // let count_inc = count.clone();
        // let count_dec = count.clone();

        // Column::new()
        //     .spacing(10.0)
        //     .child(Label::new(format!("Count: {}", count.get())))
        //     .child(Row::new()
        //         .spacing(5.0)
        //         .child(Button::new("+", move || count_inc.update(|n| n + 1)))
        //         .child(Button::new("-", move || count_dec.update(|n| n - 1))))
        //     .child(Label::new("History:"))
        //     .children(history.get().iter().map(|n| {
        //         Label::new(n.to_string())
        //     }))

        let element = create_column_element(10.0, 3 + history.get().len());
        (element, ())
    }
}
