# semantics-model — дизайн (уровень 1)

- **Статус:** утверждено владельцем 2026-10-12; работы — [tasks.md](tasks.md).
- **Дата:** 2026-10-12
- **База:** `main` @ `e32e77583` и ветка `claude/ally-w3c-standards-e435c0`.
- **Требования:** [requirements.md](requirements.md) R1–R26, D1–D4, Q1–Q4.

## Итог

FLUI владеет своей типизированной моделью семантики. Модель — одна структура атрибутов,
сгруппированных по смыслу, с типизированными состояниями; она одна и та же в конфигурации
render-объекта, в узле дерева и при публикации. Встреча с AccessKit — ровно в одном модуле
трансляции. Словарь ролей и действий в `flui-protocol` становится полным и выровненным по
WAI-ARIA 1.2 и AccessKit. Правила слияния — одна таблица по группам. Добавление свойства
проверяется компилятором: слияние и трансляция разбирают структуру исчерпывающим
деструктурированием, и новое поле без правила не компилируется.

Удаляются: `SemanticsProperties`, флаговое слово `SemanticsFlag`/`SemanticsFlags`,
`SemanticsNodeData`, `SemanticsNodeSnapshot`/`SemanticsSnapshot`, `SemanticsEvent`.
Не меняются: сборка в `flui-rendering` (тот же `absorb`/`is_compatible_with`), `SemanticsTree`,
инкрементальная публикация `SemanticsOwner`, ADR-0205.

Три необратимых решения оформляются ADR до кода: модель и слияние (ADR-0202), словарь
протокола (ADR-0203), синтетические узлы и текстовая модель (ADR-0204).

## Текущее состояние

Факты — в requirements.md, «Текущее состояние». Здесь — только то, на что опирается дизайн.

- Конфигурация — единственный вход: `RenderObject::describe_semantics_configuration(&mut
  SemanticsConfiguration)` (`crates/flui-rendering/src/traits/render_object.rs`), реализуют
  `RenderParagraph` и `RenderSemanticsAnnotations`. Сборка вызывает только `absorb`,
  `is_compatible_with`, `set_index_in_parent`, `set_scroll_child_count`
  (`crates/flui-rendering/src/pipeline/owner/semantics.rs:695`, `:715`, `:723`, `:1034`, `:1067`).
- Трансляция (`accesskit_translation.rs`) получает `SemanticsNodeData` — копию конфигурации,
  собранную `SemanticsTree::node_data_of`. `SemanticsSnapshot` строится только тестами.
- Протокол: `SemanticsRole` (34 значения, `vocabulary!`, числовые значения стабильны) и
  `SemanticsAction` (27 битов) — в `crates/flui-protocol/src/semantics.rs`; отдельно
  `flui_protocol::wire::Role` — словарь агента, свёрнутый под UI Automation (ADR-0080, ADR-0095).
- Границы графем и слов: `flui-painting/src/text_boundaries.rs` (`graphemes`, `word_at`, ICU4X);
  в контроллере есть своя `next_word_boundary` (`crates/flui-widgets/src/text/controller.rs:1074`).
- Геометрия текста: `TextPainter::get_boxes_for_selection`
  (`crates/flui-painting/src/text_painter/paint.rs:84`), `line_metrics`
  (`parley_text/caret.rs:311`).

## Варианты

| | A. AccessKit-native | **B. Своя модель + одна трансляция** | C. Ядро + escape hatch |
|---|---|---|---|
| Суть | конфигурация оборачивает `accesskit::Node` | типизированные атрибуты FLUI, отображение в одном модуле | B для частого, `FnMut(&mut accesskit::Node)` для редкого |
| Покрытие словаря | сразу полное | по мере объявления (вся ARIA в этой спеке) | быстрое |
| Стабильность публичного API после 0.2 | нет: AccessKit `0.x` ломается каждый релиз и протекает в виджеты и `flui-sdk` | да | нет: escape hatch протекает |
| Слияние | над непрозрачным узлом, без типов | таблица по группам, типы | не определено для hatch |
| Невалидные состояния | представимы | непредставимы в авторском API | представимы |
| Web-мост (D2) | читает AccessKit | читает модель или AccessKit | читает AccessKit |
| Цена | минимальная сейчас, растущая при каждом bump | таблица отображения (локальна, закрыта тестом) | обе |

**Выбран B (D4).** A проигрывает на главном нефункциональном требовании — стабильный API.
C — шов (seam) ради одного адаптера: его содержимое не видят ни слияние, ни тесты.

## (a) Модель: `SemanticsAttributes`

Публикуемая полезная нагрузка узла — одна структура. Конфигурация = директивы сборки +
атрибуты + обработчики действий.

