# Распознаватели жестов: API — требования

- **Статус:** дизайн утверждён; API и production-потребители интегрированы, итоговая приёмка остаётся открытой ([tasks.md](tasks.md)); scope подтверждён владельцем 2026-10-06 ([../orchestration.md](../orchestration.md), «Scope-решения»)
- **Дата:** 2026-10-06
- **База:** `main` @ `9a4daa3ed` (worktree оркестратора @ `814d77e8d`); строится **поверх** веток
  I1 `interaction/arena-recognizer-lifecycle` @ `2c7e48c00` и I2 `interaction/multi-pointer-recognizers`
  @ `8c234f670`
- **Design:** [design.md](design.md); задачи — [tasks.md](tasks.md)
- **Источники фактов:** [../trait-table.md](../trait-table.md) §2.1–2.8, §2.17–2.19, §3;
  [../ownership-table.md](../ownership-table.md) §A–C, F; [../panic-matrix.md](../panic-matrix.md) R1, R2, D1–D5;
  [../patterns.md](../patterns.md); [../conventions-audit.md](../conventions-audit.md) B (I1, I2)
- **Связанные ADR:** ADR-0027 (owner-local realm), ADR-0086 §4 (арена «не меняется» — заменяется этой
  спекой), ADR-0089 (словарь указателя), ADR-0097, ADR-0127 (удержание на исключительном пути),
  ADR-0151 §4 (Proposed, `SystemPreferences`)

## Зачем

Исходная проблема на базе дизайна: распознаватель — единственная точка расширения
жестов, но пользоваться ею было нельзя. Перечисленные ниже дефекты описывают эту
базу; текущее выполнение и оставшаяся проверка записаны в [tasks.md](tasks.md).

- `GestureRecognizer` не dyn-compatible (`add_pointer(self: &Arc<Self>, ..)`,
  `crates/flui-interaction/src/recognizers/recognizer.rs:47-52`, E0038 подтверждён `rustc 1.99.0`), поэтому
  разнородный набор распознавателей хранить нельзя; в workspace нет ни одного `dyn GestureRecognizer`.
- Ни один виджет не принимает пользовательский распознаватель. Три виджета вручную повторяют одну и ту же
  проводку Listener → `add_pointer` → `handle_event` (`flui-widgets` `interaction/gesture_detector.rs:949-981, 1053-1145`,
  `navigator/back_gesture.rs:286-311, 602-611`, `interaction/draggable.rs:1392-1414`). Trait-table насчитала
  четыре места: четвёртое, ручной Listener в `EditableText` (`text/editable_text.rs:915`), распознаватель не
  проводит и остаётся как есть.
- Документированный путь для своего жеста — `CustomGestureRecognizer` (`sealed.rs:82`) — переименование
  `accept/reject`, теряющее дедлайны (`arena/mod.rs:192-202`); «запечатка» ложная (`pub mod sealed`,
  `lib.rs:153`).
- 45 методов `with_on_*(self: Arc<Self>, ..) -> Arc<Self>` — сеттеры поверх уже разделённого `Arc`: замена
  колбэка видна всем клонам, старый захват дропается под `borrow_mut` (ownership-table §C №7). `Arc` у
  `!Send`-объекта ничего не даёт, кроме атомиков.
- Иерархия Flutter `OneSequenceGestureRecognizer → PrimaryPointerGestureRecognizer` (`one_sequence.rs:23`,
  `primary_pointer.rs:24`) без единого потребителя абстракции; `deadline()`/`did_exceed_deadline()` не
  вызываются.
- Освобождение — `dispose(&self)` + флаг `disposed` + `debug_assert!` как единственная защита от
  use-after-dispose (`recognizer.rs:247-257`). Пропущенный `dispose` оставляет распознаватель в арене
  (цикл арена ↔ распознаватель, ownership-table §B).

Pre-1.0 это дешевле исправить сейчас: внешних пользователей нет, все вызовы — в workspace.

## Решения владельца (scope)

- **(a)** Builder до `Rc` вместо 45 `with_on_*`; `Arc` → `Rc` у распознавателей (колбэки `!Send`, модель
  send-flip a1/b2).
