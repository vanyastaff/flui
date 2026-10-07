# flui-interaction — аудит спек и веток волны 1 по «Конвенциям Rust»

Конвенции — `rust-conventions-section.md` (пункты ниже ссылаются на его разделы:
Типы / Владение / Ошибки / API / Арифметика / Документация / Линты).
Ветки прочитаны read-only (`git diff origin/main` + файлы worktree); ничего не собиралось, кроме
отдельно оговорённого. Строки — в файлах после изменения.

## A. Спеки

| Документ:строка | Пункт конвенции | Статус | Предлагаемая правка |
|---|---|---|---|
| orchestration.md:49-50 | API: non_exhaustive, newtype-ID, enum вместо bool, builders | соответствует | Дополнить: `#[must_use]` на исходах/handle/guard; `Duration`/`EventTime` вместо f64-секунд; `try_new` для диапазонных параметров. Заменить строку ссылкой на новый раздел «Конвенции Rust». |
| orchestration.md:51-53 | Владение: guard не держится при вызове user code | соответствует по сути, без точного правила | Вписать правило edition 2024 (временные `if let`-скрутини живут весь then-блок; `match` — все ветки; `let x = c.borrow().f();` — до `;`), подтверждённое probe. |
| orchestration.md:51-53 | Владение: `dashmap`/`parking_lot` только при межпоточности | соответствует | Назвать нарушителей, у которых нет задачи: `arena/mod.rs:62-63,248,309,824-830` (`GestureArena` закреплён `!Send+!Sync`, `:1777`) и 10 × `settings: Arc<Mutex<GestureSettings>>` в `recognizers/*`. |
| orchestration.md:57-58 | Арифметика: NaN/inf не публикуются | частично | Добавить политику: проверка на входе (`try_new`/`Option`), не «clamp и надежда»; `f64::clamp` паникует при `min > max` или NaN-границе — границы проверяются до вызова; сортировка f64 — `total_cmp` (1.62). |
| orchestration.md:59 | Ошибки: thiserror, без unwrap | соответствует | Добавить: `debug_assert!` не единственная проверка входа вызывающего; `expect("BUG: …")` только на инварианте модуля. |
| orchestration.md:60 | Документация: rustdoc на каждом pub | частично | Добавить `# Errors`/`# Panics`/`# Examples`, доктесты компилируются (сейчас 50 блоков `ignore`/`no_run` в `src/`). |
| orchestration.md (нет) | Линты | отсутствует | Раздел «Линты» с измеренными счётчиками (см. conventions). |
| orchestration.md (нет) | Арифметика: счётчики/ID — `checked_*` с постоянным отказом | отсутствует | Добавить; нарушитель — `arena/signal_resolver.rs:151-152` (`next_handler_id += 1`). |
| tasks.md:27 (I5) | Владение | соответствует | Расширить I5 (или новая карточка волны 2): «`GestureArena`: `DashMap`/`parking_lot` → `RefCell`/slab; распознаватели: `Arc<Mutex<GestureSettings>>` → `Cell`; `with_on_*(self: Arc<Self>)` → builder до `Rc`». Сейчас matrix X5 это описывает, но задача не владеет. |
| tasks.md:37 (S1) | API | соответствует | Добавить в перечень: `ids.rs:223` `pub type DeviceId = i32` → newtype из словаря P1; `traits.rs:188 Disposable`, `recognizers/one_sequence.rs:23`, `primary_pointer.rs:24` — удалить иерархию; `add_listener/remove_listener` → `Subscription`; `focus.rs:24` `-> bool` → `KeyEventResult`; `focus_scope.rs:1826 step(.., forward: bool)`; `processing/velocity.rs` `get_*` и `allow_slow: bool`. |
| tasks.md:39 (S3) | Документация | соответствует | Уточнить счёт: 50 `ignore`/`no_run` в `src/` (наибольшие: `arena/team.rs` 4, `events.rs` 4). |
| tasks.md:23 (I1) | Тесты | соответствует | proptest уже в workspace (`Cargo.toml:472 proptest = "1"`); ок. |
| matrix.md:107 (X5) | Арифметика/ADR-0098 | **устарело** | `px_f32` в `binding.rs:264-268` уже тождество `f64 → f64` (сужения нет), а doc-комментарий («intentionally lossy») неверен. Правка: удалить функцию (pass-through без поведения) и исправить evidence X5. |
| matrix.md:72 (T3) | Типы | соответствует | Уточнить: `allowed_buttons: PointerButtons` (набор из словаря P1), не `Vec`/bitmask-`u32`. |
| matrix.md:87 (S4) | Типы | соответствует | «ordered SmallVec» — ок; добавить: `clippy::iter_over_hash_type` не ловит `.values()`-цепочки, поэтому правило — тип, не линт. |
| matrix.md:97 (V6) | Арифметика | соответствует | Добавить: `clamp` магнитуды через направление (как сделано в ветке I3 `velocity.rs:254-288`). |
| matrix.md:128 (C5) | Типы | соответствует | Ветка I4 выбрала `Option<CursorIcon>` (None = defer) — допустимо, если rustdoc говорит «None — наследовать». |
| matrix.md:121 (H8) | Владение (RAII) | соответствует | `PointerCapture` токен с release on drop — `#[must_use]`. |
| focus-keyboard/design.md:260 | API: `#[must_use]` | нарушает | `#[must_use] #[non_exhaustive] pub enum FocusRequestOutcome`. |
| focus-keyboard/design.md:261 | Ошибки | частично | `FocusTreeError` — `#[non_exhaustive]` + `thiserror::Error`. |
| focus-keyboard/design.md:276 | Типы: enum вместо bool | нарушает | `dispatch_key_event(&self, &KeyEvent) -> KeyEventResult` (`#[must_use]`). |
| focus-keyboard/design.md:269 | Владение (RAII) | соответствует | `register_traversal_entry -> FocusNodeRegistration` — guard; отметить `#[must_use]`. |
| send-flip/design.md:57-59 (a1), :75 (b2) | Владение | соответствует | Owner-local `Rc<RefCell>` выбран; flui-interaction должен ему следовать (см. tasks I5-расширение). |
| pointer-vocabulary/design.md:68 | API | текст ≠ код | Код P1: `PointerSample` с pub-полями валидированных newtype + `#[non_exhaustive]`, `new(EventTime, PointerPosition) -> Self`. Исправить текст design (форма кода лучше: валидация в типе поля). |
| pointer-vocabulary/design.md:72-73 | Типы: нелегальные состояния | частично | `PointerEvent::Down(PointerButtonEvent)` допускает событие, построенное с `ButtonTransition::Released` (переход не хранится). Конструкторы `PointerEvent::down(..)`/`up(..)`/`button_change(..)` вместо публичных вариантов-кортежей, либо хранить `transition`. |

