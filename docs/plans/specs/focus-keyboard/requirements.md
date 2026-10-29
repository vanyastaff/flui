# Фокус и клавиатура в приложении приёмки — требования (уровень 1)

- **Статус:** утверждена 2026-10-29 (координатор, по делегированию владельца); переписана под `main`
- **Дата:** 2026-10-05, переписан 2026-10-29
- **База:** `main` @ `015315508`
- **Уровень 0:** [../release/requirements.md](../release/requirements.md) R8 (и R16: #1040, #1092)
- **ADR:** [ADR-0026](../../../adr/ADR-0026-focus-traversal-seam.md),
  [ADR-0023](../../../adr/ADR-0023-actions-shortcuts-seam.md),
  [ADR-0056](../../../adr/ADR-0056-keep-alive-is-an-element-side-lease.md),
  [ADR-0079](../../../adr/ADR-0079-keyboard-activation-and-focus-for-assistive-technology.md)
  (поправка «activation keys, navigation keys and the focus indicator» записывает решения о
  клавишах, модальности и Back/Escape)

Прежняя редакция строилась вокруг примера Notes (`examples/two_screens`), которого больше нет.
Его место заняло приложение приёмки `tests/fixtures/acceptance_app/app.rs`: те же три экрана на
одном `Router`, только через фасад `flui`, headless-строки рядом с `smoke.rs`, окно —
`cargo run --example acceptance_app --features material`. Большая часть ядра уже на `main`
(«Состояние на `main`» ниже); требования с пометкой **сделано** остаются, потому что их тесты —
доказательство, на которое ссылаются остальные.

## Пользовательский сценарий

Пользователь без мыши запускает приложение приёмки и:

1. Tab ведёт в заголовок Home (`Settings`), затем в `Compact rows`, `New note` и список заметок.
   Стрелки, PageDown и End доходят до любой строки; экранный диктор читает «Note 5, 6 из 40».
2. Enter открывает заметку, и фокус сразу в поле `Title`. Tab и Enter на `Save note` сохраняют.
   Alt+Left (Cmd+[ на Apple) возвращает на Home, к строке той же заметки.
3. Так же проходятся `Settings` и `Retry` (после ошибки загрузки), а после Alt+Tab работа
   продолжается с того же элемента.

Элементы: заголовок — `Settings` (кроме Settings), `Back` (кроме Home); Home — `Compact rows`,
`New note`, список; Note — `Title`, `Save note`; Settings — только текст; экран ошибки — `Retry`.

## Решения

- **D1.** Список — одна остановка Tab, внутри roving focus на стрелках (ARIA listbox).
- **D2.** Индикатор фокуса: контраст ≥ 3:1 к соседним цветам (WCAG 2.2 SC 2.4.7 + 1.4.11), не
  перекрыт (SC 2.4.11), отличается от overlay нажатия. High contrast Windows — вне scope.
- **D3.** Назад — Alt+Left (Cmd+[ на macOS и iOS), `BrowserBack`, XButton1. Escape закрывает
  только модальные маршруты и не действует, пока фокус в текстовом поле.
- **D4.** Запрос на узел, который отпустил его виджет, получает `FocusRequestOutcome::Stale`.
- **D5.** Остаток #1040 (вложенная замена узла) починен; #1092 исправлен.
- **D6.** Пустой список остаётся остановкой Tab и объявляет `empty_label`; pop при пустом списке
  возвращает фокус на него.

## Состояние на `main`

- Ядро `flui-interaction`: `Stale`, `NotificationInFlight`, `FocusCause`, модальность ввода и
  `has_visible_focus`, журнал нажатий (повтор и отпускание принадлежат единице, получившей
  key-down), единица обхода с входом (`register_traversal_entry`, `EntryTarget`) и
  `settle_orphaned_focus`. Записано в `crates/flui-interaction/docs/ARCHITECTURE.md`.
- Рантайм: `note_pointer_input` на pointer-down, X1 → синтетический `BrowserBack` со своим
  `KeyId`, `forget_pressed_keys` при деактивации окна, `settle_orphaned_focus` после fixpoint
  build/layout.
- Win32: `WM_XBUTTONDOWN/UP` — pointer-событие X1, ответ `TRUE`, поэтому `WM_APPCOMMAND` не
  рождается (`crates/flui-native/src/input/windows/mod.rs`).
- `flui-widgets`: `SingleActivator::on_release`, `ActivationPressIntent`/`BackIntent`/
  `DismissIntent`, таблица `DefaultFocusTraversal`, `FocusRing` и `DefaultFocusRingStyle`,
  фокусируемый `RawButton`, `Focus` с `FocusCause::Assistive`; маршруты: фокусируем только
  текущий, начальный маршрут фокус не берёт, первый фокус push предварителен, restore pop
  окончателен, Back/Dismiss (`navigator/route_keys.rs`). `EditableText` поглощает
  Activation/Dismiss.
- Нет: фокусируемого `ListView` (у `register_traversal_entry` нет production-вызова), закрепления
  строки вне полосы (`KeepAliveLease` держит ребёнка, но не раскладывает его), кольца и видимого
  `Focused` в Material (`InkWell` и рампы кнопок показывают фокус overlay'ем и после клика),
  `autofocus` у `TextField`/`TextFormField`, Back по Alt+Left из поля (поле двигает каретку),
  GPU-проверки индикатора, строк приложения приёмки и нативного тура (`cargo xtask device`
  удалён в #1827; тур идёт через flui-driver).

## Требования

Уровни (`docs/testing.md`): **виджетный** — таблицы `crates/flui-widgets/tests/keyboard_focus/`;
**приёмка** — модуль `keyboard` рядом с `smoke` в `tests/fixtures/acceptance_app/` (его
исполняет `tests/facade_consumer.rs` через фасад); **GPU readback**; **нативный** — сценарий
flui-driver на лейне Windows.

### Обход и активация

- **R1.** КОГДА окно открывается, СИСТЕМА ДОЛЖНА оставлять primary focus пустым (ADR-0079 §3);
  первый Tab (Shift+Tab) фокусирует первый (последний) элемент текущего маршрута, элемент
  перекрытого маршрута не фокусируется. Тест: `first_tab_enters_the_current_route` —
  **сделано**; строка приёмки.
- **R2.** КОГДА пользователь нажимает Tab на любом экране (Home загружен, ошибка загрузки, Note,
  Settings), СИСТЕМА ДОЛЖНА проходить каждый интерактивный элемент ровно один раз в порядке
  чтения (список — одна остановка, D1). Shift+Tab идёт в обратном порядке, за последним — первый
  того же маршрута. Тест: приёмка, таблица по экранам, последовательность читается по узлу
  `focused` дерева доступности: `tab_order_covers_every_screen`.
- **R3.** КОГДА элемент в фокусе, СИСТЕМА ДОЛЖНА выполнять на Enter (key-down) и Space (key-up,
  с «нажато», пока Space удерживается) то же действие, что и клик; отключённый элемент ничего не
  делает. `RawButton` фокусируем при `on_press`. Тесты: `keyboard_activation_matches_click`,
  `raw_button_activates_from_the_keyboard` — **сделано**.
- **R4.** КОГДА элемент в фокусе, СИСТЕМА ДОЛЖНА рисовать индикатор по D2 у кнопок
  `flui-material`, у строк без отступов и у кнопки заголовка. Тест: GPU readback, точки выборки на
  индикаторе различают «в фокусе», «без фокуса» и «нажато», контраст по WCAG:
  `focus_indicator_is_painted`.

### Ленивый список (D1)

- **R5.** КОГДА фокус в фокусируемом `ListView`, СИСТЕМА ДОЛЖНА вести себя по таблице; после
  каждого шага целевая строка построена, в фокусе и целиком видна, а не построенная строится не
  позже следующего кадра. Тест: виджетный, `list_keyboard_navigation` на 10 000 строк; строка
  приёмки на 40 заметках.

  | Ввод | Результат |
  |------|-----------|
  | Вход Tab / Shift+Tab | запомненная строка, иначе первая видимая |
  | ArrowDown / ArrowUp | соседняя строка; на краю ничего, фокус остаётся; вложенный список отдаёт стрелку внешнему |
  | PageDown / PageUp | первое нажатие — последняя (первая) полностью видимая строка; следующие — на страницу дальше |
  | Home / End | первая / последняя строка |
  | Tab из списка | следующий элемент после списка (на Home — по кругу в заголовок) |
  | Предел построения | построенных строк ≤ видимая полоса + cache extent + 1 (закреплённая, R6) |
  | a11y | у строки роль элемента списка с позицией и размером набора («Note 5, 6 из 40») |

- **R6.** КОГДА сфокусированная строка уходит из viewport при прокрутке указателем, СИСТЕМА
  ДОЛЖНА держать её закреплённой: построенной, в фокусе и в дереве доступности; следующая стрелка
  или Enter действуют от неё и прокручивают её в видимую область. Тест: виджетный,
  `focused_row_is_pinned_while_scrolled_away`; harness-строки закреплённого ребёнка у
  `RenderSliverList` и `RenderSliverFixedExtentList`.

### Маршруты и окно

- **R7.** КОГДА маршрут открывается (push), СИСТЕМА ДОЛЖНА переносить фокус в новый маршрут;
  поле с `autofocus` перебивает предварительный первый фокус до хода пользователя (в Note — поле
  `Title`). Перекрытый маршрут не держит primary и не получает клавиш. Тест:
  `push_moves_focus_into_the_new_route` — **сделано** для `Focus::autofocus`; не сделано:
  `autofocus` у `TextField`/`TextFormField` и строка приёмки.
- **R8.** КОГДА верхний маршрут закрывается, СИСТЕМА ДОЛЖНА возвращать фокус на открывший его
  элемент; для заметки — на строку той же заметки по ключу, даже если её элемент пересобран (смена
  `Compact rows` меняет геометрию строк); если заметки нет — тот же индекс в границах списка;
  пустой список — сам список (D6); поздний `autofocus` restore не перебивает. Тесты:
  `pop_restores_the_opening_element`, `late_autofocus_does_not_override_the_restored_focus` —
  **сделано** для обычных элементов; строки списка — с R5; строка приёмки.
- **R9.** КОГДА пользователь нажимает Alt+Left, `BrowserBack` или XButton1 на странице, ниже
  которой есть другая, СИСТЕМА ДОЛЖНА закрывать только текущую страницу, сохраняя черновик; на
  Home — ничего. Escape закрывает только модальный маршрут и ничего не делает, пока фокус в поле.
  Клавиша, чей key-down поглотил IME, вместе с её key-up и повторами до навигации не доходит.
  Тесты: `back_keys_pop_only_the_current_page` — **сделано** для клавиш и XButton1, не сделано:
  строка «Alt+Left из поля»; `keys_typed_in_a_field_stay_in_the_field` (Escape в поле) и
  `input_method_keys_never_reach_the_walk` — **сделано**.
- **R10.** КОГДА окно деактивируется и снова активируется, СИСТЕМА ДОЛЖНА возвращать фокус на
  последний сфокусированный элемент и индикатор — в прежнее состояние. Тест: приёмка,
  `window_reactivation_restores_focus`; шаг нативного сценария.

### Сценарии отказа

- **R11.** Клавиша во время перехода маршрута доходит только до верхнего маршрута; отменённый
  Back посреди push возвращает фокус по R8. Тест: `keys_during_transition_reach_only_the_top_route`
  — **сделано**.
- **R12.** Одна активация на одно физическое нажатие; повтор или key-up, чей key-down достался
  другой единице, ничего не делают; Tab, стрелки и символы повторяются. Тесты:
  `key_repeats_follow_their_key_down` — **сделано**; приёмка `held_enter_activates_once`
  (удержание Enter на `Settings` даёт одну страницу Settings).
- **R13a.** Реентерабельные запросы фокуса публикуются по порядку. Тест:
  `reentrant_request_during_notification_is_applied_after_and_published_in_order` — **сделано**.
- **R13b.** После каждого шага клавиатурного обхода `focused` в дереве доступности стоит ровно на
  primary focus. Тест: проверка после каждого шага R2 и R5.
- **R13c.** `FocusAttachment::replace_node` из слушателя во время уведомления отказывает
  `NotificationInFlight` до мутации, и в release-сборке тоже. Тест:
  `nested_replacement_publishes_no_stale_edge` — **сделано**.
- **R14.** Запрос на узел, отпущенный виджетом, — `Stale`, фокус прежний, позже не исполняется.
  Тест: `stale_focus_request_is_inert` — **сделано**.
- **R15.** Элемент, пересобранный с тем же ключом, сохраняет фокус; исчезнувший отдаёт его
  следующему элементу маршрута, без `None`, пока фокусируемые есть. Тест:
  `focus_survives_rebuild_and_moves_on_removal` — **сделано**; строка приёмки «Retry».
- **R16.** Паника `on_press` или слушателя фокуса не ломает менеджер, следующий Tab работает.
  Тест: `keyboard_continues_after_callback_panic` — **сделано**.
- **R17.** Фокус, выставленный указателем, не показывает индикатор; клавиша и assistive
  technology показывают; Material не включает overlay `Focused` после клика. Тест:
  `focus_indicator_follows_input_modality` — **сделано** для `FocusRing` и `RawButton`; строки
  Material — с R4.

### Нативно

- **R18.** КОГДА приложение приёмки запущено в окне на Windows, СИСТЕМА ДОЛЖНА проходить
  сценарий: первый Tab, обход каждого экрана (UIA `HasKeyboardFocus`), End и PageDown, удержание
  Enter на строке, правка `Title`, Enter на `Save note`, Alt+Left и XButton1 с проверкой фокуса,
  Alt+Tab туда и обратно, Retry. Тест: сценарий flui-driver на лейне Windows, датированный прогон.

Карта отказов: unmount — R6, R15; устаревший callback — R14; повтор — R12; реентерабельность —
R13a–c; переход и отмена push — R11; паника — R16; платформа — R17. Исчерпание ID не применимо:
`FocusNodeId` 64-битный и отказывает навсегда.

## Вне scope

- Сочетания приложения (Ctrl+S); стрелочный обход вне списка (`FocusTraversalGroup`, ADR-0165).
- RTL-порядок обхода и high contrast Windows.
- Композиция IME — `text-ime`; скринридер на устройстве — semantics-model и flui-driver.
- Восстановление фокуса после перезапуска (`persistence` его не хранит).
- Android, iOS, Web: только R17 на виджетном уровне.
