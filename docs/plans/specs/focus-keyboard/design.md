# Фокус и клавиатура в приложении приёмки — дизайн

- **Статус:** черновик, переписан 2026-10-29 под `main`; ждёт утверждения владельца
- **Дата:** 2026-10-05, переписан 2026-10-29
- **База:** `main` @ `015315508`
- **Требования:** [requirements.md](requirements.md); уровень 0 —
  [release/requirements.md](../release/requirements.md) R8; смежные — [text-ime](../text-ime/tasks.md)
  (`editable_text.rs`), [send-flip](../send-flip/tasks.md) (T9: `TextEditingController`),
  [flui-driver](../flui-driver/tasks.md) (FD11: лейн Windows для нативного сценария)

## Что уже построено

Решения прежней редакции о запросах фокуса и уведомлениях (`Stale`, `NotificationInFlight`),
маршрутах (фокусируем только текущий, предварительный push, окончательный pop), удалении
сфокусированного (`settle_orphaned_focus` после fixpoint), журнале нажатий, модальности,
`FocusCause`, Back/Escape и XButton1 реализованы. Они записаны в
`crates/flui-interaction/docs/ARCHITECTURE.md`, `crates/flui-widgets/ARCHITECTURE.md` и в
поправке ADR-0079 «activation keys, navigation keys and the focus indicator»; отдельные ADR
для них не нужны. Ниже — только то, чего на `main` нет.

## (a) Фокусируемый ленивый список и закреплённая строка (R5, R6, R8, R13b, D1, D6)

Где живёт индекс: (1) в `FocusScopeNode` списка; (2) в состоянии виджета списка; (3) у
приложения. Как держится строка вне viewport: (i) keep-alive; (ii) закреплённый слот в sliver;
(iii) фокус пересоздаётся при повторной постройке.

**Выбор: (2)+(ii).** `ListView::focusable(true)` строит вокруг viewport **узел списка** —
обычный фокусируемый `Focus` (не scope: ADR-0026), зарегистрированный единицей обхода
(`FocusNode::register_traversal_entry`, первый production-вызов), с семантикой `Role::List` и
именем из `semantics_label`; при пустом списке имя — `empty_label` (D6). Состояние
`ListFocusState` в элементе: запомненная строка `(собственный ключ элемента данных, индекс)`,
реестр построенных строк `BTreeMap<usize, Weak<FocusNode>>` (узлы маркеров),
`pending: Option<usize>`.

Каждая построенная строка обёрнута в **маркер** `ListItemFocus`: `Focus` с
`can_request_focus(false)` и без своей семантики фокуса — Tab и primary на нём не
останавливаются. Фокус получает первый фокусируемый потомок строки (кнопка), поэтому
`ActivateIntent` и `FocusRing` работают на том же узле, что при клике. Маркер пробрасывает ключ
ребёнка, как `RepaintBoundary` (`salting_child_key`, `flui-widgets/src/paint/repaint_boundary.rs`),
и сверка ленивого списка видит ключ данных; `ListFocusState` хранит ключ без соли
(`SaltedKey::unsalt`), его принимает `ListView::find_index_by_key`. Маркер оборачивает
строку в `MergeSemantics` + `Semantics` с ролью `ListItem`, позицией и размером набора
(ADR-0227): кнопка и элемент списка — один узел доступности.

Клавиши — `on_key_event` узла списка (всплывают от строки): ArrowUp/Down, PageUp/Down (первое
нажатие — крайняя полностью видимая строка: прямоугольники маркеров против прямоугольника узла
списка), Home/End. Цель построена → её первый фокусируемый потомок,
`request_focus_with(FocusCause::Traversal)`, раскрытие через `get_offset_to_reveal`. Не построена
→ `pending = Some(i)` и `ScrollPosition::set_pixels` по оценке экстента (из обработчика клавиши,
не из build); маркер с этим индексом в `init_state` проверяет `i < count` и фокусирует потомка
(`Traversal`). На краю — `Ignored`: фокус остаётся, вложенный список отдаёт стрелку внешнему.
Вход Tab отвечает `EntryTarget::Node` запомненной строки, если она построена, иначе первой
видимой; `Pending`, если её ещё строят.

**Закрепление.** Аренда вида `Pin` в таблице keep-alive (`flui-view/src/owner/keep_alive.rs`)
берётся маркером, пока `marker.has_focus()` (primary внутри строки, в том числе поле с
композицией) **или** строка запомнена списком; при смене сначала берётся новая, потом
отпускается старая. `retain_band` (`flui-view/src/element/sparse_children.rs`) не вытесняет
закреплённого. `RenderSliverList`/`RenderSliverFixedExtentList` раскладывают его отдельным слотом:
ребёнок меряется с constraints полосы, но его экстент **не** идёт в `Virtualizer::set_measured`,
коррекцию якоря, scroll/paint/cache extent — геометрия sliver не меняется (без
`..SliverGeometry::ZERO`). Офсет — по оценке экстента (точен для fixed). Семантика видит
ребёнка; paint и hit-test пропускают закреплённый слот **явно** (при `Clip::None` клипа нет).
Граница: полоса + cache + 1 — запомненная строка и строка с primary совпадают, потому что primary
попадает в строку только через вход, который обновляет запомненную.

- Отвергнуто: (i) — строка вне дерева доступности; (iii) — R6 запрещает терять фокус при
  прокрутке указателем; (1) — scope ломает route-restore и autofocus (ADR-0026); (3) — лишняя
  поверхность у каждого приложения; фокусируемый маркер — два узла на строку: `ActivateIntent`
  идёт вверх от маркера и не находит `Actions` кнопки, кольцо смотрит на не-primary узел.