## B. Ветки волны 1

Серьёзность: **H** — блокирует, **M** — исправить в этой ветке, **L** — можно следом.

### I1 — арена/распознаватели (`interaction/arena-recognizer-lifecycle`)

| Файл:строка | Пункт | Сев. | Исправление |
|---|---|---|---|
| crates/flui-interaction/src/recognizers/tap.rs:925,932 (+ `:895` `resolve_pointer`) | Типы: identity ≠ label | M | Вердикт ищет «новейшую последовательность на pointer» — поздний вердикт старого клика попадёт в следующий (мышь переиспользует id). Решения — только через `TapArenaMember` последовательности; методы распознавателя — no-op с комментарием. |
| crates/flui-interaction/src/recognizers/recognizer.rs:38,66-67; drag.rs:469,513,804; multidrag.rs:381 | Типы: сентинел | M | `event_nanos: u64` с `0` = «нет времени» → `Option<EventTime>`/`Option<u64>`. |
| crates/flui-interaction/src/recognizers/tap.rs:138 | Владение | M | `sequences: Arc<Mutex<TapSequences>>` в `!Send` распознавателе → `RefCell<TapSequences>`; `TapArenaMember` держит `Weak<Self>` распознавателя. |
| crates/flui-interaction/src/recognizers/tap.rs:725 vs :768 | Типы | M | Две карты кнопок: неизвестная кнопка отвергается в `add_pointer_down`, но в `event_button` становится Primary → Up auxiliary совпадает с primary-последовательностью. Одна `fn tap_button(&PointerEvent) -> Option<TapButton>`. |
| crates/flui-interaction/src/recognizers/double_tap.rs:448,471; long_press.rs:678 | Арифметика: геометрия-заглушка | L | `unwrap_or(Offset::ZERO)` в деталях отмены → `Option<Offset<f64>>` в деталях или не сообщать позицию. |
| crates/flui-interaction/src/recognizers/tap.rs:613…, long_press, double_tap, multi_tap | Владение/паники | L | Прямые `callback(..)` подряд: паника первого пропускает остальные. Через общий `invoke_callback`. |
| crates/flui-interaction/src/recognizers/double_tap.rs:587; long_press.rs:542; drag.rs:842; tap.rs:773 | API: одно правило в одном месте | L | Три способа извлечь pointer/позицию из Down → один хелпер в `recognizer.rs`. |
| crates/flui-interaction/src/recognizers/recognizer.rs:171 | Документация | L | Дефолтный `add_pointer_down` пропускает все кнопки, rustdoc обещает фильтр → сказать «не фильтрует» или фильтровать `is_primary_down`. |
| crates/flui-interaction/src/arena/mod.rs:1385-1388 | Владение | — (не дефект) | Сабагент-аудит предположил гонку «слот ни в `entries`, ни в `retained`». `GestureArena: !Send + !Sync` (`:1781`), между `remove_current_slot` и `insert` нет пользовательского кода — гонки нет. Это аргумент за замену `DashMap` на однопоточное хранилище (вставить в `retained` до удаления — бесплатная гигиена). |
| crates/flui-interaction/src/recognizers/tap.rs:290 | API | L | `#[derive(Clone)]` на `TapArenaMember` без пользователя — убрать. |

