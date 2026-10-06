# Модель realm и потоков: эксперимент с двухоконными Notes

- **Статус:** эксперимент, вход для ADR (решение за владельцем)
- **Дата:** 2026-10-05
- **База:** `main` @ `2c067bbc5`
- **Прототип:** [`examples/two_window_notes.rs`](../../../../examples/two_window_notes.rs)
  (`cargo run --example two_window_notes --features material`)
- **Связанные:** ADR-0027, ADR-0043, ADR-0074, ADR-0085, ADR-0086, ADR-0091, ADR-0097;
  спека `send-flip` (`docs/plans/specs/send-flip/requirements.md` на ветке `plans/release-spec`)

## Коротко

1. Сегодня два окна с общим изменяемым списком Notes **выражаются только одним способом**:
   `SeparateRealms` плюс `Rc`-модель, которая держит по `RebuildHandle` на каждый
   подписанный элемент. Это работает, потому что `open_window` принимает `!Send` корень и все
   realm живут на одном owner-потоке. Канонический путь ADR-0074 (`Signal`) отказывает
   (`ForeignGraph`), а `SharedRealm` отказывает в контенте во время выполнения
   (`UnsupportedPolicy`).
2. Фраза в доке `WindowPolicy::SeparateRealms` («a stalled/slow window can never delay a
   sibling's frame pump») **ложна**. Все realm и inline raster lane каждого окна работают на
   одном потоке, и долгий кадр одного окна задерживает кадр соседа.
3. Рекомендация: **вариант A**. Один owner-поток закрепляется как контракт. Realm остаются
   единицами изоляции: GlobalKey, планировщик, teardown, граница паники. Добавляется
   app-scope `!Send` граф, который читают с отслеживанием зависимостей и пишут через
   `EventCx` все realm этого потока. Потоки на realm можно добавить позже аддитивно, новой
   политикой.
4. Зрелые тулкиты (GPUI, AppKit/UIKit, Flutter, Slint, Xilem, Qt) держат все окна на одном
   UI-потоке. GPUI устроен как вариант A, Flutter и Xilem как вариант C. Исключение Win32:
   ОС допускает поток на окно, но тулкиты этим не пользуются.

## 1. Два окна с общим списком сегодня

### Прототип

Окно 1 (`NotesList`) показывает список. Три кнопки открывают окно 2 тремя способами, по одному
на каждую форму, которую сегодняшний API позволяет хотя бы записать.

| Путь | Политика | Компиляция | Выполнение |
|---|---|---|---|
| `Rc`-модель + `RebuildHandle` | `SeparateRealms` | да | работает (headless-тест ниже); на экране есть риск устаревшего окна 1, см. «Будит не то окно» |
| `Signal` окна 1, переданный в окно 2 | `SeparateRealms` | да | чтение: `ForeignGraph`; запись: `ForeignGraph`, попадает в `warn` на `flui::signals` |
| та же `Rc`-модель | `SharedRealm` | да | `Err(AppWindowError::UnsupportedPolicy)` сразу при вызове |

Модель Notes из `examples/two_screens/tree.rs` переиспользовать не удалось: её `Shared`
приватна, а её поля связаны с `Router` одного окна (`RouterHandle`, `ScrollController`,
`FutureBuilder`). Прототип берёт из неё форму: заголовки, выбранная заметка, черновик в
`TextEditingController`.

### Результаты компиляции и запуска

- `cargo check --example two_window_notes --features material`: чисто.
- `cargo clippy --example two_window_notes --features material -- -D warnings`: чисто.
- `cargo test --example two_window_notes --features material`: оба теста прошли.
  - `an_rc_model_carries_an_edit_between_two_realms_on_one_thread`: два headless-realm
    (`flui_testing::widgets::lay_out`) на одном потоке. Окно 1 перестраивается после
    `model.save` только через `RebuildHandle` модели: `tick` не помечает корень грязным.
    Проверка дефектом: с закомментированным `self.notify()` в `save` тест падает на
    `assert!(shows(&list, "  Edited in window 2"))`, это и есть ожидаемая причина.
  - `a_signal_minted_in_one_realm_is_refused_in_another`: сигнал, созданный в `init_state`
    одного realm, читается в `build` другого как `SignalError::ForeignGraph`.
- GUI не запускался. По условиям эксперимента только `cargo check` и headless.

Проба «что потребовал бы `open_window` с потоком на realm». Функция
`fn open_on_own_thread<V: View + Clone + Send + 'static>(_: V)` временно добавлялась в пример и
потом удалена:

```text
error[E0277]: `Rc<ModelInner>` cannot be sent between threads safely
   --> examples\two_window_notes.rs:345:24
    = help: within `ModelEditor`, the trait `Send` is not implemented for `Rc<ModelInner>`
note: required because it appears within the type `NotesModel`
note: required because it appears within the type `ModelEditor`
note: required by a bound in `open_on_own_thread`

error[E0277]: `*const ()` cannot be sent between threads safely
   --> examples\two_window_notes.rs:346:24
    = help: within `SignalEditor`, the trait `Send` is not implemented for `*const ()`
note: required because it appears within the type `PhantomData<*const ()>`
note: required because it appears within the type `flui_view::Signal<Option<usize>>`
note: required because it appears within the type `SignalEditor`
```

Второе сообщение начинается с `*const ()`, а `Signal` появляется только в цепочке `note`.
Требование R1 спеки `send-flip` («диагностика называет публичный тип») для `Signal` сейчас
выполнено лишь частично.

### Где именно ломается

**`SharedRealm`: отказ во время выполнения, не при компиляции.**
`open_window_with_content_impl` возвращает
`UnsupportedPolicy { reason: "open_window with content requires WindowPolicy::SeparateRealms; …" }`
до создания окна (`crates/flui-app/src/app/runner/secondary_window.rs:758-764`). Его же
доки: «`SharedRealm` currently refuses content at admission»
(`secondary_window.rs:177-182`). У `open_secondary_window(SharedRealm)` контента нет
вообще: «no mounted widget content or per-window renderer» (`secondary_window.rs:134-138`).

**`Signal`: граф принадлежит presentation, а не realm.**
- `Reactive` — поле `BuildOwner` (`crates/flui-view/src/owner/build_owner.rs:781`), а
  `BuildOwner` у каждой presentation свой (ADR-0043). ADR-0085 («The graph is per
  presentation, not per realm») это признаёт и оставляет слияние графов открытым: «would need
  its own scheduled step and a reason a cross-window read needs it». Двухоконные Notes и есть
  такая причина.
- Чтение: `Signal::try_with` (`crates/flui-foundation/src/read_scope.rs:599`) уходит в граф
  читающего контекста. Тот отвечает `ForeignGraph` на чужой slot
  (`crates/flui-view/src/reactive/mod.rs:326-338`). `get`/`with` паникуют
  (`read_scope.rs`, `Signal::with`). Существующий тест
  `a_foreign_or_stale_read_subscribes_nobody` (`read_scope.rs:883`) фиксирует, что отказанное
  чтение никого не подписывает.
- Запись: `EventCx` окна 2 несёт граф окна 2. Запись в slot окна 1 отказывает с тем же
  `ForeignGraph`, а `WriterSource::check_context` отвечает `ForeignPresentation`
  (`crates/flui-view/src/reactive/writer.rs:185-187`, `:219-221`).
- Следовательно, **`SharedRealm` с контентом тоже не помог бы**: два окна одного realm — это
  две presentation и два графа.

**`Rc`-модель работает за счёт трёх свойств сегодняшнего кода:**
1. `open_window<V: View + Clone + 'static>` не требует `Send`
   (`secondary_window.rs:196-203`). Корень с `Rc` внутри компилируется, а отложенная
   установка хранит его в `thread_local!` `PENDING_SECONDARY_WINDOW_COMPLETIONS`
   (`secondary_window.rs:451-467`).
2. Второй realm ставится на тот же поток: `install_realm_alongside` берёт
   `std::thread::current()` и кладёт realm в тот же `APP_RUNTIME`
   (`crates/flui-app/src/app/runner/realm_dispatch.rs:651-684`, TLS в
   `crates/flui-app/src/app/runner/host.rs:67-89`).
3. `RebuildHandle` — это `Send + Sync` ключ во входящий ящик чужого realm
   (`crates/flui-view/src/owner/rebuild_handle.rs:66-134`). Его `schedule` работает из
   любого realm и потока.

**Что пользователь платит за обход:**
- Подписки пишутся руками: свой список `RebuildHandle`, отписка в `dispose` и клон модели в
  `create_state`, потому что `init_state` не видит view.
- Зависимости не отслеживаются: любое изменение перестраивает всех подписчиков целиком. Это
  та гранулярность, от которой уводит ADR-0074 §1.2.
- `StateHandle`/`StateCell` здесь не годятся. `bind` заменяет единственный слот
  (`crates/flui-view/src/state_cell.rs:195`, `:363`), поэтому при двух окнах перестроится
  только последнее привязанное.
- Канонический `Signal` и модель живут рядом. Окно 1 держит подсветку в `Signal`, а
  выбранную заметку дублирует в модель.
- Обход опирается на то, что ADR-0027 до сих пор называет временным: realm разных окон на
  одном потоке.

**Будит не то окно (вывод из кода, на экране не наблюдался).**
- `BuildOwner` каждого realm будит цикл через общий `wake`
  (`crates/flui-app/src/app/runner/desktop.rs:196-201`). Это `FrameWakeHandle::wake_frame`:
  он ставит общий флаг `needs_redraw` и дёргает `request_redraw` у единственного окна в слоте
  `redraw_window` (`crates/flui-app/src/app/runtime.rs:468-482`).
- Слот перезаписывает каждое `install_desktop_window`, в том числе для вторичного окна
  (`desktop.rs:709`, вызов из `secondary_window.rs:810`).
- Код сам это описывает: «`capabilities.wake` only pokes whichever ONE window … (issue
  #555's still-single-window wake contract)» (`crates/flui-runtime/src/presentation.rs:626-630`).
  Обходит это только путь `mark_needs_layout`/`paint`, а не путь `RebuildHandle`/сигналов.
- Отсюда сценарий. Окно 2 открыто последним, пользователь нажимает Save в окне 2.
  `RebuildHandle` ставит элемент окна 1 в ящик realm 1, но `request_redraw` получает окно 2.
  Окно 1 покажет новый заголовок только после своего следующего кадра (наведение мыши, ввод,
  resize).
- Проверить это можно только запуском на экране. Headless-хост `lay_out` кадрирует realm
  явно, поэтому тест эту маршрутизацию не видит.

## 2. Проверка «a stalled/slow window can never delay a sibling's frame pump»

Утверждение (`crates/flui-app/src/app/runtime.rs:441-446`) **сегодня неверно** и
противоречит ADR-0091 §1: «One owner thread per process hosts every realm from H0 through
H2 … they take turns on that one thread».

Путь в коде:
1. Все realm лежат в одном `thread_local!` `APP_RUNTIME` (`host.rs:67-89`). Его доки: «the
   `!Send` realm this holds remains in owner TLS».
2. Второй realm ставится туда же с `owner_thread` текущего потока (`realm_dispatch.rs:655-663`).
   Диспетчер отвергает вызов с любого другого потока: `RealmDispatchError::WrongThread`
   (`realm_dispatch.rs:1016-1019`).
3. Кадр окна — это колбэк `on_request_frame`, который вызывает
   `dispatch_platform_realm(…, RealmTask::Pump(…))` синхронно на этом потоке
   (`desktop.rs:355-362`). Пока один realm в работе, события остальных встают в общую
   `owner_turn_queue` (`realm_dispatch.rs:1040-1056`).
4. Растр тоже на этом потоке. Lane «runs synchronously on the owner (UI) thread»
   (`crates/flui-app/src/app/raster_lane.rs:1-13`); на окно своя `Arc<Mutex<RasterLane>>`
   (`desktop.rs:314`). Блокирующий present (`Fifo`, перекрытое окно) держит весь поток.

Верна только логическая часть: у каждого realm свой `UpdateScheduler`, поэтому спрос на
кадр одного окна не превращается в кадр соседа. Физически долгий `build` или present окна 1
задерживает следующий кадр окна 2. Доку нужно переписать (см. ADR ниже); ADR-0091 §1 эту
правку уже требует.

## 3. Варианты A и C

### Вариант A: один owner-поток навсегда; realm — единицы изоляции; app-scope состояние

**Решение.**
- Один owner-поток на процесс становится контрактом, а не переходным состоянием.
- Realm сохраняют всё, что дают ADR-0027 и ADR-0043: свой планировщик, `GlobalKeyScope`
  (`crates/flui-runtime/src/ui_realm/construct.rs:188`), фокус, ящик команд, teardown и
  границу паники кадра.
- Новое — **app-граф**: реактивный граф, которым владеет `AppRuntime`, а не `BuildOwner`.

**Устройство.**
- Слоты app-графа — обычные `Signal<T>` (`Copy`, `!Send`) с id app-графа.
  - Создаются при старте, до первого окна, через хук рядом с фабрикой `Application`
    (`crates/flui-app/src/app/application.rs:61-64` уже принимает `FnMut(&AppHandle) -> V`
    без `Send`).
  - Либо создаются из `LifecycleContext` с явным владельцем-приложением.
- **Чтение в `build`.** `ReadScope` отдаёт два графа: граф presentation и app-граф.
  `Signal::try_with` выбирает граф по `SignalSlot::graph`. Читатель регистрируется в
  app-графе как пара (адрес presentation, `ElementId`) с её `RebuildHandle`.
- **Запись.** `Writer` из `EventCx` (ADR-0086) пишет в app-граф по id slot'а.
  - Guard ADR-0074 §5.2 работает и здесь. Поток один, поэтому в любой момент строится не
    больше одного realm; app-граф держит `building: Option<(RealmId, ElementId)>`, и
    `WrittenDuringBuild` действует для всех realm.
  - Запись планирует читателей во **всех** realm. Каждый realm будится через wake своей
    presentation; это заодно чинит баг из §1.
- **Запись из другого потока.** `SignalSender<T>` (уже `Send`,
  `crates/flui-foundation/src/read_scope.rs:665`) уходит в ящик `AppRuntime` и
  применяется на owner-ходу вне кадра любого realm (ADR-0027 §3, commit только в Idle).

**Взаимодействие с guard'ами.**
- ADR-0074: «handle from realm A used against realm B is `ForeignGraph`» остаётся для
  локальных графов. App-слоты — это новый класс, и он доступен любому realm этого потока.
- ADR-0086: менять `EventCx` не нужно, `Writer` лишь узнаёт второй граф.
- ADR-0097: app-граф — поле `AppRuntime` и достаётся через уже разрешённый trampoline
  `APP_RUNTIME`; нового `static` не появляется.

**Порядок teardown.**
- В шаге 5 ADR-0027 §7 (realm уничтожается) добавляется подшаг: перед drop realm снимает
  всех своих читателей из app-графа по `RealmId`.
- Поздняя запись в app-слот находит закрытый ящик и ничего не делает: так уже ведёт себя
  `ExternalBuildScheduler::schedule`.
- App-граф уничтожается последним, после выхода всех realm, внутри `AppRuntime::drop`.
  Деструкторы значений идут через существующую дисциплину удержания при панике (ADR-0074,
  ревизия 2026-09-28).

**Изоляция GlobalKey не меняется.**
- App-состояние хранит данные, а не элементы. Один и тот же `GlobalKey` в двух realm — по
  одной заявке в двух независимых scope, как сегодня при `SeparateRealms`.
- Перенос поддерева между окнами — по-прежнему unmount плюс mount (ADR-0027 §8).

**Граница паники.**
- Паника в `update` app-слота — это паника внутри dispatch пишущего realm. Её ловит граница
  этого realm.
- Частичное значение остаётся, читатели во всех realm планируются (ADR-0074 «Unwind
  consistency»). Первой считается паника пишущего realm.
- Другие realm видят только перестроение.

**Цена до 0.2.0.**
- ADR и правка доки `WindowPolicy`; публичные сигнатуры не меняются.
- App-граф добавляется позже аддитивно. Если владелец захочет его в 0.2.0, это новый
  конструктор плюс внутреннее расширение `ScopeRef`.
- Починка wake на presentation — внутренний баг-фикс.

### Вариант C: один realm на приложение по умолчанию; окна — presentation

**Решение.** `SharedRealm` становится значением по умолчанию; приложение — один realm, окно —
одна presentation (Flutter `ViewCollection`, Xilem `WindowView`).

**Что нужно сделать.**
- Снять отказ `SharedRealm` в контенте: raster lane и рендерер на каждую presentation
  (`secondary_window.rs:758-764`).
- Слить графы presentation в граф realm. ADR-0085 оставил это открытым; без слияния C не
  решает Notes (см. §1).
- Пересмотреть ADR-0043, риск «медленный кадр одного окна задерживает другое»: общий
  `UpdateScheduler` и async driver.

**Что теряется.**
- **GlobalKey на окно.** Тот же ключ в двух окнах падает при втором mount (ADR-0043). Два
  открытых редактора с `GlobalKey` формы конфликтуют.
- **Teardown и граница паники на окно.** Изоляция realm перестаёт совпадать с окном. Паника,
  которая роняет realm, или его hot-restart затрагивают все окна. Закрытие окна — это
  закрытие presentation, а не realm.
- **Фокус на окно.** Фокус координируется по realm, поэтому все окна делят одну
  координацию.
- **Путь к потоку на окно.** Если окна — presentation одного realm, вынести окно в отдельный
  поток нельзя без расщепления realm.
- **Default ADR-0027 §1** («desktop default is one realm per window») и опубликованный
  `#[default] SeparateRealms` придётся сменить. Это поведенческий break, который лучше делать
  до 0.2.0 или не делать вовсе.

**Что выигрывается.** Без app-графа достаточно слить графы realm, и кросс-оконное чтение
«просто работает».

### Пользовательский код двухоконных Notes: A и C рядом

```rust
// ───────────── A: app-граф, SeparateRealms (по умолчанию) ─────────────
#[derive(Clone, Copy)]
struct Notes { titles: Signal<Vec<String>>, selected: Signal<Option<usize>> }

fn main() {
    Application::new(|app: &AppHandle| {
        let notes = Notes {
            titles: app.signal(seed_titles()),   // слот app-графа
            selected: app.signal(None),
        };
        NotesList { notes }
    })
    .run();
}

// окно 1, build
let titles = self.notes.titles.with(cx, Clone::clone);   // читатель в realm 1
// окно 1, нажатие на строку
move |cx| {
    notes.selected.set(cx, Some(id))?;
    open_window(AppConfig::new(), WindowPolicy::SeparateRealms, NoteEditor { notes });
    Ok(())
}
// окно 2, build
let Some(id) = self.notes.selected.get(cx) else { return hint() };  // читатель в realm 2
// окно 2, Save
move |cx| notes.titles.update(cx, |t| t[id] = draft.text())       // перестроит окно 1
```

```rust
// ───────────── C: один realm, окна — presentation ─────────────
fn main() { run_app(NotesApp) }

impl ViewState<NotesApp> for NotesAppState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.notes = Notes { titles: cx.signal(seed_titles()), selected: cx.signal(None) };
        // граф realm (после слияния графов presentation)
    }
}
// окно 1, нажатие на строку
move |cx| {
    notes.selected.set(cx, Some(id))?;
    open_window(AppConfig::new(), WindowPolicy::SharedRealm, NoteEditor { notes });
    Ok(())
}
// окна 2: build и Save — те же строки, что в A
```

Код view в A и C почти совпадает. Разница — где живёт состояние (приложение или первый
realm) и что изолировано. Сегодня (`examples/two_window_notes.rs`) тот же сценарий стоит
около 90 строк собственной `Rc`-модели с подписками, без отслеживания зависимостей.

## 4. Совместимость вперёд: поток на realm поверх A

Добавить потоки на realm позже **можно без поломки опубликованного API**, если сейчас не
обещать лишнего.

1. `WindowPolicy` уже `#[non_exhaustive]` (`runtime.rs:428`). Новый вариант, например
   `WindowPolicy::OwnThread`, не ломает `match` у пользователей.
2. Сегодняшний `open_window<V: View + Clone + 'static>` (`secondary_window.rs:196-203`)
   принимает `!Send` корень. Поэтому он навсегда означает «на owner-потоке». Realm на своём
   потоке получает новую функцию с фабрикой
   `open_window_on_thread(config, impl FnOnce() -> V + Send + 'static)`. Корень строится на
   целевом потоке. Компилятор не пропускает в фабрику `!Send` захваты (`Rc`-модель, app-
   `Signal`) — это ровно ошибки E0277 из пробы в §1.
3. Обмен между realm на разных потоках идёт через уже существующие `Send`-формы.
   - Внутрь: `SignalSender<T>` и `UiCommand::SignalWrite` (ADR-0074 §5.8, ADR-0085 §1) —
     запись в app-граф owner-потока.
   - Наружу: новый `Send`-канал снимков `AppSignal::watch() -> Receiver<T>` при
     `T: Send + Clone`.
   - Для приложения — `AppHandle` (уже «Thread-safe application control»,
     `crates/flui-app/src/app/application_control.rs:121-125`).
   - Актор-модель — это обычный пользовательский `Send` код поверх этих каналов.
4. Платформенный шов: колбэки окна сегодня `Send` (ADR-0091 §1). Поток на realm требует
   внутренней пересылки событий owner → realm. Это внутренняя деталь `flui-app`, в
   публичный API она не выходит.

**Чего нельзя обещать в 0.2.0, чтобы это осталось аддитивным:**
- что любой realm может прочитать app-`Signal`. Нужна оговорка «любой realm на
  owner-потоке»; threaded-realm просто не получает app-граф в своём `ReadScope`;
- что `open_window` когда-нибудь перенесёт корень на другой поток;
- `Send`-границ на UI-типах «на будущее». Спека `send-flip` снимает их до 0.2.0, и это с A
  совместимо: всё, что `!Send`, остаётся realm-локальным при любой будущей политике.

**macOS:** ограничение платформы, а не FLUI. `NSWindow` и `NSView` — `@MainActor`, поэтому
realm на своём потоке там может владеть только build/layout, а все вызовы окна пойдут через
main. ADR-0091 §5 уже держит macOS raster inline по той же причине.

## 5. Первоисточники: все окна на одном UI-потоке

Проверено 2026-10-05 через `gh api` и официальную документацию.

| Тулкит | Вердикт | Где |
|---|---|---|
| GPUI (`zed-industries/zed`) | подтверждено; форма A | `crates/gpui/src/app.rs:83-84` (`AppCell` = `RefCell<App>`), `:147` (`Application(Rc<AppCell>)`), `:774` (`App.windows: SlotMap<WindowId, …>`), `:865-867` (конструктор проверяет main thread); `crates/gpui/src/executor.rs:27-33` (`ForegroundExecutor`, `PhantomData<Rc<()>>`); общие entity и globals в одном `App`: `App::notify` инвалидирует все окна, рисующие entity (`app.rs:2928-2947`), `observe_global` (`:2326`); диспетчеры: `crates/gpui_apple/src/dispatcher.rs:51-55` (main queue), `crates/gpui_windows/src/dispatcher.rs:118-123` (`PostMessageW`), `crates/gpui_linux/src/linux/dispatcher.rs:119-123` |
| AppKit/UIKit | подтверждено, с нюансом | Apple «Thread Safety Summary» (Threading Programming Guide, developer.apple.com/library/archive/…/ThreadSafetySummary.html): `NSView` создавать и использовать только на main thread; там же сказано, что окно можно создать на вторичном потоке, а рисовать оттуда можно через `lockFocusIfCanDraw`. `@MainActor class NSWindow` и `@MainActor class UIView` в декларациях SDK (developer.apple.com/documentation/appkit/nswindow, …/uikit/uiview); Main Thread Checker (developer.apple.com/documentation/xcode/diagnosing-memory-thread-and-crash-issues-early) |
| Flutter | подтверждено; форма C | `packages/flutter/lib/src/widgets/binding.dart:416` (один element tree), `:422-423`; `engine/src/flutter/lib/ui/platform_dispatcher.dart:194-199` (`views` включает окна верхнего уровня); `packages/flutter/lib/src/widgets/view.dart:650-669` (`ViewCollection`); `packages/flutter/lib/src/widgets/_window.dart:197` (`WindowController`, ранее `RegularWindowController`), `:1348` (`WindowingOwner`), `:2592-2617`; API помечен `@internal` и экспериментален. **Не проверено:** сливают ли десктопные embedder'ы platform и UI потоки |
| Slint | подтверждено | docs.rs/slint (раздел threading: компоненты создаются на потоке event loop, в большинстве backend это main); `run_event_loop` идёт до закрытия последнего окна; `invoke_from_event_loop`; `Weak` — `Send`, но `upgrade()` возвращает `None` вне потока-создателя |
| Xilem/Masonry (`linebender/xilem`) | подтверждено; форма C | `xilem/src/driver.rs:25-28` (одно `State`, `windows: HashMap`), `:71` (`logic(&mut state)` отдаёт `WindowView`); `masonry_winit/src/event_loop_runner.rs:206-222`, `:300`; пример `xilem/examples/multiple_windows.rs` |
| egui/eframe | нюанс | `crates/egui/src/viewport.rs:27-30`: immediate viewport — тот же поток и `Context`. `:19-23`, `:271`: deferred callback `Send + Sync`, связь «via channels, or Arc/Mutex». **Не проверено:** вызывает ли eframe deferred-колбэки на другом потоке |
| Qt Widgets | подтверждено | doc.qt.io/qt-6/threads-qobject.html: GUI-классы, в частности `QWidget`, — только на main thread |
| Win32 | контрпример на уровне ОС | learn.microsoft.com/en-us/windows/win32/procthread/creating-windows-in-threads: «Any thread can create a window». Окно принадлежит создавшему потоку с его очередью сообщений; тулкиты выше этим не пользуются |

Вывод: утверждение верно для тулкитов. Win32 позволяет поток на окно, macOS — нет. Поэтому
«поток на realm» может быть только платформенной политикой, не моделью по умолчанию.

## Сравнение

| | **A**: один поток, realm-изоляция, app-граф | **B**: потоки на realm открыты, общие модели `Send + Sync` | **C**: один realm, окна — presentation |
|---|---|---|---|
| Код пользователя для Notes | `Signal` из app-графа; чтение в `build`, запись через `cx`; почти как одно окно | `Arc<RwLock<Model>>` или актор + ручные подписки `RebuildHandle`, либо `SignalSender` + `UiCommand` на каждую запись; `open_window` с `Send`-фабрикой; в `build` нет кросс-оконного отслеживания зависимостей | как в A, но состояние в первом realm; `SharedRealm` |
| Гарантии | изоляция realm (GlobalKey, scheduler, teardown, паника) сохраняется; отслеживание зависимостей кросс-оконно; нет блокировок | реальная параллельность окон; изоляция таймингов окон (на Win32/Linux); блокировки в пользовательском коде, недетерминированный порядок | одна сессия; GlobalKey, фокус, scheduler и паника общие на все окна |
| macOS | естественно (main thread) | realm не на main не может трогать `NSWindow`; только build/layout вне main, всё окно через main | естественно |
| Цена до 0.2.0 | ADR + правка доки `WindowPolicy` + баг wake; app-граф можно позже аддитивно | отменить `!Send`-корень `open_window` (break) или добавить второй API; все кросс-оконные модели `Send + Sync`; противоречит духу `send-flip` в пользовательском коде; замер выигрыша отсутствует (ADR-0091) | контент в `SharedRealm` (raster lane на presentation), слияние графов (ADR-0085), смена default — поведенческий break |
| Риск для опубликованного API | низкий: всё новое аддитивно (`#[non_exhaustive]` политика, новый конструктор слотов) | высокий: сигнатуры и `Send`-границы закрепляются в 0.2.0 и мешают `!Send`-UI | средний: меняется default и семантика GlobalKey между окнами; путь к потоку на окно закрывается |

## Рекомендация

**Принять A.**
- Один owner-поток на процесс — контракт, а не переходное состояние.
- Realm остаются единицами изоляции; `SeparateRealms` остаётся по умолчанию.
- Кросс-оконное состояние получает app-граф: `!Send`, чтение с отслеживанием зависимостей,
  запись через `EventCx`.

**До 0.2.0 сделать минимум:**
1. Переписать доку `WindowPolicy::SeparateRealms` (`runtime.rs:441-446`): изоляция
   логическая (scheduler, GlobalKey, teardown, паника); тайминг общий, потому что поток один.
2. Починить wake: realm, присланный `RebuildHandle` или сигналом, будит свою presentation, а
   не последнее открытое окно (`runtime.rs:468-482`, `desktop.rs:196-201`, `:709`). Тест:
   два окна, запись из окна 2, у окна 1 есть запрос кадра.
3. Новый ADR (ниже), с поправками в ADR-0027 и ADR-0091.
4. В доке `open_window` сказать, что корень строится на owner-потоке.

App-граф можно выпустить в 0.2.0 или аддитивно в 0.3, сигнатуры он не трогает. Вариант B не
брать: он закрепляет `Send` там, где спека `send-flip` его снимает, и ничего не даёт на macOS.
C не брать как default; `SharedRealm` с контентом остаётся отдельной задачей для сценария
«инспектор рядом с окном».

## Набросок ADR

**ADR-NNNN: Один owner-поток; realm — единицы изоляции; состояние приложения — app-граф**

- **Supersedes:**
  - ADR-0027: в verdict — «Multiple realms may execute concurrently»; в §1 — предложение о
    числе realm на owner-поток («Win32, Linux and headless may use distinct owner
    threads»); в диаграмме — «executor 2».
  - ADR-0091 §1: абзац о spike'е «Per-realm owner threads on Win32 and Linux are an H2 spike»;
    остаток §1 и §2–§7 не меняются.
- **Amends:**
  - ADR-0074 §5.1: app-уровневое состояние живёт в app-графе, а не «released with the realm».
  - ADR-0085 §1: маршрутизация по `SignalSlot::graph` охватывает app-граф.

**Решение:**
1. Процесс держит ровно один owner-поток UI. Все realm и их presentation исполняются на нём
   по очереди. Это контракт API, а не временное состояние.
2. Realm — единица изоляции, не параллельности. Свои `UpdateScheduler`, `GlobalKeyScope`,
   фокус, ящик команд, teardown и граница паники. Изоляции по времени нет и не обещается.
3. `AppRuntime` владеет app-графом. Его `Signal<T>` (`!Send`, `Copy`) читаются в `build`
   любого realm этого потока с регистрацией читателя (presentation, элемент) и пишутся через
   `EventCx` любого realm.
4. Запись планирует читателей во всех realm и будит presentation каждого. Guard
   `WrittenDuringBuild` действует для всех realm. `SignalSender<T>` применяется на owner-ходу
   вне кадра.
5. Teardown realm снимает его читателей до drop realm. App-граф уничтожается после
   последнего realm.
6. `WindowPolicy::SeparateRealms` остаётся по умолчанию. `SharedRealm` — для одной сессии на
   нескольких поверхностях.
7. Поток на realm возможен только как будущая аддитивная политика (`#[non_exhaustive]`
   вариант и `Send`-фабрика корня). Связь с app-графом только через `Send`-каналы
   (`SignalSender`, снимки, `AppHandle`). Это не план, а граница, которую API не должен
   закрывать.
8. Проверка: двухоконный тест «запись в окне 2 перестраивает окно 1 и запрашивает его
   кадр»; trybuild на `!Send` корень в фабрике будущей политики не нужен, пока её нет.

## Что не проверено

- Поведение на экране: пример не запускался (условие эксперимента). Риск «будит не то окно»
  выведен из кода (§1).
- `SharedRealm` с двумя presentation одного realm headless: `flui-testing` не поднимает
  вторую presentation у realm. Отказ виден из кода (`secondary_window.rs:758-764`).
- Win32, macOS, Linux не запускались. Сборка — только `cargo check`/`clippy`/`test` хоста
  Windows.
- Код варианта A не писался. Наброски пользовательского кода в §3 — дизайн, не компилируемый
  код.
- Flutter: сливают ли десктопные embedder'ы platform и UI потоки. egui: на каком потоке
  eframe вызывает deferred viewport.