```rust
// flui-semantics::configuration
pub struct SemanticsConfiguration {
    assembly: AssemblyDirectives,   // boundary / merge_descendants / explicit_children / blocks_user_actions
    attributes: SemanticsAttributes,
    handlers: ActionHandlers,       // SemanticsAction -> Arc<dyn Fn(ActionArgs) + Send + Sync>
    annotated: bool,
}

// flui-semantics::model — публичные типы, поля приватные, чтение через методы
pub struct SemanticsAttributes {
    role: Option<SemanticsRole>,          // None: роль выводится (статичный текст / контейнер)
    naming: Naming,                       // label, description, value, placeholder, role_description,
                                          // tooltip, keyboard_shortcut, access_key, language, url
    states: States,                       // все ARIA-состояния, каждое Option<T>
    range: Option<NumericRange>,          // существующий проверенный тип
    live: Option<Live>,                   // politeness + atomic
    collection: Option<Box<Collection>>,  // позиция в наборе, уровень, таблица/сетка, сортировка
    text: Option<Box<TextContent>>,       // только у полей и абзацев, (h)
    scroll: Option<ScrollMetrics>,        // ADR-0157: ось, позиция, границы, scroll_index
    relations: Relations,                 // ADR-0205: anchors + (SemanticsRelation, anchor)
    traversal: Traversal,                 // sort key, text direction
    custom_actions: SmallVec<[CustomSemanticsAction; 2]>,
    tags: SmallVec<[SemanticsTag; 2]>,
    platform_view: Option<PlatformViewId>,
}
```

**Состояния** — `Option<T>`: `None` — не объявлено, `Some(false)` — объявлено ложью.
Это различие нужно слиянию: два узла, объявивших одно состояние, конфликтуют (сегодняшнее
`HasEnabledState` у `ListTile` и `Radio`, ARCHITECTURE.md решение 1).

```rust
pub struct States {
    toggled: Option<Toggled>,        // Toggled { Off, On, Mixed }: checked у отмечаемых ролей,
                                     // pressed у Button — смысл задаёт роль, как в AccessKit и Chromium
    selected: Option<bool>,
    expanded: Option<bool>,
    disabled: Option<bool>,
    read_only: Option<bool>,
    required: Option<bool>,
    invalid: Option<Invalid>,        // Invalid { True, Grammar, Spelling }
    busy: Option<bool>,
    modal: Option<bool>,
    hidden: Option<bool>,
    focusable: Option<bool>,
    focused: Option<bool>,
    visited: Option<bool>,
    multiselectable: Option<bool>,
    current: Option<Current>,        // Page, Step, Location, Date, Time, True
    has_popup: Option<HasPopup>,     // Menu, Listbox, Tree, Grid, Dialog
    orientation: Option<Orientation>,
    autocomplete: Option<AutoComplete>,
}
```