Хорошо: `TapArenaMember` с `Weak` (идентичность последовательности), `checked_add` + `BUG:` для sequence id, состояние фиксируется до колбэков, `mem::take` колбэков в `dispose` и drop вне `RefCell`, все `if let … = x.borrow()` переписаны в `let …; if let`.

### I2 — scale/force press/tap-and-drag/eager (незакоммичено)

| Файл:строка | Пункт | Сев. | Исправление |
|---|---|---|---|
| crates/flui-interaction/src/recognizers/force_press.rs:388-395; tap_and_drag.rs:497-502 | Паники: доставка долга | H | Паника соперника при выходе из арены пропускает уведомления, а состояние уже сброшено → `on_start` без `on_end`. Доставлять уведомления после шага арены всегда, первая паника — `preserve_first` (как `scale.rs:637-660`). |
| crates/flui-interaction/src/recognizers/tap_and_drag.rs:292-310, :497 | Паники | H | Паника `on_tap_down` теряет DragStart, но DragUpdate/DragEnd идут. Либо DragStart доставляется после сбоя, либо последовательность прерывается. |
| crates/flui-interaction/src/recognizers/scale.rs:991-1008 vs doc :183-185 | Поведение ≠ doc | H | Отвергнутый контакт при ≥3 не отменяет; при 2 остаётся `won == true`. Reject → тот же teardown, что `handle_cancel`. |
| crates/flui-interaction/src/recognizers/tap_and_drag.rs:697-711; force_press.rs:525-528 | Реентерабельность | M | После колбэков (`on_cancel`/`on_end`), которые могут вызвать dispose, — снова `assert_not_disposed` перед `start_tracking`/входом в арену. |
| crates/flui-interaction/src/recognizers/tap_and_drag.rs:560,583,601-605 | Арифметика: NaN | M | `global_position` не проверяется на конечность → фильтровать отдельно (как `force_press.rs:454`). |
| crates/flui-interaction/src/recognizers/force_press.rs:461-483 | Поведение | M | Падение давления в `Claiming` игнорируется (`_ => {}`) → назад в `Possible` или выход. |
| crates/flui-interaction/src/recognizers/scale.rs:40-83 (импорт в force_press.rs:24, tap_and_drag.rs:48) | Архитектура модулей | M | Общее сдерживание колбэков — в `recognizers/callback_containment.rs`. |
| crates/flui-interaction/src/recognizers/tap_and_drag.rs:74,87,101,117,130; scale.rs:102,116,157 | API: `#[non_exhaustive]` | M | Новое pub-поле в pub-структурах деталей без `#[non_exhaustive]` → добавить на все. |
| crates/flui-interaction/tests/multi_pointer_recognizers.rs | Тесты: NaN-строки | M | Нет строк NaN scale-move, NaN pressure, non-finite tap-drag up, NaN global position — добавить в таблицы. |
| crates/flui-interaction/src/recognizers/force_press.rs:112,249; tap_and_drag.rs:349 | Владение | L | Новые `Arc<Mutex<Thresholds>>`/`Arc<Mutex<TapDragState>>` в `!Send` типах → `Cell<Thresholds>` (Copy), `RefCell<TapDragState>`. |
| crates/flui-interaction/src/recognizers/force_press.rs:197-205 | Арифметика: диапазон | L | Давление вне `[0,1]` считается датчиком и уходит в колбэк → `Pressure::try_new`/`saturating` (словарь P1) или отбросить. |
| crates/flui-interaction/src/recognizers/scale.rs:226; force_press.rs:122; tap_and_drag.rs:172 | Линты | L | `#[expect(clippy::struct_field_names)]` с причиной в комментарии → `reason = "…"`. |
| crates/flui-interaction/src/recognizers/eager.rs:84-87,109-117 | API: неподключённый pub | L | `settings()`/`set_settings()` по признанию комментария не читаются → удалить. |
| crates/flui-interaction/tests/multi_pointer_recognizers.rs:180 | Тесты | L | `run_rows` дублирует `tests/text_store_host.rs:404` в том же бинаре → общий модуль-раннер (то же в I3 `tests/velocity_and_resampling.rs:31`). |