- ADR: следует ADR-0026, ADR-0053; частично заменяет ADR-0056 («удержанный не раскладывается»)
  для `Pin`.

## (b) Индикатор в Material (R4, R17, D2)

`FocusRing` и `DefaultFocusRingStyle` уже в `flui-widgets` (стиль: явный → унаследованный →
встроенный), `RawButton` их использует. Material их не ставит: `InkWell`
(`packages/flui-material/src/ink_well.rs`) выставляет `WidgetState::Focused` по наличию фокуса,
поэтому клик включает overlay фокуса, а overlay совпадает с нажатием.

- `ThemeData::focus_ring` (из `ColorScheme`), `Theme` ставит `DefaultFocusRingStyle` из него, и
  `RawButton` под темой получает её цвета без зависимости от Material.
- `InkWell` ставит `Focused` = **видимый** фокус (`FocusNode::has_visible_focus`): после клика
  нет ни overlay, ни гало ни у одного потребителя.
- Индикатор ровно один: кнопки (Text/Elevated/Filled/Outlined/Icon/FAB), Tabs, NavigationBar —
  `FocusRing`, уровень `Focused` убирается из их overlay-рамп; Checkbox/Switch/Radio оставляют
  M3-гало и строят `InkWell` без кольца (`pub(crate)`). `InputDecorator` не меняется: поле
  показывает фокус всегда.
- Отвергнуто: флаг на каждом виджете (N копий одного правила); overlay как индикатор (D2:
  отличается от нажатия, у строк без отступов перекрывается соседями).

## (c) `autofocus` у текстовых полей (R7)

`Focus::autofocus` берёт фокус при входе маршрута и снимает предварительный первый фокус
(`take_provisional_first_focus`), но `EditableText`, `TextField` и `TextFormField` строят свой
`Focus` без этого флага, и приложение приёмки не может поставить фокус в `Title` при push.
Выбор: `autofocus(bool)` у `EditableText`, пробрасывается `TextField` и `TextFormField`; поле
передаёт его своему `Focus`, правило «restore pop окончателен» остаётся
(`late_autofocus_does_not_override_the_restored_focus`). Отвергнуто: обёртка `Focus` снаружи
поля — второй узел фокуса над узлом поля.

## (d) Alt+Left из поля (R9)

Вне Apple `EditableText` обрабатывает Alt+Left/Right как посимвольный сдвиг: аккорд оставлен за
будущим переходом к границе строки (`is_word_jump_modifier`, `# DEFERRED`), а пока проваливается
в обычное движение каретки и поглощается. Поэтому Back из поля не работает. Выбор: пока
действия границы строки нет, поле возвращает Alt+Left/Right без Shift как `Ignored`, клавиша
доходит до `BackIntent` маршрута; Alt+Right ни к чему не привязан и всплывает дальше. Когда
действие границы строки появится, его PR решает, на каких ОС оно занимает аккорд. На Apple Back —
Cmd+[, конфликта нет. Строка «Alt+Left из поля» добавляется в
`back_keys_pop_only_the_current_page`.

Клавиша, поглощённая IME (`NamedKey::Process` или композиция), до навигации уже не доходит:
журнал нажатий записывает её за методом ввода (`input_method_keys_never_reach_the_walk`).

## ADR

Один новый ADR (номер — следующий свободный на `main` в момент PR): **«Pinned lazy children and
the focusable list»** — закреплённый ребёнок меряется вне полосы, в геометрию и якорь не входит,
виден семантике, явно пропущен paint и hit-test; граница — полоса + cache + число аренд `Pin`;
список — одна остановка Tab через единицу обхода. `Superseded-by` в ADR-0056 (в части `Pin`).
Правило видимого `Focused` в Material дописывается в поправку ADR-0079 тем же PR, что его
вводит.

## Работы

‖ — параллельно (непересекающиеся файлы). Оценки — инженеро-дни.

1. ‖ `flui-view` + `flui-objects`: `Pin`, `retain_band`, закреплённый слот в двух sliver,
   harness-строки, ADR. 4 д.
2. `flui-widgets/scroll` (после 1): `ListView::focusable`, маркер, `ListFocusState`, вход,
   клавиши, раскрытие, имена. 4,5 д.
3. ‖ `flui-material` + `flui-sdk`: `ThemeData::focus_ring`, `Theme`, `InkWell`, рампы и
   потребители `Focused`, `surface.rs`. 2 д.
4. ‖ `flui-widgets/text` (после text-ime T5 и send-flip T9, которые меняют `editable_text.rs`):
   `autofocus`, Alt+Left, строки поля в таблице Back. 1 д.
5. Приложение приёмки (после 2–4): `ListView::focusable` и `autofocus` в `app.rs`, модуль
   `keyboard` рядом с `smoke`. 2,5 д.
6. GPU readback `focus_indicator_is_painted` (после 3). 1 д.
7. Нативный сценарий (после 5 и FD11). 2 д.
8. Закрытие: `crates/flui-widgets/ARCHITECTURE.md`, удаление спеки. 0,5 д.

## Риски

1. **Закреплённый слот в sliver** трогает протокол, где уже были ошибки (ADR-0053/0054).
   Снижение: слот не входит ни в геометрию, ни в якорь; harness-строки на оба sliver и
   `Clip::None`.
2. **Порядок с `editable_text.rs`:** text-ime T5 (#1958) и send-flip T9 правят тот же файл;
   работа 4 идёт после них.
3. **Нативный сценарий** ждёт лейн Windows flui-driver (FD11) и машину владельца.
