# Полнота ввода: ядро без пакетов-заплаток — задачи

- **Статус:** в работе. Интеграционная ветка `interaction/completeness` слита в `main` (#1733), строки
  со статусом `merged` на `main`; оставшиеся задачи идут отдельными PR прямо в `main`
- **Дата:** 2026-10-13
- **Требования:** [requirements.md](requirements.md); design — [design.md](design.md)
- **Правила:** задача = ветка = worktree = PR. Каждое поведение — тест через публичный API, красный с
  откатом изменения (вывод в PR); `cargo xtask check-changed` зелёный; нативный путь, не исполненный на
  хосте, помечен в PR. ID задач действуют только здесь. Задачи одного яруса идут параллельно, когда их
  зависимости слиты. Ничего не откладывается (S3). Каждая задача добавляет свои строки `surface.rs`,
  таблицы размещения, проверки соответствия и матрицы доступности (E1, R6, A6); чеклист N1–N5 — в каждом PR.
  Задача, меняющая поддержку на платформе, обновляет свою строку в `docs/platform-parity.md` (✅ — только
  после прогона на этой платформе) и пункт в `docs/gaps.md` в той же ветке.
- **Статусы:** `todo`, `in-progress`, `merged`, `blocked-external` (ждёт внешнего релиза, E4).

## Ярус 0 — документы

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T01 | ADR из design §16 (11 шт.), перепроверка `../matrix.md`, вывод M2-R3 и M3-M3 из OUT; docs-only PR со спекой | §16, S3 | — | merged |

## Ярус 1 — основание

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T02 | Слои модулей `flui-interaction` | M1, ADR-0217 | T01 | merged |
| T03 | Сборка указателя, чистые функции разбора, часы, `ClickCounter`, исходное нативное событие на контакт; бюджет без аллокаций | P2, P3, P4, Q3, ADR-0217 | T01; Win32 QPC — ветка `platform/win32-pointer-qpc-time` | merged |
| T05 | `SystemPreferences` и пресеты `preset(platform, kind)` с цепочкой отката (только числа) | S13, H4, ADR-0212 | T01 | merged |

## Ярус 2

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T04 | Словарь: `Scroll` с фазами для прокрутки двумя пальцами, `PanZoom` только масштаб/поворот, `Revise`, поля пера, `PointerKind::Virtual`, `InputCapabilities`/`Availability` за `unstable-input`, удаление `PlatformCapabilities`; аудит всех `_`-ветвей потребителей; `bitflags` для `PointerKindSet` и вместо самописного `TransferActions(u8)`; workspace-записи для `android-activity`, `slab`, `smallvec` | P1, P5, P6, B5, S25, L, ADR-0213, ADR-0217 | T02 | merged |
| T06 | Сервисы `GestureBinding` с ключом `InputSurface`, `drain(&self)`; статическая проверка `!Send` для `GestureBinding`, `compile_pass` с `Rc`-захватами | M2, Q2, Z1 | T02 | merged |

## Ярус 3 — позвоночник

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T07 | `HostInput` (во `flui-runtime`), шкала времени (дедлайны на `EventTime`), `InputSequenceId`, очередь реентерабельных нативных событий, детерминизм двойного прогона на рантайм; снятие `+ Send` с `on_input` после регистрации в потоке владельца (ADR-0082); вид указателя доходит до hit-test (`min_target` у `HitSlop` и пресет `preset(platform, kind)` выбираются по виду контакта, а не одним профилем презентации) | B1–B4, Z5, Z6, ADR-0207 | T04, T06 | merged |
| T08 | Запись и скрипты: `InputTap`, DTO, фрейминг, круговой gate вариантов, builder'ы устройств, текстовые скрипты, поиски по semantics, строки кадр/события | R1, R2, R4, R5, R7, R9, R10, Q1, Q4, ADR-0215 | T07, T05 | merged |
| T09 | `InputRegions` (протокол, render-объект с harness, сбор на окно, метод окна), `SystemGestureExclusion` | P7, ADR-0217 | T04 | merged |
| T10 | `HitSlop` (визуальное → край → z, без пересечения слоёв), hit-test по фигуре (render-объект с harness) | W9, W10 | T05 | merged |
| T11 | `OneEuro` после распознавания | S17 | T05 | merged |

## Ярус 4

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T12 | Каркас `assert_backend_conforms` (указатель, часы, грамматика `Scroll`/`PanZoom`) | R6, ADR-0215 | T03, T08 | merged |
| T13 | Нативный кодек в тестах `flui-platform`, проигрывание в Linux CI | R3, ADR-0215 | T03, T08 | merged |
| T14 | Арена: таблица владения, `PanClaim`, `allowed_devices`, накопленное смещение, `Sequenced`, вложенная композиция, `max_resolution_delay`, отмена проигравшим и `Listener`, `assert_ownership_table` | G1–G6, ADR-0208 | T07, T05 | merged |
| T15 | `GestureObserver` (очередь, паники, без аллокаций) | S19, Q3 | T07 | merged |
| T16 | Haptics: уровни 1–2, `HapticRequest`, `Queued`, очередь с дедлайном, устаревший источник; строки проверки | H1–H5, ADR-0210 | T07, T05, T12 | merged |

## Ярус 5 — фокус для T19 (задачи спеки focus-keyboard, сделаны здесь)

T19 были нужны модальность ввода, Back/Dismiss и правило клавиш во время перехода маршрута из спеки
[focus-keyboard](../../focus-keyboard/tasks.md). Её задачи ниже сделаны в интеграционной ветке и слиты с ней;
остальное (T2, T4 → T33, T9, T11 → T33, T12–T16) остаётся в её плане.

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| FK3 | focus-keyboard T3 целиком (ядро `flui-interaction`: `Stale`, `NotificationInFlight`, `FocusCause`, модальность, журнал клавиш, вход обхода, маркер сирот) с нужной ей частью контракта T1 | focus-keyboard T3 | T02 | merged |
| FK6 | focus-keyboard T6 и T7 целиком (рантайм: `note_pointer_input`, X1 → `BrowserBack`, `forget_pressed_keys`; `settle_orphaned_focus` в кадре после fixpoint — без него FK3 оставляет фокус на удалённом узле до следующего ввода) | focus-keyboard T6, T7 | FK3, T07 | merged |
| FK8 | focus-keyboard T8 целиком (`on_release`, Activation/Back/Dismiss, таблица `DefaultFocusTraversal`, `FocusRing`, `RawButton`, `Focus` `Assistive`) | focus-keyboard T8 | FK3 | merged |
| FK10r | focus-keyboard T10 целиком: R1 (первый Tab входит в текущий маршрут), R7 (первый фокус push предварителен, `autofocus` его перебивает до хода пользователя), R8 (restore pop окончателен), R9 (Back/Dismiss), R11 (клавиши только верхнему маршруту, push, отменённый Back посреди перехода, возвращает фокус) | focus-keyboard T10 | FK8 | merged |

## Ярус 5

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T17 | `RawGestureDetector` (фабрика, `update(&config)`), поверхность SDK, нестандартный распознаватель с пересборкой посреди жеста, шаблон-чеклист | W2, E1–E3, E5 | T14 | merged |
| T18 | `AxisLock`, `TransformLock`, `SwipeGestureRecognizer`, поворот как Scale, hold на Down как правило арены, замирание hover | G7–G10, ADR-0208 | T14 | merged |
| T19 | `GestureAlternatives`, `assert_gesture_alternatives`, клавиши как `Shortcuts` платформы, локализация с параметрами, диагностика без `Intent` | A1–A5, A9, ADR-0216 | T17; semantics `semantics/custom-action-ids` (слит); FK3, FK6, FK8, FK10r | merged; из A5 слиты параметризованные метки (`context_menu_button_label`, `page_label` — позиция и число страниц `PageView`), объявление позиции после перемещения — в T33 |

## Ярус 6

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T20 | `GestureDetector` на слотах поверх `RawGestureDetector` | W1, S17 | T17, T18, T19 | merged |
| T21 | Канал прокрутки: один примитив смещения, лист без контакта, latch и таймауты, fling с наследованием, `reveal`, `ScrollEndReason`, замена endpoint ADR-0169; `Scrollable` на осевом drag; строка C1 | C1, C5–C9, Q3, ADR-0209 | T18, T08 | merged |
| T22 | `GestureInspector` | S19 | T15, T17 | merged |

## Ярус 7

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T23 | `SnapPolicy`; `PageView` = `Scrollable` + `SnapPolicy` | C10, W11, ADR-0209 | T21 | merged |
| T24 | Pull-to-refresh, строки C3 и матрицы | C3 | T21, T19 | merged |
| T25 | Сворачивающийся заголовок, строки C4 (включая Tab и AT) | C4 | T21, T19 | merged |
| T26 | `InteractiveViewer` как лист канала, строки C2 | C2, W6 | T21, T18, T19 | merged |
| T27 | `AnchoredDrag` (одномерный, `SnapPolicy`), haptics на якорях | W3 | T23, T16 | merged |
| T28 | DnD: ядро сессий, идентичность, эскалация в ОС, доставка в runtime | D1, D2, D6, ADR-0211 | T21, T07 | merged |

## Ярус 8

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T29 | DnD: `Draggable`/`DragTarget` на сессиях, `DragTrigger` через `Sequenced`, Esc и Cut/Copy/Paste, курсор | D3, D4 | T28, T19, T16 | merged; строка матрицы `draggable_and_drop_target_by_every_path` — перенос из двух команд на двух виджетах (Cut у источника, Paste у цели), поэтому не `assert_gesture_alternatives`: она сама проходит мышь, палец и виртуальный указатель, минимальную цель и reduced motion |
| T30 | DnD: spring-loading, автопрокрутка, несколько сессий, drop последней принявшей цели | D3, D4 | T28 | merged |
| T31 | `Dismissible` на `AnchoredDrag`, `SwipeActions`, системные края | W4 | T27, T09, T19, T32 | merged |
| T32 | `ContextMenuArea` на `Sequenced` | W7 | T14, T19 | merged |
| T33 | `ReorderableList`, `ReorderableGrid` как DnD-сессия, keep-alive, A8; объявление новой позиции после перемещения — метод `WidgetsLocalizations` с параметрами (элемент, позиция, число), добавляется здесь вместе с его единственным производителем | W5, A8, A5 (объявление позиции) | T29, T30; focus-keyboard T4, T11 | todo |
| T34 | `SelectionArea` (указатель, клавиатура, AT, ручки на `Sequenced`, `TapAndDrag`) | W8, S17, ADR-0214 | T17, T19, T14; поправка ADR-0204 (T01); спека text-ime | в работе: семантика и AT-путь (#2034), подсветка и `SelectionStyle` (#2044) готовы; геометрический порядок чтения в документе (PR 3a); указатель, клавиатура, Copy, затемнение при потере фокуса, перенос конца выделения с ушедшего текста и тот же порядок без семантики (PR 3b), ручки касанием: долгое нажатие выделяет слово, ручки в оверлее за концами через `LayerLink`, перетаскивание ручки двигает её конец (PR 4, #2077); публичный `SelectionController` с причиной изменения в `on_selection_changed` (PR 5, #2098); продолжение долгого нажатия тем же касанием: движение пальца расширяет выделение по словам, первое слово остаётся выделенным; распознаватель долгого нажатия сам держит контакт, передача `Sequenced` не нужна (PR 6); прогоны у ссылок внутри выделяемого текста: прогоны режутся на границах ссылок, регионы ссылок и текста между ними держат свои диапазоны прогонов абзаца, веб-зеркало показывает их текст (PR 7, ADR-0282); прогоны с переопределениями доступности у спанов: переопределённый спан — свой регион с переопределением как чтением и авторскими прогонами, заглушённый — скрытый регион (PR 8, ADR-0283); тулбар выделения — отдельная задача после v1 |
| T34b | `EditableText` берёт цвет выделения из `DefaultSelectionStyle` вместо своего `selection_color`, чтобы у выделения был один источник цвета (ломает API, допустимо до релиза) | S17 | T34 (PR 2) | merged (#2101): `EditableText::selection_style`, активный цвет при фокусе и приглушённый после blur, `ThemeData::selection` в Material отдаёт стиль из цветовой схемы |
| T34c | `RenderEditable` получает тот же `set_selection_handles`, что `RenderParagraph`, когда у `EditableText` появятся ручки: один контракт ручек у обоих рендер-объектов | S17 | T34 (PR 4) | done |
| T34d | Ручки выделения под масштабом или поворотом: `LinkFollower` берёт только сдвиг лидера (`resolve_follower_offset` суммирует `local_translation`), поэтому под `Transform::scale` ручка уходит от конца выделения. Нужен полный перенос лидер→оверлей в `flui-layer` для всех ведомых (ручки, подсказки, меню) | S17 | T34 (PR 4) | merged (#2093, ADR-0276) |
| T34e | Ручки выделения прячутся, когда их конец вне отсечения предков (прокрутка, `ClipRect`): ведомый в оверлее не наследует отсечение лидера, поэтому ручка конца, уехавшего за край вьюпорта, остаётся видимой и перетаскиваемой. Нужен признак видимости лидера, который публикует отрисовка | S17 | T34 (PR 4) | merged (#2093: `LeaderClip::Hide`, ручки на нём; `a_handle_scrolled_out_of_view_takes_no_touch`) |
| T34f | Ручки выделения следят за перекладкой абзаца без пересборки (уже ширина переносит крупный спан с линии конца): сейчас высоту линии перечитывает проверка после сборки ручек или текста, а слежение после каждого кадра в презентации само просит кадры. Нужен сигнал от `RenderParagraph` о новой раскладке, не будящий кадр | S17 | T34 (PR 4) | merged (#2114: `LeaderWatch`; `shown_handles_follow_a_relayout_without_a_build`) |
| T35 | Несколько окон и рантаймов: DnD между окнами, захват при drag над другим окном | Q2, D2 | T28 | merged |

## Ярус 9 — Windows

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T36 | DirectManipulation: `Scroll` с фазами и `PanZoom`, прокачка, свой запрос кадра, модальный цикл размера | P8, S6 | T03, T13, T21 | merged; на тачпаде не исполнено (V1) |
| T37 | OLE DnD: источник и цель, отложенный старт, кадры всем рантаймам, перерисовка второго окна | P8, D5 | T28 | merged; без настоящей мыши и другого приложения (V3) |
| T38 | Haptics Windows: перо; `InputHapticsManager` — `Unavailable` | P8, H1 | T16 | merged; перо не вибрировало (V2) |
| T38a | Подъём `windows` с `InputHapticsManager` | P8 | внешний релиз | blocked-external |
| T39 | Golden-записи и `xtask device windows-input` | R8 | T36, T37 | merged; golden DirectManipulation написан вручную, R8 для тачпада не выполнено (V1) |

## Ярус 10 — остальные ОС (на каждую ОС: ввод, DnD, haptics, IME, инъекция и записи)

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T40a–e | macOS: ввод · DnD · haptics · IME-проверка · `CGEvent` и записи | P9, P14, R8 | T12 и ядро функции (T21, T28, T16) | todo |
| T41a–e | iOS: ввод · DnD · haptics · `UITextInput` · XCUITest и записи | P10, P14, R8 | T12, T09 и ядро функции | todo |
| T42a–e | Android: ввод · DnD · haptics · `InputConnection` · `adb shell input`, `uinput` и записи (ADR-0215 §8) | P11, P14, R8 | T12, T09 и ядро функции | merged; часть путей не исполнена (V4), таблица порядка IME не построена (V6) |
| T43a–c | Linux: `composing` и `Unavailable`-пути · uinput/libei и записи · после T43x — перо, планшет, DnD | P12, R8 | T12 | todo |
| T43x | Подъём winit до 0.31 | P12 | внешний релиз | blocked-external |
| T44a–e | Web: ввод и синхронный `preventDefault` · HTML DnD · вибрация · IME (`EditContext`/`<textarea>`) · BiDi и записи | P13, P14, P15, R8, Q4 | T12 и ядро функции; спека web-canvas-input-ownership | merged; исполнено только в headless Chrome (V5), таблица порядка IME не построена (V6) |

## Не исполнено на железе и не построено

Слитые задачи, чей нативный путь собран и проверен синтетически, но не шёл на устройстве, которое его
сертифицирует. Строка уходит, когда прогон показан в PR и в `docs/platform-parity.md`.

| ID | Задачи | Что не исполнено | Что нужно |
|---|---|---|---|
| V1 | T36, T39 | Прокрутка и pinch DirectManipulation не шли на прецизионном тачпаде: окно, прокачка, запрос кадра, пауза размера и teardown исполнены на настоящем окне с синтетическими показаниями viewport. `win32_direct_manipulation.jsonl` написан вручную по документированному порядку колбэков (так и сказано в его `provenance`), а не записан; R8 для тачпада не выполнено | Прогон `native-capture` на Windows с прецизионным тачпадом: записанный golden вместо рукописного и знаки осей из `crates/flui-platform/ARCHITECTURE.md` |
| V2 | T38 | Ни одно перо не вибрировало: актуатор настоящего окна исполнен на хосте без haptic-пера, каждый запрос `Unavailable` | Прогон с пером, у которого есть `SimpleHapticsController` |
| V3 | T37 | OLE DnD исполнен в одном процессе с синтетическим вводом (отложенный старт, кадры второго окна, drop в окно FLUI); ни с зажатой настоящей кнопкой мыши, ни с пером или касанием, ни в другое приложение | Ручной прогон перетаскивания мышью в другое приложение и из него |
| V4 | T42a–e | Envelopes (API 36) и compositions не исполнены (у Galaxy S21 нет primitives); тачпад и стилус — только `uinput` и `input stylus`, без аппаратных; `CursorAnchorInfo`, Gboard и рукописный ввод не исполнены; DnD принят только окном самого probe | Устройство с API 36 и primitives, аппаратные тачпад и стилус, Gboard |
| V5 | T44a–e | Всё исполнено в headless Chrome: вибрация без мотора, pinch трекпада из сконструированных событий, IME — путь Chrome без IME ОС и экранной клавиатуры; ни Safari, ни Firefox, ни трекпада | Прогоны в Safari и Firefox, с трекпадом, на телефоне с экранной клавиатурой |
| V6 | T42d, T44d | Таблица порядка событий IME на ОС (P14) — одна строка проверки, `a_key_during_composition_is_the_input_methods` (Esc при композиции, Web и Android); порядок правок `TextStore` по ОС строками проверки не закреплён. Это шаги проверки для композиции и строки на каждый IME-путь (TSF, `InputConnection`, `EditContext`/`<textarea>`, затем T40d, T41d) | Решение владельца: строить здесь или в спеке text-ime |

## Ярус 11 — завершение

| ID | Задача | Требования | Зависит от | Статус |
|---|---|---|---|---|
| T45 | Material/Cupertino: обёртки стиля | W12 | каждый виджет | todo |
| T46 | Вклад в AccessKit (custom actions, `ShowContextMenu`) | A7 | — (в любой момент) | todo |
| T47 | Сборка матрицы A6 и проверка версии адаптеров | A6 | T24–T26, T29–T34; focus-keyboard T15 (нативный тур) | todo |
| T48 | Аудит достижимости `pub`, имён и конвенций | M3, N1–N5 | все | todo |

## Внешние связи

- semantics-model: `semantics/custom-action-ids` (PR #1606 слит: `Semantics::custom_action(label, handler)`, `CustomActionId`), W7 (announcer), строки W5 жестовых виджетов
  (design.md спеки semantics-model, раздел (e), ветка `semantics/architecture-decisions`).
- focus-keyboard: T3, T4 (pinned keep-alive slot), T6, T10, T11, T15.
- text-ime, web-canvas-input-ownership — для T34, T44.
- Win32 QPC — ветка `platform/win32-pointer-qpc-time`.