Хорошо: исход (`Outcome`/`Notice`/`ArenaStep`) строится под guard, guard отпускается, потом колбэки; `DEGENERATE_SPAN`; `wrap_angle` в (−π, π]; упорядоченный `Vec<Contact>` вместо `HashMap`; 0.5 без датчика ≠ сила.

### I3 — velocity/resampling 

| Файл:строка | Пункт | Сев. | Исправление |
|---|---|---|---|
| crates/flui-interaction/src/processing/velocity.rs:366,377; processing/resampler.rs:303 | API: неподключённый pub | H | `estimate_at`, `velocity_at`, `add_event_at` без производственного вызова (`binding.rs:1127` зовёт `add_event`). Подключить в этой ветке или назвать follow-up в PR. |
| crates/flui-interaction/src/processing/velocity.rs:110,113,132,425 | Типы: Duration | M | `HORIZON_MS`/`ASSUME_POINTER_STOPPED_MS` дублируют `Duration`-константы, окно сравнивает f64-мс → сравнение `Duration` (`checked_duration_since`), f64-мс только как вход LSQ. |
| crates/flui-interaction/src/processing/velocity.rs:573 | Арифметика: `==` на f64 | M | `get_velocity_estimate() == Some(STOPPED)` — идентичность по значению → `enum Estimate { Stopped, Fit(..) }`. |
| crates/flui-interaction/src/processing/prediction.rs:316 | Время | M | Предиктор всё ещё через wall-clock стоп-гейт (`Instant::now()`) → `predict(now)` + `estimate_at`. |
| crates/flui-interaction/src/processing/prediction.rs:84-92; settings.rs:190,203-226 | Ошибки: валидация | M | Невалидная конфигурация молча клампится/обнуляется → `PredictionConfig::try_new`/`GestureSettings::try_new -> Result<_, thiserror-enum>`. |
| crates/flui-interaction/src/processing/resampler.rs:519 (`clear`), :373 | Доставка | M | Doc «Down/Up/Cancel never dropped», но `clear()` выбрасывает терминальные события, а `Leave` в очереди запирает Up до `stop()` → сузить гарантию в doc и дренировать не-move события в `sample` и без трекинга. |
| crates/flui-interaction/src/processing/sampling_clock.rs:118-119 | Время | M | `Manual::tick()` читает `Instant::now()` → детерминированные часы становятся wall-clock. Владелец `Instant` — binding (`tick_manual`) или переименовать. |
| crates/flui-widgets/src/scroll/scrollable.rs:663 | Настройки | L | `GestureSettings::default()` на каждом drag end игнорирует настройки распознавателя. |
| crates/flui-interaction/src/processing/resampler.rs:432 | Арифметика: `as` | L | `as f64 … as u64` в интерполяции наносекунд → явная граница (`u64::try_from` / `checked_add`). |
| crates/flui-interaction/src/velocity.rs:366,377 | API: `#[must_use]` | L | Добавить. |
| crates/flui-interaction/src/settings.rs:201 | Линты | L | `#[expect(clippy::too_many_arguments)]` без `reason` (строка тронута). |