- **(b)** `OneSequenceGestureRecognizer`/`PrimaryPointerGestureRecognizer` → поле-помощник.
- **(c)** Удалить ложные `Sealed`, `Disposable`, `GestureCallback`, `GestureRecognizerExt`,
  `CustomGestureRecognizer`. Hit-test-трейты (`CustomHitTestable`, `HitTestable`, `HitTestTarget`,
  `sealed::{hit_testable, focus_node}`) — зона send-flip T6d: здесь только список передачи.
- **(d)** Dyn-compatible `GestureRecognizer` — точка расширения для своих жестов; запечатывать, только если
  внешняя реализация нелегитимна (решение и обоснование — design D5).
- **(e)** `Listener::recognizer(..)` (или равный типизированный attachment) вместо ручной проводки;
  `add_pointer(dispatch)` вместо `add_pointer`/`add_pointer_with_kind`/`add_pointer_down`.
- Исторический план настроек в следующем пункте заменён
  [ADR-0172](../../../../adr/ADR-0172-host-owned-system-preferences.md): существующий
  `GestureArenaScope` доставляет read-only provider, snapshot сохраняется от admission
  до terminal; см. [design.md §D8](design.md#d8-настройки).
- Дополнительно решить: дедлайны — один `deadline() -> Option<Instant>`; настройки — `Cell<GestureSettings>`
  из `GestureSettingsScope` (I11); освобождение — `Drop` против явного `cancel()` с исходом.

## Не цели

- Переключение primary-указателя на второй палец (Flutter его тоже не делает для primary-распознавателей);
  второй контакт честно не допускается (R9).
- Поверхность `flui-sdk` (ADR-0088): точка расширения выходит через facade `flui::interaction`; в SDK — когда
  её попросит пакет (единственный открытый вопрос, design «Открытый вопрос»).
- `GestureSettingsScope` и чтение `SystemPreferences` — задача I11; здесь только форма поля и builder.
- Подключение или удаление `GestureArenaTeam` и `PointerSignalResolver` — S2; здесь их сигнатуры меняются
  механически (`Arc<dyn>` → `Rc<dyn>`).
- Собственный словарь событий (P2); до него распознаватель сам отвергает non-finite (R13).
- Изменения в `GestureDetector`-API для пользователей (`on_tap`, `on_pan_*`, …): вызовы в `Scrollable`,
  `EditableText`, `InkWell` не меняются.

## Требования

Нумерация `R*` — только в этом каталоге. «Проверка» называет тест (имя — в design «Тестовая стратегия»).

### Конструирование и владение

- **R1.** КОГДА автор создаёт распознаватель, СИСТЕМА ДОЛЖНА принимать колбэки и настройки только в builder,
  который возвращает `Rc<Self>` из `build()`; после `build` колбэк заменить нельзя. Проверка:
  `rg 'fn with_on_|self: Arc<Self>|self: &Arc<Self>' crates/flui-interaction/src` пуст; builder каждого из 10
  распознавателей покрыт строкой таблицы `built_recognizer_delivers_each_callback`.
- **R2.** КОГДА распознаватель, его builder или `RecognizerSet` передаются в другой поток, СИСТЕМА ДОЛЖНА
  отказывать при компиляции. Проверка: trybuild `recognizer_stays_on_its_thread` (E0277).
- **R3.** КОГДА код хранит разнородные распознаватели, СИСТЕМА ДОЛЖНА принимать `&dyn GestureRecognizer`,
  `Rc<dyn GestureRecognizer>` и повышать их до `Rc<dyn GestureArenaMember>`. Проверка: строка
  `dyn_compatible_extension_points` (до изменения — E0038) и поведенческий
  `heterogeneous_set_drives_builtin_and_custom_recognizers`.
- **R4.** КОГДА сторонний крейт реализует `GestureRecognizer` поверх публичных помощников (`PrimaryContact`,
  `ArenaMembership`) и подключает его через `Listener::recognizer`, СИСТЕМА ДОЛЖНА доставлять ему
  Down/Move/Up/Cancel, арбитраж арены и собственный дедлайн так же, как встроенному. Проверка: тест
  `custom_recognizer_competes_through_a_listener` в `tests/fixtures/facade_extensions.rs` (`tests/facade_consumer.rs:188`
  собирает его отдельным крейтом-потребителем, группа `nested-cargo`: только публичный API facade).

### Вход указателя

- **R5.** КОГДА Listener получает `Down`, СИСТЕМА ДОЛЖНА передавать распознавателю весь `PointerDispatch`
  одним методом `add_pointer(dispatch)`; тип устройства, кнопка и обе системы координат берутся из события.
  Проверка: `rg 'add_pointer_with_kind|add_pointer_down'` пуст; `double_tap_reports_the_device_kind_of_the_down`.
- **R6.** КОГДА к Listener подключены распознаватели, СИСТЕМА ДОЛЖНА применять предикат допуска только к
  `Down`, доставлять `Move`/`Up`/`Cancel` всем подключённым в порядке подключения, а распознаватель — игнорировать
  указатель, который он не отслеживает. Проверка: `listener_admits_by_predicate_and_forwards_terminal_events`.
- **R6a.** КОГДА rebuild снимает колбэки семейства посреди его жеста (например, `on_pan_*` во время drag),
  СИСТЕМА ДОЛЖНА доставить этому распознавателю `Up`/`Cancel` и вернуть его в `Ready`. Сегодня
  `RecognizerGroup::forward` гейтит drag по живому слоту (`gesture_detector.rs:1139-1144`), и drag залипает.
  Проверка: `clearing_pan_callbacks_mid_drag_still_finishes_the_drag`.
- **R7.** КОГДА участник арены взводит дедлайн, СИСТЕМА ДОЛЖНА узнавать его из одного
  `deadline() -> Option<Instant>` и вызывать `poll_deadline(now)` только для наступивших дедлайнов с часами
  арены. Пользовательский участник владеет дедлайном так же, как long press. Проверка:
  `custom_member_owns_a_deadline`; `has_pending_deadlines == next_deadline().is_some()` по построению.
- **R8.** КОГДА распознаватель допускает контакт, СИСТЕМА ДОЛЖНА зафиксировать настройки и тип устройства
  контакта на всю последовательность; slop — по типу устройства; неизвестный тип — slop касания (наибольший).
  Проверка: строки `slop_follows_the_contact_device`.
- **R9.** КОГДА второй указатель нажимает, пока распознаватель с одним контактом отслеживает первый, СИСТЕМА
  ДОЛЖНА не допускать второй и не трогать первый (`BeginContactError::Busy`). Проверка:
  `second_pointer_does_not_replace_the_primary_contact`.

### Освобождение и отмена

- **R10.** КОГДА владелец роняет последний `Rc` распознавателя, СИСТЕМА ДОЛЖНА ровно один раз: снять его со
  всех арен (решение арены — отложенно, без чужого пользовательского кода внутри `Drop`) и освободить
  захваты колбэков по ADR-0127. Флага `disposed` и метода `dispose` нет. Проверка:
  `dropping_a_recognizer_withdraws_it_from_the_arena`.
- **R11.** КОГДА владелец вызывает `cancel()`, СИСТЕМА ДОЛЖНА прекратить текущую последовательность, доставить
  колбэк отмены не более одного раза, снять участие в арене и вернуть `CancelOutcome::{Idle, Cancelled}`;
  распознаватель остаётся пригодным для следующего `Down`. Проверка: `cancel_reports_what_it_cancelled`.

### Отказы (каждая точка, две в конкуренции, следующая операция)

- **F1.** КОГДА колбэк распознавателя синхронно размонтирует свой виджет (владелец роняет последний `Rc`),
  СИСТЕМА ДОЛЖНА завершить текущий вызов без `BorrowError`, не вызывать больше колбэков этого распознавателя,
  оставить арену согласованной; следующий `Down` обслуживается. Проверка:
  `callback_that_unmounts_its_detector_finishes_the_event`.
- **F2.** КОГДА колбэк вызывает `cancel()` своего распознавателя, СИСТЕМА ДОЛЖНА не доставлять оставшиеся
  уведомления того же события (их контакт устарел) и допустить следующий `Down`. Проверка:
  `cancel_from_a_callback_drops_the_stale_notices`.
- **F3.** КОГДА один колбэк паникует, СИСТЕМА ДОЛЖНА зафиксировать состояние распознавателя до вызова,
  вернуть панику на границу lane/арены и распознать следующий жест. Проверка: таблица
  `next_gesture_after_a_callback_panic` (строка на распознаватель; закрывает D1, D2, D6).
- **F4.** КОГДА паникуют два распознавателя на одном указателе (два attachment одного Listener или два
  участника арены), СИСТЕМА ДОЛЖНА доставить событие каждому, вернуть первую панику, остальные payload —
  удержать. Проверка: `two_recognizers_panicking_on_one_pointer_keep_the_first_payload`.
- **F5.** КОГДА `Drop` захвата колбэка паникует при освобождении распознавателя, СИСТЕМА ДОЛЖНА освободить
  остальные захваты и вернуть первую панику; при уже идущей раскрутке — удержать все захваты без abort.
  Проверка: `dropping_a_recognizer_retires_each_capture_and_resumes_the_first_failure`,
  `dropping_a_recognizer_during_unwind_retains_its_captures` (в дочернем процессе, как строки drag).
- **F6.** КОГДА `cancel()` одного распознавателя паникует при `dispose` виджета, СИСТЕМА ДОЛЖНА отменить
  остальные распознаватели виджета и вернуть первую панику (`cancel_all`). Проверка:
  `cancel_all_cancels_every_recognizer_before_resuming_the_first_panic`.
- **F7.** КОГДА после F3–F6 приходит следующий `Down`, СИСТЕМА ДОЛЖНА распознать жест с нуля. Проверка —
  последняя колонка каждой из таблиц F3–F6.
- **F8.** КОГДА виджет размонтируется посреди жеста, СИСТЕМА ДОЛЖНА доставить отмену один раз, освободить
  распознаватель, игнорировать события кэшированного маршрута до терминального и отдать арену соперникам.
  Проверка: `unmount_mid_drag_cancels_once_and_hands_the_arena_to_the_rival`.
- **F9.** КОГДА два указателя нажимают в одном кадре, СИСТЕМА ДОЛЖНА решать арену каждого указателя
  независимо, а результат — не зависеть от порядка их `Up`. Проверка:
  `two_pointers_resolve_independently_in_either_up_order`.
- **F10.** КОГДА участник арены уронен без `cancel` (владелец забыл), СИСТЕМА ДОЛЖНА считать его вышедшим
  при следующем разборе отложенных решений: соперник побеждает, слабых ссылок на мёртвый участник не
  остаётся, его колбэки не вызываются. Проверка: `dropped_member_without_cancel_leaves_the_arena` (до
  изменения: арена держит сильный `Arc`, сброс отдаёт победу уроненному).
- **F11.** КОГДА позиция `Down` не конечна, СИСТЕМА ДОЛЖНА не допускать контакт; КОГДА non-finite приходит в
  `Move`, СИСТЕМА ДОЛЖНА считать slop превышенным и выйти из арены; non-finite не публикуется в деталях.
  Проверка: NaN-строки таблицы `slop_follows_the_contact_device`.
- **F12.** КОГДА кнопка или устройство не поддерживаются распознавателем, СИСТЕМА ДОЛЖНА не допускать
  контакт (правило I1 для кнопок сохраняется). Проверка: строки `unsupported_button_is_not_admitted`.
- **F13.** КОГДА пользовательский `deadline()` или `poll_deadline()` паникует, СИСТЕМА ДОЛЖНА опросить
  остальных участников, считать дедлайн паникующего отсутствующим и вернуть первую панику после обхода.
  Проверка: `panicking_deadline_query_does_not_hide_other_deadlines`.
- **F14.** КОГДА счётчик идентичности контакта исчерпан, СИСТЕМА ДОЛЖНА остановиться с `BUG:` (`strict_add`;
  2^64 контактов недостижимо), а не переиздать старый `ContactId`.

### Миграция и документация

- **R12.** КОГДА API меняется, СИСТЕМА ДОЛЖНА иметь мигрированными все вызовы workspace (таблица design
  «Миграция»), фрагмент `changelog.d/`, ADR, заменяющий ADR-0086 §4, и обновлённые
  `crates/flui-interaction/docs/GESTURES.md`, `docs/ARCHITECTURE.md`, `README.md`, пример
  `examples/custom_recognizer.rs`.
- **R13.** КОГДА меняется путь доставки (`Weak::upgrade` на событие, `dyn` вызов), СИСТЕМА ДОЛЖНА показать
  бенч до/после на одном хосте: `tap_detector_bench` (static vs dyn `handle_event`) и `gesture_arena_bench`
  (сильный vs слабый участник). Регрессия больше 10 % на строку — объясняется в PR или устраняется.