Перечисления повторяют ARIA, а не AccessKit: они наши и стабильны; трансляция переводит их.
`aria-checked` и `aria-pressed` в ARIA — разные состояния, но и Chromium (`CheckedState` +
роль `kToggleButton`), и AccessKit (`Toggled`, «interpreted based on the node's role») хранят
одно значение, смысл которого задаёт роль. FLUI делает так же: отдельное `pressed` дало бы
состояние, которое трансляция всё равно сливает.

**Правило слияния живёт на типе состояния**, как у `CheckedState`/`Tristate` во Flutter:

```rust
pub trait StateMerge: Copy {
    /// Объявлено обоими — значит отдельные узлы при обычном слиянии.
    fn conflicts(a: Option<Self>, b: Option<Self>) -> bool { a.is_some() && b.is_some() }
    /// Принудительное слияние: документированный приоритет типа.
    fn merge(receiver: Option<Self>, contributor: Option<Self>) -> Option<Self>;
}
// Toggled: Mixed > On > Off; bool-состояния: получатель, если объявлен, иначе вкладчик.
```

Таблица (d) ссылается на это правило, а не повторяет его.
Редкие группы (`collection`, `text`) — в `Box`, чтобы узел без них не рос (R22: размер
`SemanticsConfiguration` замеряется тестом `size_of` только как бюджет в бенчмарке, не как
контракт — правило тестов из AGENTS.md).

**Исчерпывающее деструктурирование — механизм R19.** `merge`, `is_compatible`, `translate` и
`validate` начинаются с `let SemanticsAttributes { role, naming, states, … } = self;` без `..`,
и так же внутри каждой группы. Новое поле не компилируется, пока его не учли все четыре.

## (b) Роли и протокол (ADR-0203)

`SemanticsRole` в `flui-protocol` — плоское перечисление (стабильные значения и wire-имена,
`vocabulary!`), но полное: каждая неабстрактная роль ARIA 1.2 с аналогом в AccessKit плюс
desktop-роли. Интерактивные виды перестают быть флагами: `Button`, `CheckBox`, `RadioButton`,
`Switch`, `Link`, `Slider`, `SpinButton`, `Image`, `Heading`, `TextInput`, `MultilineTextInput`,
`PasswordInput`, `SearchInput`, `EmailInput`, `NumberInput`, `PhoneNumberInput`, `UrlInput`,
`DateInput`, `TimeInput`, … Полный список и отображение — «Матрица покрытия» ниже.

**Q1: роль с данными или плоская роль.** Протокол остаётся плоским: числовое значение и
wire-имя роли должны быть стабильны и сериализуемы без полезной нагрузки, а `aria-level`
в ARIA один на заголовок, элемент списка, строку и элемент дерева. Поэтому уровень — атрибут
`Collection::level`. Невалидные сочетания закрываются так:

1. **Авторский API типизирован для частых форм:** `Semantics::heading(HeadingLevel)`,
   `checkbox(Toggled)`, `toggle_button(Toggled)`, `switch(bool)`, `slider(NumericRange)`, `text_input(TextInputKind)`,
   `list_item(index)` выставляют роль и её данные вместе.
2. **Остальное проверяет `validate`** при публикации по таблице применимости ARIA
   (`applicable(role, attribute) -> bool`): неприменимое не публикуется, `tracing::warn!` один
   раз на изменение узла (R3). Это требование ARIA 1.2 к пользовательскому агенту («MUST ignore
   non-global states and properties» на роли, которая их не поддерживает), и Chromium делает так
   же — фильтрует при сериализации, без DCHECK. Harness `flui-testing` переводит предупреждение
   в падение теста, чтобы ошибку автора видел его тест, а не приложение пользователя.
   Предупреждения копятся во время публикации (с дедупликацией по узлу и атрибуту) и
   выдаются после того, как владелец зафиксировал публикацию и отпустил все заимствования:
   подписчик `tracing` — пользовательский код и может войти в того же владельца. Тест —
   повторный вход через публичный handle.
3. **Имя у роли, где оно запрещено** (`paragraph`, `code` и т.п. по ARIA 1.2), переносится в
   описание, а не отбрасывается — правило Chromium (`GetProhibitedNameError`): текст не
   теряется для пользователя. Узел без роли (`generic`, `presentation`/`none`) публикуется с
   именем как статичный текст (`Label`), а не как `GenericContainer` с описанием:
   `accesskit_consumer::common_filter` исключает такой контейнер, и его описание не
   наследуется детьми.

`role: None` — роль не объявлена: узел с именем публикуется как статичный текст (`Label`, как
сейчас, ARCHITECTURE.md решение 4), без имени — как `GenericContainer`; корень окна — `Window`.

`wire::Role` агента остаётся отдельным словарём (Q4): он описывает то, что видит desktop-сервер
через UI Automation, и сравнивается между бэкендами (ADR-0095 §4). Он получает новые значения,
где UIA их различает (`heading`, `tree`, `grid`), а `wire_role` пересчитывается из новой матрицы.

## (c) Действия

`SemanticsAction` выравнивается по `accesskit::Action`: `Click`, `Focus`, `Blur`, `Expand`,
`Collapse`, `Increment`, `Decrement`, `ScrollUp/Down/Left/Right`, `ScrollIntoView`,
`SetScrollOffset`, `SetTextSelection`, `ReplaceSelectedText`, `SetValue`, `SetNumericValue`,
`ShowContextMenu`, `ShowTooltip`, `HideTooltip`, `CustomAction`. Удаляются `LongPress` (это
`ShowContextMenu`), `MoveCursor*` (это `SetTextSelection`), `DidGain/LoseAccessibilityFocus`
(понятие Android без пути в AccessKit), `Copy`, `Cut`, `Paste`, `Dismiss` (нет пути).

Обработчик получает `ActionCall` — действие вместе ровно с теми данными, которые оно
принимает, так что несовпадение действия и данных невыразимо:

```rust
pub enum ActionCall {
    Click, Focus, Blur, Expand, Collapse, Increment, Decrement,
    ScrollUp(ScrollUnit), ScrollDown(ScrollUnit), ScrollLeft(ScrollUnit), ScrollRight(ScrollUnit),
    ScrollIntoView(Option<Reveal>),          // None — сам узел; Some — предок раскрывает потомка (ADR-0168)
    SetScrollOffset(Offset),
    SetTextSelection(TextSelection),         // позиции в единицах редактора (h)
    ReplaceSelectedText(String),
    SetValue(String), SetNumericValue(f64),  // ADR-0124: число проверяется по диапазону
    ShowContextMenu, ShowTooltip, HideTooltip,
    CustomAction(CustomActionId),
}
```

- `SetValue` и `SetNumericValue` — два действия FLUI, оба публикуются как `SetValue`
  AccessKit: какое из них узел принимает, решает, что он публикует (паттерн значения UIA и
  записываемость диапазона).
- `ScrollUnit { Item, Page }` из `ActionData::ScrollUnit` доходит до обработчика; без данных —
  `Page`, как у адаптеров, которые его не шлют. `ActionData::ScrollHint` у `ScrollIntoView` —
  пожелание о положении цели; FLUI раскрывает с минимальным сдвигом и подсказку не передаёт
  (записанное ограничение, не потеря действия).
- Запрос, чьи данные не подходят действию или отсутствуют, отклоняется при декодировании
  (`ActionDecodeError`) до вызова обработчика.
- `ScrollToPoint` (точка в пикселях окна, переводу нужен масштаб презентации) и
  `SetSequentialFocusNavigationStartingPoint` (точка обхода фокуса, а не обработчик узла) не
  маршрутизируются: у focus-keyboard нет API точки начала обхода. Каждое получает маршрут с
  первым потребителем — отдельной работой focus-keyboard с end-to-end тестом действия; до
  тех пор декодер отклоняет их как `Unrouted`, а не теряет молча.

## (d) Слияние (ADR-0202)

Одна таблица политик, одна функция на группу.

| Группа / поле | Обычное слияние | Принудительное (`MergeSemantics`) |
|---|---|---|
| `role` | конфликт, если объявлены обе | порядок специфичности (ниже) |
| `naming.label`, `naming.description` | конкатенация с учётом направления | то же |
| `naming.value` | конфликт, если обе непусты | получатель |
| прочие `naming` | первый выигрывает | то же |
| каждое поле `states` | `StateMerge::conflicts` типа | `StateMerge::merge` типа |
| `range`, `collection`, `text`, `scroll`, `live`, `platform_view` | конфликт, если оба есть | получатель |
| `relations`, `tags` | объединение | то же |
| `custom_actions` | объединение по `CustomActionId`, каждое со своим обработчиком | то же; действие, чей обработчик не сохранить, не публикуется |
| `traversal` | первый выигрывает | то же |
| `handlers` | объединение; один и тот же action у обоих — конфликт | получатель; `blocks_user_actions` фильтрует вклад |

Пользовательские действия делят одно `SemanticsAction::CustomAction`, поэтому их обработчики
хранятся по `CustomActionId`, а не по действию: иначе слитый узел публиковал бы действие
вкладчика и отдавал его чужому обработчику.

**Порядок специфичности ролей** при принудительном слиянии переносит ARCHITECTURE.md решения
1–2 с флагов на роли: авторская структурная роль (вкладка, элемент списка, диалог, …) >
отмечаемые (`Switch`, `RadioButton`, `CheckBox`) > `Button` > `Link`, текстовые поля,
`Slider` > содержимое (`Image`, `Heading`, текстовые роли вроде `Paragraph`); при равенстве
остаётся роль получателя. Это одна функция и одна таблица, а не каскад `if` по битам.

Порядок специфичности — решение FLUI, а не перенос: Compose и Flutter при слиянии оставляют
роль родителя или первую найденную. Здесь это дало бы `Button` для плитки с радио, что
закреплено как ошибка (`merge_semantics_over_a_tile_and_radio_announces_as_one_radio_button`).

**Вложенный merger остаётся отдельным узлом.** Узел, который сам сливает потомков
(`MergeSemantics` внутри `MergeSemantics`), не поглощается внешним: это отдельная единица
фокуса — правило Compose (`SemanticsNode.mergeConfig` пропускает таких детей). Сейчас сборка
поглощает всё под `merge_into_ancestor`
(`crates/flui-rendering/src/pipeline/owner/semantics.rs:532`), так что это изменение поведения:
входит в W3 с тестом «карточка со вложенной плиткой даёт два узла» («Риски» 1).

Каждая строка — строка табличного теста `merge_policy_table` (одиночное, конфликт,
принудительное). Поведение сегодняшних фикстур (`ListTile` + `Radio`, ARCHITECTURE.md
решение 1) закрепляется теми же тестами в `packages/flui-material/tests/list_tile.rs`.

## (e) Трансляция и матрица покрытия

`translate.rs` — единственный модуль, импортирующий `accesskit` для узлов (агент и адаптеры
получают уже переведённое).

- `role_for(SemanticsRole) -> accesskit::Role` — `match` по всем вариантам; тест
  `every_role_maps_to_accesskit` обходит `SemanticsRole::ALL` (перечисление
  `#[non_exhaustive]` в другом крейте, поэтому полноту ловит тест, а не компилятор).
  `ALL` выводится из самого перечисления и не доказывает полноту словаря, поэтому
  `every_aria_role_is_covered` сверяет его с независимым списком: роли ARIA 1.2, переписанные
  из спецификации, каждая — либо `SemanticsRole`, либо в списке исключённых с причиной
  (абстрактная, устаревшая, без аналога в AccessKit).
- Атрибуты переводятся исчерпывающим деструктурированием (R19).
- `validate` — таблица применимости ARIA (b).
- **Матрица покрытия** — табличный тест: для каждой роли и каждого атрибута ожидаемое свойство
  AccessKit и то, что о нём говорит `accesskit_consumer` (имя по AccName, `is_invocable`,
  фильтр). Проверяет то, что увидит адаптер, а не поля.
- **Сверка версий** — расширение `the_role_fold_was_transcribed_from_the_locked_windows_adapter`
  на `accesskit` и `accesskit_consumer`: bump проваливает тест до пересмотра матрицы (R21).

## (f) Дерево, владелец, сборка

- `SemanticsNode` хранит `SemanticsConfiguration`; публикация читает
  `PublishView<'a> { attributes: &'a SemanticsAttributes, rect, transform, children: SmallVec<[NodeId; 4]> }`
  без копии атрибутов. `SemanticsNodeData` и `SemanticsTree::node_data` удаляются.
- `SemanticsSnapshot` удаляется: межпоточная передача — это `accesskit::TreeUpdate`
  (ADR-0082). Проверки структуры (`DuplicateAccessibilityIdentity`, `RepeatedNode`) остаются
  внутренними проверками владельца с теми же ошибками.
- `SemanticsOwner`: дифф с зеркалом, индексы фокуса и связей без изменений; добавляются
  синтетические узлы (h) и объявитель (j).
- Сборка (`flui-rendering`) меняет только типы вызовов `set_index_in_parent` →
  `set_position_in_set`, `set_scroll_child_count` → размер набора у контейнера (i).

## (g) Связи

ADR-0205 без изменений; `Relations` — группа атрибутов. Новый вид связи добавляется, когда его
начинает читать зафиксированный адаптер (сегодня `LabelledBy`, `Controls`).

## (h) Синтетические узлы и текстовая модель (ADR-0204)

**Синтетические id (Q2).** Прогоны текста и объявитель — узлы, у которых нет render-объекта.
`RenderId` упакован `(generation << 32) | index` с `generation ≥ 1`, поэтому значения
`1..2³²` не выдаются ни одному render-объекту. Владелец семантики раздаёт их из своего
счётчика. Ключ прогона — его полная идентичность: родитель (`AccessibilityNodeId` поля),
порядковый номер, начало в поле и текст. Пока все четыре совпадают, id тот же — неизменённые
прогоны сохраняют id между публикациями; любое изменение даёт новое значение, так что
правка не отдаёт старый id другому тексту. Освобождённое значение не переиспользуется за
время жизни владельца; исчерпание отказывает навсегда (новые прогоны не публикуются;
ARCHITECTURE «Identity is not a label»). Сообщение об исчерпании — одно `tracing::error!` —
ставится в очередь вместе с состоянием публикации и выдаётся после того, как владелец
отпустил заимствования, как и предупреждения `validate` (b); тест исчерпания входит повторно
через публичный handle. Входящее действие на синтетический id маршрутизируется родителю по
обратной карте.

Так резервируют диапазон все проверенные системы: Chromium — отрицательные id от
`-1000000001` для inline text box, Flutter — синтетические id движка от `1 << 16`, Slint —
`sub_index << 38 | parent_id` с `sub_index ≥ 1`. Masonry делит счётчик с виджетами и держит
`TODO` о непересекающемся пуле — этот вариант не годится.

**Поддеревья AccessKit (`tree_id`) отвергнуты по факту.** В зафиксированных версиях они
работают на трёх desktop-адаптерах, но `TextPosition` — голый `NodeId` без дерева:
`accesskit_consumer` 0.39 ищет прогон в дереве поля (`text.rs:22-41`) и падает на `unwrap()`
(`text.rs:1448-1480`), если прогоны в поддереве. Поддеревья остаются на будущее для встроенных
деревьев (webview).

**TextContent.** Производит render-объект, владеющий раскладкой (`RenderEditable`, позже
`RenderParagraph`):

```rust
pub struct TextContent {
    runs: Box<[TextRun]>,              // по строке и направлению; перенос строки — последний символ прогона
    selection: Option<TextSelection>,  // anchor/focus в (номер прогона, индекс символа)
}
pub struct TextRun {
    text: SmolStr,                     // для пароля — маска
    character_lengths: Box<[u8]>,      // байты UTF-8 на графему (единица AccessKit)
    character_positions: Box<[f32]>,  // от начала прогона, по направлению
    character_widths: Box<[f32]>,
    word_starts: Box<[u8]>,            // индексы графем
    direction: TextDirection,
    rect: Rect,
}
```

- Прогон — непрерывный кусок одной строки, одного направления и не длиннее **256 символов
  AccessKit** — считаются опубликованные элементы `character_lengths`, а не графемы, так что
  разрезанный длинный кластер тоже входит в предел (тест — такой кластер на границе 256):
  `word_starts` в AccessKit — `u8`. Так режут Parley (`MAX_CHARACTERS_PER_SPAN = u8::MAX + 1`),
  egui и Slint. Прогоны — дети поля в том же дереве (у всех проверенных систем).
- Прогоны одной визуальной строки связаны взаимными `next_on_line`/`previous_on_line`:
  `accesskit_consumer` определяет начало и конец строки по их отсутствию, и без ссылок каждая
  граница прогона (смена направления, предел длины) читалась бы как новая строка. Тест —
  строка со сменой направления или длиннее 256 символов читается как одна строка.
- AccessKit не умеет запрашивать прогоны лениво (у Chromium — `kLoadInlineTextBoxes`, у
  Compose — запрос `GetTextLayoutResult`), поэтому они публикуются вместе с полем; стоимость
  ограничивается тем, что прогоны строят только поля ввода (Q5) и только при изменении текста
  или раскладки.
- Графемы и слова — из **одного** источника, `flui-painting::text_boundaries` (ICU4X), тем же,
  что использует навигация редактора: `crates/flui-widgets/src/text/controller.rs` строит
  остановки Ctrl+стрелок (`next_word_boundary`, `prev_word_boundary`) поверх
  `word_segments_from`. Публикуемые `word_starts` вызывают ту же политику, а не копию, и
  помощники контроллера остаются (иначе Narrator и Ctrl+стрелки разойдутся — «Риски» 2).
- `character_lengths` в AccessKit — `u8`: кластер длиннее 255 байт UTF-8 публикуется как
  несколько символов, разрезанных по границам `char` (каждый ≤ 255 байт), так что длины
  по-прежнему суммируются в длину прогона; грубее становятся только позиции внутри кластера.
- Идентичность прогона: прогон сохраняет синтетический id, пока он тот же — тот же порядковый
  номер, то же начало в поле и тот же текст; иначе получает новый (ADR-0204). Вставка перед
  существующими прогонами перевыдаёт их id, и устаревшая `TextPosition` отклоняется, а не
  указывает на чужой текст.
- Позиции и ширины — из раскладки (`get_boxes_for_selection` по графеме, кешируется до
  следующего `layout`), не из оценки.
- Пароль (R13): прогон строится по маске, длины — по маске; число символов совпадает.
- Пустое поле публикует один пустой прогон (нулевые длины, геометрия каретки): у выделения
  AccessKit должен быть узел роли `TextRun`, иначе каретку пустого поля нельзя ни прочитать,
  ни сдвинуть. Тест — пустое поле в фокусе, каретка читается и двигается через consumer.
- Входящие `SetTextSelection`/`ReplaceSelectedText`/`SetValue` переводятся из
  (прогон, символ) в байтовые смещения редактора и идут тем же путём правки, что клавиатура
  (R14, text-ime). Оба конца выделения сначала проверяются: их прогоны должны быть текущими
  детьми целевого поля (владелец и поколение id), иначе запрос отклоняется типизированной
  ошибкой — прогон чужого поля или устаревший конец не применяются к целевому редактору. Тесты
  — конец из другого поля и смесь живого и устаревшего концов.
- Символ AccessKit внутри разрезанного длинного кластера — не граница графемы: такой индекс
  прижимается к началу кластера (для правого конца — к концу), так что выделение и правка
  всегда ложатся на границы графем редактора. Тест — выделение внутри такого кластера.
- Инкрементальность: грязный родитель перестраивает свои прогоны; прогоны диффятся с зеркалом
  по своим синтетическим id; удаление родителя чистит их (R22).

## (i) Коллекции

```rust
pub struct Collection {
    position_in_set: Option<usize>,      // у элемента; с нуля, как в AccessKit
    set_size: Option<SetSize>,           // у контейнера: Known(n) | Unknown
    level: Option<NonZeroU32>,           // aria-level: заголовок, элемент дерева/списка, строка
    table: Option<TableShape>,           // rows/cols: Known | Unknown — у таблицы/сетки
    cell: Option<CellPosition>,          // row, col, row_span, col_span — у ячейки/заголовка
    sort: Option<SortDirection>,         // у заголовка столбца
}
```

- Соглашения AccessKit 0.25: `position_in_set` отсчитывается с нуля (`accesskit_windows`
  прибавляет единицу), а `size_of_set` ищется `accesskit_consumer` у предков — поэтому
  размер лежит на контейнере, а не на элементе.
- Позиция, которую сегодня ставит сборка из `IndexedSemantics`
  (`semantics.rs:1034`, `:1067`), становится `position_in_set`; размер контейнера —
  `scroll_child_count`, а если он неизвестен — `SetSize::Unknown`, не число построенных строк
  (R17).
- Сегодня сборка ставит и индекс, и `scroll_child_count` на конфигурацию **элемента**, так что
  одного переименования мало: W6 меняет поток данных сборки — размер набора записывается на
  узел контейнера (ближайший предок-список/прокрутку, который формирует узел), индекс — на
  элемент. Тест публикует пару предок/элемент и читает «N из M» через
  `accesskit_consumer`.
- Неизвестный размер не должен наследоваться: если `size_of_set` у контейнера просто опущен,
  `accesskit_consumer::size_of_set_from_container` идёт дальше вверх и найдёт размер внешнего
  известного набора. Поэтому у элементов вложенного набора с `SetSize::Unknown` не публикуется
  и `position_in_set`: без позиции потребитель не строит «N из M» с чужим M.
  Тест — неизвестный внутренний набор внутри известного внешнего:
  элементы внутреннего не читаются как «N из размера внешнего».
- AccessKit получает `position_in_set`/`size_of_set`, `level`, `row_count`/`column_count`,
  `row_index`/`column_index`, `row_span`/`column_span`, `sort_direction`.

## (j) Live-регионы и объявления

- `Live { setting: LiveSetting, atomic: bool }`, `LiveSetting { Off, Polite, Assertive }`.
  Неявные значения по ARIA 1.2 (R8): `Alert` — assertive и atomic, `Status` — polite и
  atomic, `Log` — polite, `Timer` и `Marquee` — off. Явное значение побеждает. `Off`
  публикуется явно: при отсутствии свойства потребители AccessKit наследуют настройку
  родителя, и `Timer` внутри polite-региона остался бы живым. `busy` — состояние (R10).
- Срочность объявления — отдельный тип `Urgency { Polite, Assertive }`: у объявления нет
  значения `Off`, а у объявителя ровно два узла.
- **Live-регион — основной путь, объявление — вторичный.** Android в API 36 объявил
  `announceForAccessibility` устаревшим в пользу live-регионов; у AccessKit 0.25 нет API
  объявлений вовсе (`accesskit_windows` поднимает только `UIA_LiveRegionChangedEventId`).
- **Объявитель (Q3)** повторяет рекомендованный AccessKit приём (`example_common`): владелец
  держит два синтетических узла роли `Label` — polite и assertive — последними детьми корня;
  `SemanticsOwner::announce(message, Urgency)` пишет текст в `value` нужного узла (macOS и
  iOS реагируют на смену `value`, не `label`: AccessKit #765). Сообщения одного кадра
  объединяются, повтор того же текста чередует невидимый разделитель, чтобы адаптер увидел
  изменение. Runtime (`SemanticsHost::announce`) получает production-путь через presentation;
  `SemanticsEvent` удаляется.
- **Доставка объявления.** Принятое объявление — долг доставки: сначала фиксируется в очереди
  владельца (приём до пробуждения), затем презентация запрашивает кадр, иначе в простое без
  кадров текст не прозвучал бы до случайной перерисовки. Неудачный или заменённый запрос кадра
  не стирает долг: очередь остаётся, и следующий кадр её публикует; старое успешное
  пробуждение не снимает новое обязательство. Тест — объявление в простаивающей презентации
  доходит до значения узла без иной перерисовки.
- **Ограничение.** UIA Notification (`UiaRaiseNotificationEvent`, приоритет и прерывание, как
  `ariaNotify` в Chromium) через AccessKit недоступен. Записывается как известное ограничение и
  кандидат во вклад в AccessKit; своего моста ради него не делаем (D2).

## (k) Авторский API

`SemanticsConfiguration` и виджет `Semantics` получают сеттеры по группам с типами из (a), и
типизированные конструкторы частых форм из (b). Конфигурации без роли и с единственным
обработчиком (помощники `GestureDetector`, `Focus`) остаются как сейчас
(`is_actions_only`, `is_focus_state_only`). Миграция существующих вызовов в виджетах
механическая (D1): `.button(true)` → `.role(SemanticsRole::Button)` и т.п., без изменения
поведения; тесты виджетов — оракул.

Пакеты зависят только от `flui-sdk` (ADR-0088), поэтому каждый тип, которым настраивают
семантику (`Toggled`, `ActionCall`, `SetSize`, `AttributeError`, …), реэкспортируется через
`flui-widgets` и тем самым через `flui_sdk::widgets`, и получает строку в
`crates/flui-sdk/tests/surface.rs`. Это часть той работы, что вводит тип, а не отдельный шаг.

## Публичный контракт (эскиз)

```rust
// flui-protocol
#[non_exhaustive] pub enum SemanticsRole { /* полный словарь, value(), name(), ALL */ }
#[non_exhaustive] pub enum SemanticsAction { /* выровнен с AccessKit, value(), name(), ALL */ }

// flui-semantics
pub struct SemanticsAttributes { /* (a), чтение методами */ }
#[non_exhaustive] pub enum Toggled { Off, On, Mixed }
#[non_exhaustive] pub enum Urgency { Polite, Assertive }
pub struct SetSize { /* Known(n) | Unknown, у контейнера */ }
#[non_exhaustive] pub enum AttributeError { NonFinite, PositionOutOfSet, /* … */ }
impl SemanticsConfiguration {
    pub fn set_role(&mut self, role: SemanticsRole);
    pub fn set_toggled(&mut self, toggled: Toggled);
    pub fn set_position_in_set(&mut self, index: usize);
    // … по группам
}
impl SemanticsOwner {
    pub fn announce(&mut self, message: &str, urgency: Urgency);
}
```

## Инварианты и владение

- Атрибуты — данные без обработчиков, `Clone + PartialEq + Send + Sync`; обработчики живут
  в конфигурации, в публикацию не попадают.
- Синтетические id принадлежат владельцу; не переиспользуются; исчерпание — постоянный отказ.
- Якоря держат свою аллокацию (ADR-0205).
- Трансляция — чистая функция `(PublishView, &AnchorIndex, &SyntheticIds) -> accesskit::Node`.
- Путь кадра синхронный, без блокировок; обработчики вызываются вне заимствований дерева
  (как сейчас в `resolve_action`).

## Ошибки и отказы

| Отказ | Поведение |
|---|---|
| неконечное или вне-диапазонное значение при задании | `AttributeError`, прежнее значение остаётся |
| атрибут неприменим к роли | не публикуется, `warn!` один раз на изменение узла |
| якорь без цели | связь опускается (ADR-0205) |
| исчерпание синтетических id | новые прогоны и объявления не публикуются, `error!` один раз |
| паника обработчика действия | PANIC-POLICY и существующее сдерживание владельца (R25) |
| паника колбэка платформы при публикации | без изменений: зеркало не продвигается, повтор доставляет |

## Тестовая стратегия

- **Юнит, в `src`** (приватный шов): `merge_policy_table`, `validate`-таблица применимости,
  синтетические id (выдача, неповторение, исчерпание на границе счётчика).
- **Интеграционные, `tests/`** через публичный API владельца: матрица покрытия через
  `accesskit_consumer`, инкрементальные прогоны текста и объявитель (по образцу
  `relations_resolve_through_anchors_across_publication`), отказы.
- **Сборка** (`flui-rendering/tests/semantics_assembly.rs`): позиции наборов, виртуализация.
- **Регрессия поведения**: существующие тесты виджетов и пять тестов ветки — оракул миграции;
  каждый проверяется «с откатом исправления» по AGENTS.md.
- **Нативно** (R26): фикстуры `examples/a11y_probe` по семействам ролей и проверки
  `cargo xtask device windows-a11y`: тип элемента, `AriaRole`, `AriaProperties`, шаблоны,
  `TextPattern` для прогонов.

## ADR

Приняты 2026-10-12: [ADR-0202](../../../adr/ADR-0202-typed-semantics-attributes.md),
[ADR-0203](../../../adr/ADR-0203-semantics-vocabulary.md),
[ADR-0204](../../../adr/ADR-0204-synthetic-semantics-nodes.md). ADR-0124 получил
`Superseded-by: ADR-0203` в части номеров битов действий; решения 1–2 в
`crates/flui-semantics/ARCHITECTURE.md` помечены заменёнными ADR-0202 и действуют до W3.

- **ADR-0202** — модель атрибутов, единая трансляция, таблица слияния, удаление
  `SemanticsProperties`/флагов/`NodeData`/`Snapshot`/`SemanticsEvent`. Заменяет ARCHITECTURE.md
  решения 1–2 (каскад по флагам) порядком специфичности ролей.
- **ADR-0203** — словарь `flui-protocol`: полный `SemanticsRole`, выровненный `SemanticsAction`,
  новые числовые значения и wire-имена; связь с `wire::Role` (ADR-0080, ADR-0095).
- **ADR-0204** — синтетические id `1..2³²` и текстовая модель.
- **ADR-0205** — связи (уже в ветке).

## Работы

Каждая — отдельный PR с `cargo xtask check-changed`.

1. **W1.** ADR-0202, ADR-0203, ADR-0204 на утверждение.
2. **W2.** `SemanticsAttributes` и группы за существующими сеттерами; трансляция из
   `PublishView`; удаление `SemanticsNodeData`, `SemanticsSnapshot`, `SemanticsProperties`.
   Поведение не меняется — все тесты зелёные без правок ожиданий.
3. **W3.** Типизированные состояния вместо флагов; таблица слияния; порядок специфичности ролей.
4. **W4.** Словарь протокола (роли, действия), миграция агента, `tools/desktop-mcp`,
   `flui-devtools`, `flui-testing`, механическая миграция вызовов в виджетах.
5. **W5.** Матрица покрытия, `validate`, сверка версий, фикстуры UIA по семействам.
6. **W6.** Коллекции и виртуализация.
7. **W7.** Live, `busy`, объявитель; production-путь `SemanticsHost::announce`.
8. **W8.** Синтетические id и текстовая модель полей; единый источник границ слов.
9. **W9.** Оставшиеся свойства (`orientation`, `current`, `has_popup`, `autocomplete`,
   `keyboard_shortcut`, `language`, `url`, `role_description`) и роли из матрицы.

W2–W3 — фундамент. W6–W8 зависят от W4: коллекциям нужны роли деревьев и сеток, live-регионам — `Log`, `Timer`, `Marquee`, тексту — `SetTextSelection`, `ReplaceSelectedText` и типизированные данные действий; W5 и W9 тоже идут после W4. После W4 работы независимы друг от друга, кроме W8: синтетические id выдаёт аллокатор W7.

## Риски

1. **Изменение исхода слияния.** Перевод с пересечения битов на таблицу групп может разделить
   или слить узлы иначе; правило «вложенный merger остаётся узлом» меняет дерево намеренно.
   Смягчение: W2 не меняет поведения; W3 сравнивает опубликованные деревья всех тестов виджетов
   до и после, каждое расхождение объясняется (намеренное — тестом) или исправляется.
2. **Расхождение слов с редактором.** Публикуемые начала слов могут разойтись с навигацией
   редактора. Смягчение: W8 вызывает политику контроллера, а не копию, и тест «опубликованные начала слов = остановки Ctrl+→».
3. **Bump AccessKit.** Смягчение: тест сверки (R21) и матрица покрытия.
4. **Объём W4.** Затрагивает много крейтов одним PR. Смягчение: механическая замена через
   `ast-grep`, поведение закреплено существующими тестами.
5. **Геометрия символов.** Стоимость `get_boxes_for_selection` по графемам на длинных полях.
   Смягчение: кеш до следующего `layout`, замер в бенчмарке сборки.

## Сверка с другими системами (2026-10-12)

Дизайн проверен по первоисточникам после того, как был составлен (AGENTS.md: чек-лист, не цель).

| Решение | Flutter (master) | Compose / SwiftUI | Rust поверх AccessKit | Chromium / ARIA 1.2 |
|---|---|---|---|---|
| Одна роль на узел | роль + флаги-трейты; `isHeader` — роль на вебе и трейт на desktop | одна `Role` / трейты-множество | одна (модель AccessKit) | одна; список ролей ARIA — только запасной вариант авторства |
| Типизированные состояния | `CheckedState`, `Tristate` с `merge`/`hasConflict` на типе | `ToggleableState` | `Toggled` AccessKit | `CheckedState` kNone/kFalse/kTrue/kMixed |
| Конфликт → отдельные узлы | да (`isCompatibleWith`) | нет, побеждает первый | слияния нет или «последний» | — |
| Уровень — общий атрибут | `headingLevel` 0–6 | Compose: только булев heading; SwiftUI: h1–h6 | `level` | `kHierarchicalLevel` у heading/listitem/treeitem/row |
| Неприменимый атрибут | assert в debug | не проверяется | Slint — при компиляции | ARIA: MUST ignore; Chromium фильтрует, имя → описание |
| Типизированные действия | типизированные сеттеры над `Object?` | типизированные ключи | Slint — да, остальные — сырой `ActionRequest` | — |
| Синтетические id | `1 << 16` и выше | глобальный счётчик, фейковые id + 1e9 | Slint — старшие биты; egui — хеш; Masonry — `TODO` | отрицательный диапазон |
| Прогоны текста | нет (TextPattern по смещениям) | лениво по запросу | дети поля, ≤ 256 символов | inline text box лениво |
| Объявления | вне дерева: alert-узел UIA, уведомление macOS | Android 36: устарело, live-регионы; Apple: `Announcement` | live-регион `Label` с `value` | `ariaNotify` → UIA Notification |
| Своя модель против AccessKit в API | своя | своя | Masonry/egui/Bevy — AccessKit; Slint — своя (и Qt) | своя (AXNodeData) |

Изменения дизайна по итогам: состояние `toggled` вместо `checked`/`pressed`; правило слияния на
типе состояния; перенос запрещённого имени в описание; вложенный merger — отдельный узел;
прогоны ≤ 256 графем со стабильными id; поддеревья отвергнуты по факту; два узла-объявителя
со значением в `value`; неявный live по ARIA (timer, marquee — off; alert, status — atomic).

## Открытые вопросы

Закрыты 2026-10-12 по итогам сверки: Q1 — плоский словарь и общий `level` (b); Q2 —
синтетические id `1..2³²` в основном дереве (h); Q3 — два объявителя у владельца (j); Q4 —
отдельный словарь агента (b); Q5 — прогоны только у полей ввода в W8, у статичного текста
после замера стоимости.