Хорошо: `Arc<Mutex>` → `Rc<RefCell>` в resampler; колбэк клонируется из ячейки до вызова (`raw_input.rs:306`); LSQ на нормализованных данных; `clamp_magnitude` через направление; переполнение очереди сливается в `coalesced`, терминальные события всегда ставятся в очередь; все `clamp` с допустимыми границами.

### I4 — hover/hit-test (строки по HEAD `3c23e4054`; в worktree шёл revert-прогон)

| Файл:строка | Пункт | Сев. | Исправление |
|---|---|---|---|
| crates/flui-interaction/src/routing/mouse_tracker.rs:536, :368 | Типы: порядок `HashMap` | M | Порядок устройств (и какая паника возобновится) зависит от `HashMap`; тест принимает оба исхода. `BTreeMap<DeviceId, DeviceState>` или сортировка; закрепить payload в тесте. |
| crates/flui-interaction/src/routing/mouse_tracker.rs:284,300,342 | Паники/ADR-0127 | M | `register/unregister_annotation`, `remove_device` дропают аннотации простым `drop`, а `DeviceWork::invoke` (`:905`) — через `latch.release` в `catch_unwind`. Один `retire(Vec<_>)` с сохранением первой паники. |
| crates/flui-interaction/src/routing/mouse_tracker.rs:408 | Владение: edition-2024 temporaries | L | `last.retired = self.inner.borrow_mut().release_unhovered(..)` — старое значение `retired` дропается, пока жив временный `RefMut` (временные выражения-оператора живут до `;`). Сначала `let r = …;` затем присвоить. |
| crates/flui-interaction/src/routing/hit_test.rs:438 | API: `#[must_use]` | L | `with_paint_offset -> Option<R>` (отказ поддерева) → `#[must_use]`. |
| crates/flui-interaction/src/routing/mouse_tracker.rs:583 | Типы | L | `motion: Option<PointerType>` = «движение vs ambient» → `enum Refresh { Motion(PointerType), Ambient }`. |

Хорошо: `keep_first_panic` вместо трёх копий; коммит всех устройств до колбэков, изоляция паники hit-test по устройству; `Option<CursorIcon>` вместо сентинела; `localize_delta` не публикует non-finite.

### I8 — Win32 

| Файл:строка | Пункт | Сев. | Исправление |
|---|---|---|---|
| crates/flui-platform/src/platforms/windows/events.rs:233 | Время | M | Модификаторы — на время сообщения, `time` — время обработки; `GetMessageTime` не используется (в I8 он в scope). `GetMessageTime() as u32`, дельта `wrapping_sub` от базы окна (wrap ≈ 49,7 сут), плюс монотонный якорь → `EventTime`. |
| crates/flui-platform/src/platforms/windows/util.rs:112 | API: мёртвый код | M | `is_key_pressed` без вызовов (снят `expect(dead_code)`) → удалить. |
| crates/flui-platform/src/platforms/windows/events.rs:127 | Типы: bool | L | `button_message -> (PointerButton, bool)`, `mouse_button_event(is_down: bool)` → `ButtonTransition::{Pressed, Released}` (тип уже есть в словаре P1). |
| crates/flui-platform/src/platforms/windows/events.rs:183 | Арифметика/provenance | L | `HWND(lparam.0 as *mut c_void)` только для сравнения → сравнивать целые (`lparam.0 == hwnd.0 as isize`). |

Хорошо: захват берётся только если не удержан, отпускается при пустом наборе кнопок и только своим окном; `WM_CAPTURECHANGED` → Cancel; нет Rust-флага, расходящегося с `GetCapture()`; `windows::*` не утекает; каждый `unsafe` называет инвариант owner-thread; `i32::from(key.0)` без потерь. Тесты — только Windows-native: PR должен показать локальный прогон.

### I7 — runtime (vanyastaff/flui#1467)

| Файл:строка | Пункт | Сев. | Исправление |
|---|---|---|---|
| crates/flui-runtime/src/held_input.rs:220 | API: видимость | L | `drop_open_sequences` — `pub` при единственном вызове внутри крейта (`presentations.rs:410`) → `pub(crate)`; doc-ссылка на `cancel_pointer_sequences_for`. |
| crates/flui-runtime/src/held_input.rs:~250 | Владение/аллокации | L | Пересборка `VecDeque` на каждый указатель (`mem::take` + `collect`) → `VecDeque::retain` за один проход. |
| crates/flui-runtime/src/ui_realm/tests/addressed_input_routing.rs (`host_pause_cancels_a_routed_contact_with_a_delivered_cancel`) | Тесты | L | Проходит и без фикса (покрывает старый путь) — так и написать в PR. |

