# Фокус и клавиатура в Notes — дизайн

- **Статус:** черновик (переработка (a), (b) после ревью); **Дата:** 2026-10-05
- **База:** `main` @ `4915054c8`
- **Требования:** [requirements.md](requirements.md); уровень 0 —
  [release/requirements.md](../release/requirements.md) R8, R16; смежные —
  [text-ime](../text-ime/requirements.md) (R11, Escape; preedit коммитится при unmount),
  [render-proof](../render-proof/requirements.md) (R1, R3), [teardown](../teardown/design.md)
  (пользовательский код — только в realm, стоящем в своём слоте), `send-flip` (работы 5 и 8 после него)
- **Открытые вопросы закрыты:** Back на Apple — Cmd+[; пустой список — остановка Tab с меткой
  «нет заметок», и pop при пустом списке возвращает фокус на него (правка R8, «Риски» 3).

## Текущее состояние

Дополняет список в требованиях только тем, на что опирается дизайн.

- **Запрос фокуса.** `FocusNode::request_focus` (`crates/flui-interaction/src/routing/focus_scope.rs:896-927`):
  `Unbound` → `Queued`. `remove_child` (`:1285-1309`) снимает primary через `manager.unfocus()` и
  переводит поддерево в `Unbound` (`:1311-1318`): запрос на узел уничтоженного элемента неотличим
  от запроса до присоединения. `detach()` в production — только `dispose` у `Focus`, `FocusScope`,
  `EditableText` (`crates/flui-widgets/src/interaction/focus.rs:914`, `:1234`,
  `crates/flui-widgets/src/text/editable_text.rs:1737`); внешний узел (`Focus::with_external_node`)
  переживает хоста и может быть присоединён снова.
- **Удаление сфокусированного** даёт `unfocus()` сразу, изнутри `dispose`, то есть изнутри
  `SparseChildren::reconcile` с удерживаемым `&mut ElementTree` (`sparse_children.rs`): после
  `Retry` primary = `None` (нарушает R15), и слушатели запускаются посреди сверки.
- **Порядок уведомлений.** Реентерабельные запросы — FIFO `pending_focus_transitions` с бюджетом
  (`crates/flui-interaction/src/routing/focus.rs:244-345`). `finish_node_replacement` защищён только
  `debug_assert_eq!` (`:395-404`); `replace_node` публичен (`focus_scope.rs:241-251`); `Focus`
  делает `attachment.take()` перед ним и `expect` после (`focus.rs:848-853` в flui-widgets).
- **Клавиши.** `dispatch_key_event` (`focus.rs:653-711`): глобальные обработчики, затем primary и
  предки; кому достался key-down, не учитывается. Вызов — `handle_input_addressed`
  (`crates/flui-runtime/src/ui_realm/input.rs:286-296`). `SingleActivator::matches` — только key-down
  (`shortcuts.rs:139-147`). `EditableText` возвращает `Ignored` на любой key-up
  (`editable_text.rs:2011`) и поглощает ArrowLeft с Alt (`:2074-2084`).
- **Win32** обрабатывает только `WM_LBUTTON*`/`WM_MBUTTON*`
  (`crates/flui-platform/src/platforms/windows/platform.rs:1539`, `:1573`); `WM_XBUTTON*` и
  `WM_APPCOMMAND` не обрабатываются: XButton1 до FLUI не доходит.
- **Assistive focus.** `Focus::build` уже вешает `on_focus` → `request_focus` (`focus.rs:942-951`).
- **Маршруты.** `set_first_focus` на каждый `TopChanged`, включая начальный (`modal_route.rs:296-301`),
  — фокус без Tab при старте (нарушает R1); перекрытый маршрут фокусируем; отложенный первый
  фокус исполняется на первом присоединённом потомке (`focus_scope.rs:1655-1729`), раньше
  `autofocus` поля `Title` (`focus.rs:715-733`).
- **Ленивый список.** Keep-alive (ADR-0056) не раскладывает удержанного ребёнка: он вне paint,
  hit-test и семантики (`crates/flui-view/src/owner/keep_alive.rs:31-37`). Обёртка строки
  обязана пробрасывать ключ ребёнка (`salting_child_key`, `crates/flui-widgets/src/scroll/sliver_list.rs:100-111`),
  иначе сверка по ключу теряет состояние. Измеренный экстент идёт в `Virtualizer::set_measured` и
  коррекцию якоря (`crates/flui-objects/src/sliver/virtualized_band.rs:132-160`, `:303-317`).
  AccessKit-пара позиция/размер набора публикуется (`accesskit_translation.rs:553-572`).
- **Material.** `WidgetState::Focused` читают: рампы кнопок (`elevated_button.rs:174-186`, им
  пользуются Text/Elevated/Filled/IconButton), `outlined_button.rs:141`,
  `floating_action_button.rs:209`, `tabs.rs:448`, гало `checkbox.rs:541-580`, `switch.rs:368-432`,
  `radio.rs:375-404` (NavigationBar — через Radio-подобный `InkWell`), и `input_decorator.rs:149-203`,
  который ставит `Focused` сам по фокусу поля (`:448`). Всего ~25 мест; источник — `InkWell`
  (`ink_well.rs:435-440`), кроме `InputDecorator`.

## Варианты

### (a) Roving focus в ленивом списке (R5, R6, D1)

Где живёт индекс: (1) в `FocusScopeNode` списка; (2) в состоянии виджета списка; (3) у
приложения. Как держится строка вне viewport: (i) keep-alive; (ii) pinned-слот в sliver;
(iii) фокус пересоздаётся при повторной постройке.

**Выбор: (2)+(ii).** `ListView::focusable(true)` строит вокруг viewport **узел списка** — обычный
фокусируемый `Focus` (не scope: ADR-0026) с точкой входа ((b)), семантикой `Role::List` и именем
из `semantics_label`; при `count == 0` имя — `empty_label`. Состояние `ListFocusState` в элементе:
запомненная строка `(собственный ключ элемента данных, индекс)`, реестр построенных строк
`BTreeMap<usize, Weak<FocusNode>>` (узлы маркеров), `pending: Option<usize>`.

Каждая построенная строка обёрнута в **маркер** `ListItemFocus`: `Focus` с
`can_request_focus(false)` и без своей семантики фокуса — **не фокусируем**, Tab и primary на нём
не останавливаются. Фокус получает первый фокусируемый потомок строки (`InkWell` кнопки), поэтому
`ActivateIntent` и `FocusRing` работают на том же узле, что при клике. Маркер пробрасывает ключ
ребёнка, как `RepaintBoundary` (`salting_child_key`), и сверка ленивого списка видит ключ данных;
`ListFocusState` хранит ключ без соли (`SaltedKey::unsalt`) — именно его принимает
`find_index_by_key`. Маркер оборачивает строку в `MergeSemantics` + `Semantics` с ролью `ListItem`,
`index_in_parent` и размером набора: кнопка и элемент списка — один узел a11y.

Клавиши — `on_key_event` узла списка (всплывают от строки): ArrowUp/Down, PageUp/Down (первое
нажатие — крайняя полностью видимая строка: прямоугольники маркеров против прямоугольника узла
списка), Home/End. Цель построена → её первый фокусируемый потомок,
`request_focus_with(FocusCause::Traversal)`, раскрытие через `get_offset_to_reveal`. Не построена
→ `pending = Some(i)`, `ScrollPosition::set_pixels` по оценке экстента (из обработчика клавиши, не
из build); маркер с этим индексом в `init_state` проверяет `i < count` и фокусирует потомка
(`Traversal`). На краю — `Ignored`: фокус остаётся, а вложенный список отдаёт стрелку внешнему.

**Pin.** Аренда вида `Pin` в таблице keep-alive берётся маркером, пока `marker.has_focus()`
(primary внутри строки — кнопка или поле с композицией) **или** строка запомнена списком; при
смене сначала берётся новая, потом отпускается старая. `retain_band` не вытесняет закреплённого.
`RenderSliverList`/`RenderSliverFixedExtentList` раскладывают его отдельным слотом: ребёнок
**меряется** (layout с constraints полосы), но его экстент **не** идёт в `Virtualizer::set_measured`,
коррекцию якоря, scroll/paint/cache extent — геометрия sliver не меняется. Офсет — по оценке
экстента (точен для fixed). Placed-stamp ставится, семантика видит ребёнка; paint и hit-test
пропускают pinned-слот **явно** (не полагаясь на клип: при `Clip::None` его нет). Граница:
полоса + cache + 1 — на список одна аренда сверх полосы, потому что запомненная строка и строка с
primary совпадают (primary переходит в строку только через вход, который обновляет запомненную).

- Покрывает R5, R6, R8, R13b, бюджет D1.
- Отвергнуто: (i) — строка вне `A11yTree`; (iii) — R6 запрещает терять фокус при прокрутке
  указателем; (1) — scope ломает route-restore и autofocus (ADR-0026); (3) — лишняя поверхность;
  фокусируемый маркер — два узла на строку: `ActivateIntent` идёт вверх от маркера и не находит
  `Actions` кнопки, кольцо смотрит на не-primary узел.
- ADR: следует ADR-0026, ADR-0053; частично заменяет ADR-0056 для `Pin`.

### (b) Идентичность фокуса через rebuild, удаление и pop (R8, R15)

(1) `GlobalKey` на строку; (2) ключ данных строки + индекс + первый; (3) новая идентичность
на `FocusNode`.

**Выбор: (2), через точку входа и отложенное разрешение.**

*Единица обхода.* `FocusNode::register_traversal_entry(entry)`: потомки узла не кандидаты Tab;
курсор внутри поднимается до внешней единицы; история scope записывает единицу вместо узла
внутри неё. Вход возвращает `EntryTarget`: `Node(n)` — фокус на `n`; `Pending` — цель известна, но
не построена: фокус сейчас на единице, список доведёт его до строки при монтировании; `Unit` —
фокус на единице (пустой список, Narrator читает `empty_label`). Вход списка: живая запомненная
строка → `find_index_by_key(ключ)` → индекс, приведённый к `0..count`; построена → `Node`, иначе
`Pending` + запись в `pending` (прокрутка — в шаге разрешения, вне build); `count == 0` → `Unit`.
Строка с тем же ключом и тем же корневым типом — тот же элемент и узел; если корневой тип
сменился, узел новый, и вход находит его по ключу.

*Удаление сфокусированного.* `remove_child` вызывается из `dispose` внутри сверки и **ничего не
публикует и не вызывает**: он записывает в менеджер маркер осиротевшего primary — ближайший
уцелевший scope, внешнюю единицу (если удаляемый был в ней) и позицию по порядку (ключ сортировки
из последней закоммиченной раскладки). Повторные удаления в том же кадре маркер не меняют.
`primary_focus()` до разрешения возвращает прежний узел (он отсоединён; ключи в это время не
приходят — идёт кадр). **Шаг разрешения** `FocusManager::settle_orphaned_focus()` вызывает рантайм
один раз за кадр после fixpoint build/layout и до семантики, вне заимствований дерева, в realm,
стоящем в слоте: единица жива → её вход; иначе следующий по порядку кандидат scope после позиции,
иначе первый; удалён сам scope → `set_first_focus` объемлющего. Публикуется одно ребро
`старый → преемник` (`Programmatic`), без `None`. Если ребро или вход пометили элементы грязными
либо сдвинули прокрутку, рантайм повторяет build/layout ровно один раз до paint, остаток — в
следующем кадре. Reload, заменивший все 10 000 ключей: сверка удаляет построенные строки (одна
запись маркера), шаг разрешения один раз зовёт вход — ключ не найден → индекс, приведённый к
новому `count` → строка из только что построенной полосы → одно ребро; ни N переходов, ни
устаревших индексов, ни записи прокрутки в build.

- Покрывает R8, R15 (нет ребра в `None`), D1, решение по пустому списку.
- Риск: Retry→список: преемник — `Settings`, а не список (список ещё не разложен на момент
  записи позиции). R15 это допускает; строка таблицы пинит.
- Отвергнуто: (1) — 10 000 `GlobalKey` в приложении; (3) — вторая система идентичности рядом
  с ключами, которые сверка уже использует; немедленный преемник в `remove_child` — запуск
  пользовательского кода посреди сверки и N переходов при массовой замене.
- ADR: дополняет ADR-0026 §2 (единица — фильтр кандидатов и подъём курсора).

### (c) Результат запроса на устаревший узел (R14, D4)

(1) `FocusRequestOutcome::Stale`; (2) `Result<_, FocusRequestError>`; (3) `Rejected` с логом.

**Выбор: (1).** `Stale` — только для узлов, которыми **владеет виджет** (`Focus`, `FocusScope`,
`EditableText` создали узел сами) и которые этот виджет отсоединил в `dispose`: узел помечается
`retired`, запрос отвечает `Stale`, ничего не ждёт и не публикует. Внешний узел приложения после
`dispose` хоста остаётся `Queued` (запрос, затем показ виджета с этим узлом — работает). Enum
`#[non_exhaustive]`. В Notes узел `Title` внешний, но его владелец `ScreenState` уничтожается при
pop — запрос из захваченного callback остаётся без последствий; тест R14 строится на узле,
которым владеет виджет.

- Покрывает R14, D4 (правка `focus_scope.rs:1672`, `:1850`). Отвергнуто: (2) — ломает все
  вызовы ради одного случая; (3) — запрещено D4.

### (d) Остаток #1040: вложенная замена узла (R13c, D5)

(1) Очередь публикации замены; (2) отказ `FocusTreeError::NotificationInFlight` до мутации.

**Выбор: (2).** `replace_node` при `notification_depth > 0` возвращает ошибку без мутации, в
любой сборке; `debug_assert` в `finish_node_replacement` → `expect("BUG: …")`, недостижимый по
фазе (замена идёт из `did_update_view` в build, уведомлений там нет). `Focus::did_update_view`
при `Err` возвращает взятую `take()` аттачмент на место и оставляет прежний узел.

- Покрывает R13c, закрывает #1040. Отвергнуто: (1) — структура уже изменена, primary снят «тихо».

### (e) Модальность ввода и индикатор (R4, R10, R17)

(1) Флаг на виджетах; (2) состояние `FocusManager` (на presentation, в realm); (3) сервис в
`flui-runtime`. **Выбор: (2).**

`FocusManager` хранит модальность `Keyboard | Pointer` (приватно) и причину перехода
`FocusCause { Programmatic, Traversal, Assistive }` (`#[non_exhaustive]`). Key-down, допущенный к
обходу, → `Keyboard`; `note_pointer_input()` (рантайм, pointer-down) → `Pointer`; переход с
`Traversal`/`Assistive` → `Keyboard`; `Programmatic` не меняет. UIA `SetFocus` → `on_focus` →
`request_focus_with(Assistive)`. `has_visible_focus()` = primary && `Keyboard`; смена модальности
уведомляет слушателей primary. После Alt+Tab в окно приходит только сиротский key-up Alt — он
отбрасывается и модальность не меняет; активация её тоже не трогает, поэтому R10 возвращает
индикатор в прежнее состояние, каким бы оно ни было.

**Индикатор.** `FocusRing { child, style: Option<FocusRingStyle> }` (литерал, `FocusRing::new`,
`.style`) в `flui-widgets`: двухцветная рамка внутри границ (2 px тёмная + 1 px светлая, как WinUI
focus visual), foreground поверх ребёнка — не перекрывается соседними строками без отступов,
одна из полос даёт ≥ 3:1 к любому фону. Стиль: явный → унаследованный `DefaultFocusRingStyle` →
встроенный. Material `Theme` ставит `DefaultFocusRingStyle` из `ThemeData::focus_ring`, поэтому
`RawButton` под темой получает её цвета без зависимости от Material.

**Material.** `InkWell` ставит `WidgetState::Focused` = **видимый** фокус (`has_visible_focus`),
так что клик указателем не включает ни overlay, ни гало (R17) ни у одного потребителя.
Индикатор ровно один: кнопки (Text/Elevated/Filled/Outlined/Icon/FAB), Tabs, NavigationBar —
`FocusRing`, а уровень `Focused` убирается из их overlay-рамп (D2: отличается от нажатия);
Checkbox/Switch/Radio сохраняют M3-гало как индикатор и строят `InkWell` без кольца
(`pub(crate)`). `InputDecorator` не меняется: поле показывает фокус всегда.

- Покрывает R4, R10, R17, D2. Отвергнуто: (1) — N копий; (3) — два владельца фокуса.
### (f) Клавиши: повторы, Space, IME, Back

(1) Фильтр в каждом виджете; (2) реестр нажатий в `FocusManager`. **Выбор: (2) плюс привязки.**

Реестр `pressed: HashMap<KeyId, KeyOrigin>`. `KeyId` — физический `Code`, для
`Code::Unidentified` — логическая клавиша, для синтетического Back от XButton1 — отдельный вариант,
не совпадающий с физическим `BrowserBack`. `KeyOrigin` — `Option<FocusNodeId>` primary на момент
key-down или `Ime`. Фильтр стоит **только перед обходом**; глобальные обработчики видят все
события, как сейчас.

1. key-down с `is_composing` или `NamedKey::Process` → `Ime`, до обхода не доходит;
2. key-down → запись origin, обход; если в этой доставке был переход `Traversal`, origin = новый primary;
3. repeat и key-up идут в обход, только если запись есть, не `Ime` и origin и текущий primary,
   **поднятые до внешней единицы обхода**, совпадают; key-up удаляет запись; сироты отбрасываются.
   Поднятие держит удержанный PageDown/End к непостроенной строке (фокус придёт позже из
   `init_state` строки, но единица — тот же список); Enter на строке → редактор — разные единицы,
   повторы отброшены;
4. `forget_pressed_keys()` при деактивации окна.

Запись — до вызова обработчиков, перепривязка — после: паника обработчика оставляет запись
согласованной (R16).

| Клавиша | Платформы | Интент | Повтор |
|---|---|---|---|
| Enter, Select (down) | все | `ActivateIntent` | нет |
| Space (down) / Space (up) | все | `ActivationPressIntent` / `ActivateIntent` | нет |
| Alt+Left, `BrowserBack`, XButton1 | Windows, Linux, Android, Web | `BackIntent` | нет |
| Cmd+[, `BrowserBack`, XButton1 | macOS, iOS | `BackIntent` | нет |
| Escape | все | `DismissIntent` | нет |

`BackIntent` — `Navigator::maybe_pop` (уважает `PopScope`; корневая страница → `NotPerformed`,
клавиша уходит хосту); Router синхронизируется своим `NavigatorObserver`, как для жеста назад.
`DismissIntent` — только модальный маршрут. `EditableText` отображает `DismissIntent`,
`ActivationPressIntent` и `ActivateIntent` в поглощающий no-op (иначе key-up набранного пробела
всплывает и активирует предка) и не поглощает Arrow+Alt на не-Apple платформах. XButton1: Win32
обрабатывает `WM_XBUTTONDOWN/UP` как pointer-события с `PointerButton::X1` и возвращает `TRUE`
(без `DefWindowProc`, значит без второго `APPCOMMAND_BROWSER_BACKWARD`); winit уже даёт `Back`.
Рантайм на pointer-down/up X1 диспатчит синтетические `BrowserBack` down/up со своим `KeyId`.
`InkWell` на `ActivationPressIntent` ставит `Pressed` до `ActivateIntent` или потери фокуса.

- Покрывает R3, R9, R11, R12, R16, text-ime. Отвергнуто: (1) — «key-down достался другому» знает
  только владелец primary; `WM_APPCOMMAND` — второй путь Back только для Win32.

### Маршруты (R1, R7, R11)

Scope маршрута допускает фокус потомков, только пока маршрут текущий (сначала активируется
новый верх, затем выключается старый — без ребра в `None`). Начальный `TopChanged` фокус не берёт.
Первый фокус **push** предварителен: `autofocus` в том же scope до первого непрограммного перехода
его перебивает (`Title`). Восстановление при **pop** окончательно: поздний `autofocus` его не
перебивает (R8). Doc `page_route.rs:32` правится.

## Публичный контракт

Новое — без пометки; **(изм.)** — меняется существующее. Пути — через facade, как соседние элементы.

`flui-interaction` → `flui::interaction`:

```rust
#[non_exhaustive] pub enum FocusRequestOutcome { Focused, Queued, Rejected, OwnerClosed, Stale } // (изм.)
pub enum FocusTreeError { /* … */ NotificationInFlight { node: FocusNodeId } }               // (изм.)
#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusCause { Programmatic, Traversal, Assistive }
#[non_exhaustive] pub enum EntryTarget { Node(Rc<FocusNode>), Pending, Unit }
pub type TraversalEntry = Rc<dyn Fn() -> EntryTarget>;
impl FocusNode {
    pub fn request_focus_with(self: &Rc<Self>, cause: FocusCause) -> FocusRequestOutcome;
    pub fn has_visible_focus(&self) -> bool;
    pub fn register_traversal_entry(self: &Rc<Self>, entry: TraversalEntry) -> FocusNodeRegistration;
}
impl FocusAttachment { pub fn replace_node(&self, r: &Rc<FocusNode>) -> Result<FocusAttachment, FocusTreeError>; } // (изм.)
impl FocusManager {
    pub fn note_pointer_input(&self);      // рантайм: pointer-down
    pub fn forget_pressed_keys(&self);     // рантайм: деактивация окна
    pub fn settle_orphaned_focus(&self);   // рантайм: раз за кадр, после build/layout
    pub fn dispatch_key_event(&self, event: &KeyEvent) -> bool; // (изм.: реестр перед обходом)
}
```

Пометка «узел виджета» для `Stale` — `pub(crate)`-конструктор, доступный `flui-widgets` через
`__runtime`. `flui-view` (не в facade): `impl KeepAliveHandle { pub fn pin(&self) -> KeepAliveLease; }`.

`flui-widgets` → `flui::widgets`:

```rust
impl SingleActivator { pub fn on_release(self) -> Self; }  // (изм.: matches принимает key-up)
pub struct ActivationPressIntent; pub struct BackIntent; pub struct DismissIntent;
pub struct FocusRing { pub child: BoxedView, pub style: Option<FocusRingStyle> }
impl FocusRing { pub fn new(child: impl IntoView) -> Self; pub fn style(self, s: FocusRingStyle) -> Self; }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusRingStyle { pub outer: Color, pub inner: Color, pub outer_width: f64, pub inner_width: f64 }
pub struct DefaultFocusRingStyle { /* inherited */ }
impl DefaultFocusRingStyle { pub fn new(style: FocusRingStyle, child: impl IntoView) -> Self;
                             pub fn of(ctx: &dyn BuildContext) -> FocusRingStyle; }
impl ListView {
    pub fn focusable(self, focusable: bool) -> Self;
    pub fn semantics_label(self, label: impl Into<String>) -> Self;
    pub fn empty_label(self, label: impl Into<String>) -> Self;
}
```

**(изм.):** `RawButton` (`Focus` + `Actions{Activate, ButtonActivate}` + `FocusRing`, фокусируем
только с `on_press`), `DefaultFocusTraversal` (таблица), `Navigator` (`BackIntent`), `ModalRoute`,
`EditableText`, `Focus` (`Assistive`, отказ замены). `ListItemFocus`, `ListFocusState` — `pub(crate)`.

`flui-material` → `flui::material`: `ThemeData { pub focus_ring: FocusRingStyle }` (изм.);
`Theme` ставит `DefaultFocusRingStyle`; `InkWell` (изм.: видимый `Focused`, `FocusRing`,
`ActivationPressIntent`). `flui-sdk`: реэкспорт новых типов и строки в `tests/surface.rs`
(ADR-0088 §4).

Production-вызовы: `request_focus_with` — `Focus` (Assistive), список и `NextFocusAction`
(Traversal); `note_pointer_input`, `forget_pressed_keys`, `settle_orphaned_focus` — `flui-runtime`;
`pin` — `ListItemFocus`; `register_traversal_entry` — `ListView`; `on_release` —
`DefaultFocusTraversal`; `DefaultFocusRingStyle` — Material `Theme`.

`examples/two_screens/tree.rs`: `.focusable(true).semantics_label("Notes").empty_label("No notes")`,
строки с `ValueKey::new(id)` и `find_index_by_key`, `TextFormField::autofocus(true)`.

## Инварианты, владение, lifecycle

- **Владение.** Реестр нажатий, модальность, причина и маркер осиротевшего primary — поля
  `FocusManager` (один на presentation, ADR-0078); `ListFocusState` — элемент списка; pin — таблица
  keep-alive. Новых `static`/`thread_local!` нет.
- **Пользовательский код** (слушатели, вход, действия) запускается только в realm в своём слоте и
  без заимствований дерева: никогда из `dispose`/сверки (там — только запись маркера).
- **Порядок для одной клавиши:** глобальные обработчики → фильтр реестра → запись → модальность →
  обход → действия → переходы (commit, узлы, слушатели; реентерабельные — FIFO) → перепривязка.
- **Порядок кадра:** build/layout fixpoint → `settle_orphaned_focus` (одно ребро) → не более одного
  повтора build/layout → paint → семантика.
- **a11y == primary.** `focused` публикует аннотация `Focus`; маркер строки фокус не публикует;
  после кадра primary не стоит на узле scope (кроме маршрута без фокусируемых) и не проходит через
  `None`, пока есть кандидаты.
- **Pin.** Не более одной аренды на список сверх полосы; строку, которую данные перестали
  выдавать, сверка уничтожает несмотря на аренду (ADR-0056).
- **Паника** обработчика, слушателя или входа — под `FocusClosePanic` (вход → `Unit`);
  `NotificationDepthGuard` не застревает; маркер снимается шагом разрешения и при панике в нём.

## Ошибки и отказы

| Ситуация | Результат | Состояние после |
|---|---|---|
| запрос на узел виджета после его `dispose` | `Stale` | фокус прежний, ничего не ждёт |
| запрос на отсоединённый внешний узел | `Queued` | исполнится при присоединении |
| `replace_node` из слушателя фокуса | `Err(NotificationInFlight)` | дерево и primary не тронуты, аттачмент возвращён |
| repeat/key-up другой единицы, сирота, IME-клавиша | в обход не идёт | глобальные обработчики видели |
| удалён сфокусированный | маркер; одно ребро в шаге разрешения | без `None` |
| вход: цель не построена | `Pending`, фокус на списке | строка берёт фокус при монтировании |
| цель ≥ `count` при монтировании | `pending` сброшен | фокус на списке |
| `pin()` без ленивого хоста | инертная аренда | — |
| паника `on_press`/слушателя/входа | локализована (PANIC-POLICY) | следующий Tab работает |
| Back на корне | `NotPerformed`, `false` хосту | — |
| Escape, Space-up в поле | поглощены без эффекта | — |
| платформа без клавиатуры | `Pointer`; a11y-фокус видим | без паники |

## Тестовая стратегия

Уровни: **таблица** — `crates/flui-widgets/tests/contracts.rs` (`run_cases`), семейство
`keyboard_focus_contracts`; **Notes** — `notes_public_input_flow_matrix`; **ядро** —
`crates/flui-interaction/tests/`; **harness** — `render_object_harness`; **GPU**, **нативный**.
После каждого шага Notes — помощник `assert_a11y_focus_matches_primary` (R13b).

| R | Тест | Уровень | Падает без изменения, потому что |
|---|---|---|---|
| R1 | `first_tab_enters_the_current_route` | Notes + a11y | начальный `TopChanged` фокусирует `Settings` |
| R2 | `tab_order_covers_every_screen` | Notes | перекрытый маршрут фокусируем; 10 000 остановок |
| R3 | `keyboard_activation_matches_click` (+ строка «Space-up в поле не активирует предка») | таблица | Space на down; key-up пробела всплывает |
| R3 | `raw_button_activates_from_the_keyboard` | `tests/raw_button.rs` | нет `Focus`/`Actions` |
| R4 | `focus_indicator_is_painted` | GPU | overlay = нажатие; контраст < 3:1 |
| R5 | `list_keyboard_navigation` (строки таблицы R5, удержание PageDown) | Notes | стрелки не привязаны; повторы к непостроенной строке отброшены |
| R6 | `focused_row_is_pinned_while_scrolled_away` | Notes | строка вытеснена или вне семантики |
| R6 | `harness_pinned_child_is_laid_out_outside_the_band`, `harness_pinned_child_leaves_sliver_geometry_unchanged`, `harness_pinned_child_is_skipped_by_paint_and_hit_test` (оба sliver, `Clip::None`) | harness | нет слота; экстент попадает в якорь |
| R7 | `push_moves_focus_into_the_new_route` | Notes | первый фокус перебивает autofocus |
| R8 | `pop_restores_the_opening_element` (ключ при смене корневого типа строки, индекс, пустой, поздний autofocus) | таблица + Notes | история теряет строку; новый узел не найден |
| R9 | `back_keys_pop_only_the_current_page` (+ модальный, XButton1, Alt+Left из поля) | Notes + таблица | Back не привязан; поле съедает Alt+Left |
| R10 | `window_reactivation_restores_focus` | Notes | защитный: деактивация не должна менять фокус и модальность |
| R11 | `keys_during_transition_reach_only_the_top_route` | Notes | уходящая страница фокусируема |
| R12 | `held_enter_activates_once`: одна страница, у `Title` нет `on_submitted`/перевода строки от повторов | Notes + ядро `key_repeats_follow_their_key_down` | повторы доходят до редактора |
| R13a | `reentrant_request_during_notification_is_applied_after_and_published_in_order` (в `tests/`) | ядро | регрессия #1152 |
| R13c | `nested_replacement_publishes_no_stale_edge` | ядро | замена из слушателя публикует устаревшее ребро |
| R14 | `stale_focus_request_is_inert` (узел виджета) | таблица | `Queued` |
| R15 | `focus_survives_rebuild_and_moves_on_removal` (+ Reload всех ключей: одно ребро, без `None`) | таблица + Notes | `unfocus` в `dispose`; N рёбер |
| R16 | `keyboard_continues_after_callback_panic` | таблица | защитный |
| R17 | `focus_indicator_follows_input_modality`: кольцо есть/нет в render-дереве (клик, Tab, Assistive, Alt) | таблица | `Focused` = overlay при клике |
| R18 | тур `cargo xtask device windows-notes` (+ XButton1) | нативный | шагов нет |

R3, R5, R7, R8, R12, R14, R15, R17 прогоняются с отменённым production-хунком в изолированном
worktree; R4 — точки на внешней и внутренней полосе и в центре кнопки в трёх состояниях.

## ADR

ADR (номер назначит оркестратор) для каждого:

- **«Focus requests on retired nodes and nested node replacement».** Supersedes ADR-0026 §1 в части
  запроса. `Stale` — для узла, которым владеет виджет, после его `dispose`; внешний узел —
  `Queued`; enum `#[non_exhaustive]`. `replace_node` во время уведомления — `NotificationInFlight`
  до мутации. Закрывает #1040.
- **«Keyboard focus across routes, lazy lists and removal».** Supersedes пункт `ModalRoute` ADR-0026
  §1 и частично «Not implemented: FocusTraversalGroup». Только текущий маршрут фокусируем; начальный
  маршрут фокус не берёт; первый фокус push предварителен, restore pop окончателен. Единица обхода с
  входом (`Node`/`Pending`/`Unit`). Удаление сфокусированного — маркер, одно ребро в шаге
  разрешения после build/layout, без `None` и без пользовательского кода в сверке.
- **«Pinned lazy children».** Supersedes ADR-0056 («удержанный не раскладывается») для `Pin`:
  ребёнок меряется вне полосы, в геометрию и якорь не входит, виден семантике, явно пропущен paint
  и hit-test. Граница — полоса + cache + число pin-аренд.
- **«Key press ledger, input modality and navigation keys».** Supersedes ADR-0079 §1 и «Not
  implemented»; дополняет ADR-0023 §2 (`on_release`). Реестр перед обходом с подъёмом до единицы;
  модальность и `FocusCause`; `Focused` у `InkWell` = видимый фокус; таблица Back/Escape;
  XButton1 — pointer-событие, синтетический `BrowserBack` с собственной идентичностью.

## Работы

‖ — параллельно (непересекающиеся файлы). Оценки — инженеро-дни.

1. ‖ Четыре ADR, `Superseded-by` в ADR-0023/0026/0056/0079. 1 д.
2. ‖ `flui-interaction`: `Stale` (узлы виджетов), `NotificationInFlight`, `FocusCause`, модальность,
   реестр с подъёмом, единица и `EntryTarget`, маркер осиротевшего primary и
   `settle_orphaned_focus`; тесты ядра; `ARCHITECTURE.md`. 5 д.
3. ‖ `flui-view` + `flui-objects`: `Pin`, `retain_band`, pinned-слот в двух sliver (мерка без
   `set_measured`, явный пропуск paint/hit-test), шесть harness-строк, `## Mapping decisions`. 4 д.
4. ‖ `flui-platform` Win32: `WM_XBUTTONDOWN/UP` → pointer X1, `TRUE`; Win32-unit. 1 д.
5. `flui-runtime` (после 2, 4): `note_pointer_input`, X1 → синтетический `BrowserBack`,
   `forget_pressed_keys`, шаг разрешения + один повтор build/layout в `ui_realm/frame.rs`. 2 д.
6. `flui-widgets/interaction` (после 2 и **после слияния send-flip**, он переписывает
   `raw_button.rs`): `on_release`, интенты, таблица, `FocusRing`, `DefaultFocusRingStyle`,
   `RawButton`, `Focus` (Assistive, возврат аттачмента), `EditableText` (no-op интенты, Alt+Arrow). 3,5 д.
7. `flui-widgets/navigator` (после 2, 6): фокусируемость по `is_current`, начальный маршрут,
   предварительный push / окончательный pop, `BackIntent`/`DismissIntent`, doc. 2 д.
8. ‖ с 7 — `flui-widgets/scroll` (после 2, 3, 6): `ListView::focusable`, маркер с пробросом ключа,
   `ListFocusState`, вход, PageUp/Down, раскрытие, имена. 4,5 д.
9. ‖ с 7, 8 — `flui-material` + `flui-sdk` (после 6 и **после слияния send-flip**, он переписывает
   33 сеттера `on_*`): `ThemeData::focus_ring`, `Theme`, `InkWell` (видимый `Focused`, кольцо),
   рампы и ~25 потребителей `Focused`, `surface.rs`. 2 д.
10. Notes (после 7–9): `tree.rs`, строки `notes_flow.rs`, таблица `keyboard_focus_contracts`. 2,5 д.
11. GPU readback `focus_indicator_is_painted` (после 9 и render-proof R1). 1 д.
12. Нативный тур (после 5, 10): первый Tab, обход, End/PageDown, удержание Enter, Alt+Left, XButton1,
    Alt+Tab, Retry; датированный прогон; закрыть #1040, #1092. 2,5 д.
13. `changelog.d/`, `ARCHITECTURE.md` flui-widgets. 0,5 д.

Итого ≈ 33 д. Критический путь 2 → 6 → 8 → 10 → 12 ≈ 18 д; работы 6 и 9 не начинаются до слияния
send-flip (отсечка 2026-11-24), так что начало пути после 2 привязано к этой дате.

## Риски

1. **Pinned-слот в sliver** трогает протокол, где уже были ошибки (ADR-0053/0054). Снижение:
   слот не входит ни в геометрию, ни в якорь; harness-строки на оба sliver и `Clip::None`.
2. **Повтор build/layout после шага разрешения** — новый шаг кадра в `flui-runtime`; если вход
   снова пачкает дерево, остаток уходит в следующий кадр, и Narrator на кадр видит фокус на
   списке. Проверка — Notes-строка Reload и нативный шаг Retry.
3. **Расхождение с текстом R8 для пустого списка** и зависимость от send-flip: правка одной строки
   R8 и подтверждение порядка работ — до утверждения дизайна.