Хорошо: недиспатченный Down удаляется, диспатченный — отменяется (порядок `presentations.rs:403-411`); `borrow_mut()` — временный до `;`, пользовательский код под ним не выполняется; состояние — у презентации.

### P1 — словарь указателя (незакоммичено)

| Файл:строка | Пункт | Сев. | Исправление |
|---|---|---|---|
| crates/flui-platform/src/shared/input_vocabulary.rs:144,147,266-272 | Согласованность (WIP) | H (если в PR) | Мост вызывает `PointerId::new(u64)?`/`.expect(..)` и `.primary()`, а API (`pointer/mod.rs:103`, изменён позже моста) — `new(NonZeroU64) -> Self` и `with_role(PointerRole::Primary)`. Сейчас не соберётся: `PointerId::from(upstream_id.get_inner())`, `.with_role(PointerRole::Primary)`. |
| crates/flui-platform-api/src/pointer/mod.rs:551-580 | Типы: нелегальное состояние | M | `Down(PointerButtonEvent)` можно построить с `ButtonTransition::Released` (набор без кнопки). Конструкторы `PointerEvent::down/up/button_change` или хранить `transition`. |
| crates/flui-platform/src/shared/input_vocabulary.rs:73 | Арифметика: `as` | L | `(other as u32).trailing_zeros()` — каст дискриминанта; `u32::from(other.bits())`-подобный геттер upstream, если есть, или комментарий `#[repr(u32)]`. |
| docs/plans/specs/flui-interaction/pointer-vocabulary/design.md:68 | Документация ≠ код | L | См. раздел A. |

Хорошо (образец для всего крейта): валидированные newtype-величины с `try_new -> Result<_, InputValueError>` (thiserror, `#[non_exhaustive]`), явная политика NaN/диапазон/обёртка угла (`value.rs:1-12`), `None` = «нет датчика», `EventTime` + `saturating_duration_since`, `PointerId(NonZeroU64)` с `From`/`TryFrom<u64>` (C-CONV-TRAITS), `const fn` builders `with_*` + `#[must_use]`, доктесты с `?` вместо `unwrap` (C-QUESTION-MARK).

## C. Текущий main — сводка нарушений вне волны 1 (для S1/S2)

| Файл:строка | Пункт | Исправление |
|---|---|---|
| src/ids.rs:223 | Типы: newtype | `pub type DeviceId = i32` → `DeviceId(NonZeroU64)` (словарь P1). |
| src/arena/signal_resolver.rs:151-152 | Арифметика | `next_handler_id += 1` → `checked_add(1)` + ошибка исчерпания. |
| src/binding.rs:264-268 | API: pass-through | `px_f32` — тождество с неверным doc → удалить. |
| src/arena/mod.rs:62-63,248,309,824-830 | Владение | `DashMap`/`parking_lot` в `!Send` арене → однопоточное хранилище. |
| src/recognizers/*: `settings: Arc<Mutex<GestureSettings>>` (10 мест: tap.rs:135, drag.rs:181, scale.rs:119, …) | Владение | `Cell<GestureSettings>`. |
| src/traits.rs:26 `HitTestTarget: Send + Sync`; src/sealed.rs:136 `CustomHitTestable: Send + Sync` | Владение | Снять `Send + Sync` bound (owner-local цели, send-flip). |
| src/processing/velocity.rs:341-342,510-511 | Типы: Duration | `age_ms > HORIZON.as_secs_f64()*1000.0` → `age > HORIZON`. |
| src/recognizers/recognizer.rs:249; src/routing/focus.rs:400; src/processing/sampling_clock.rs:149 | Ошибки | `debug_assert!` как единственная защита: use-after-dispose и `finish_node_replacement` (последнее уже в спеке focus-keyboard → `Result`). |
| src/lib.rs:375; src/arena/team.rs:148,473; src/processing/raw_input.rs:214; src/processing/lsq_solver.rs:138,215,283; src/settings.rs:177; src/recognizers/{tap_and_drag.rs:155,drag.rs:197} | Линты | `#[expect(..)]` без `reason = "…"` (11 мест; `#[allow(` в крейте — 0). |
