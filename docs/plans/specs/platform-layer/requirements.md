# Платформенный слой — требования (уровень 1)

- **Статус:** черновик, редакция 3, на утверждение владельцу. Заменяет редакцию 2 целиком:
  горизонтов выпусков нет, серия идёт до последней задачи [tasks.md](tasks.md).
- **Ограниченное утверждение:** R9 исполняется по
  [ADR-0172](../../../adr/ADR-0172-host-owned-system-preferences.md), остальные разделы
  сохраняют статус черновика. Реализация R9 ещё не завершена.
- **Дата:** 2026-10-20
- **База:** `main` @ `deb9faea2`
- **Design:** [design.md](design.md) (решения D1–D31)
- **ADR:** [ADR-0151](../../../adr/ADR-0151-platform-layer-boundary-and-names.md),
  [ADR-0152](../../../adr/ADR-0152-capability-seam-revised.md),
  [ADR-0153](../../../adr/ADR-0153-release-train-and-capability-crates.md),
  [ADR-0154](../../../adr/ADR-0154-capability-crates.md),
  [ADR-0254](../../../adr/ADR-0254-window-contract.md),
  [ADR-0255](../../../adr/ADR-0255-mobile-and-web-hosts.md)
- **Уровень 0:** [../release/requirements.md](../release/requirements.md) (R2: публикуются все
  крейты, нужные `flui`, поэтому имена и состав крейтов этой спеки — публикуемые)
- **Смежные спеки:** [send-flip](../send-flip/), [text-ime](../text-ime/),
  [persistence](../persistence/), [teardown](../teardown/), [focus-keyboard](../focus-keyboard/),
  [naming](../naming/), [window-lifecycle](../window-lifecycle/),
  [web-canvas-input-ownership](../web-canvas-input-ownership/),
  [display-pacing](../display-pacing/); на ветках: flui-animation reduce-motion, flui-interaction
  (pointer-vocabulary, matrix X1, X2)

Спека задаёт, что должно быть верно про платформенный слой: какие крейты, что в каждом, кто за
что отвечает, как устроено ядро-хост, как в слой входят необязательные возможности ОС и как
устроены хосты Android, iOS, Web и нативный Linux. Как — [design.md](design.md); порядок —
[tasks.md](tasks.md); cross-crate решения — ADR-0151…0154, ADR-0254, ADR-0255.

## Зачем

Платформенный слой вырос по частям, и его границы больше не говорят, что где лежит:

- контракт `flui-platform-api` (Stable; фасад реэкспортирует его целиком как `flui::platform`)
  держит рядом со словарём политику text store одного потребителя, тестовые двойники,
  таблицы, нужные только хосту, и синхронный `Clipboard`, который нельзя честно реализовать на Web и Wayland;
- окно отдаёт каждый факт трижды: снимок `WindowState`, геттер и один из 14 `on_*`; 21 из 39
  методов по умолчанию ничего не делает; `Send + Sync` у окна нужен одному потребителю — потоку
  растеризации, а из-за `Send`-колбэков `flui-app` держит runtime в `thread_local!`;
- «не поддерживается» написано девятью способами; 26 из 82 публичных перечислений закрыты без
  решения;
- ядро `flui-platform` на 45 % в `shared/` состоит из кода одной ОС, правила ОС бывают
  `pub #[doc(hidden)]` ради `tests/`, пользовательский код вызывается под локом (Android,
  headless), есть шесть разных «аренд» хуков, семь реализаций `Platform` по ОС с одинаковой
  обвязкой, полноту которых держит только ревью, публичные OS-типы и заглушка `LinuxPlatform`;
- шва возможностей в коде нет: любой сервис ОС может жить только в ядре;
- Android не имеет доступности, живых insets и результатов activity;
  iOS — `UITextInput` и доступности; Web — ARIA DOM; Linux
  работает на winit, у которого нет буфера обмена на чистом Wayland и нет пера;
- планка качества не закреплена гейтами: macOS и Android опрашивают предпочтения по таймеру и будят
  простой процесс, доступность спрятана за выключенной по умолчанию фичей `a11y`, ни один гейт не
  проверяет, что каждая `extern "C"` проходит границу паники, у времени показа кадра нет источника,
  у HDR и профиля дисплея нет словаря;
- имя `flui-platform-api` — из трёх частей, вопреки правилу владельца `flui-<слово>`.

Когда крейты опубликованы, каждое из этого — breaking change или долг, растущий с каждым
выпуском. Спека закрывает их до публикации и строит недостающее.

## Пользовательские сценарии

**Автор приложения** (зависит от `flui`):

1. пишет `flui::platform::…` и видит только словарь и трейты без OS-типов и чужих крейтов; факты
   об окне читает из одного снимка, команды отдаёт через `WindowControl`, события получает одним
   обработчиком окна и одним обработчиком приложения;
2. получает системную настройку (масштаб текста, контраст, «меньше движения», локаль, тайминги
   жестов) значением из окружения виджета, до первого окна и после каждого изменения в ОС;
   переопределяет её политикой приложения («всегда/никогда/как в системе»);
3. на платформе без возможности получает значение `Unsupported` с причиной — не панику, не
   `Ok(None)`, похожий на отмену, не выдуманные данные, не молча выброшенный вызов;
4. добавляет сервис ОС одним крейтом возможности (`flui add <capability>` включает фичу фасада,
   и сервис доступен как `flui::services::<имя>`); компилирует и объявляет разрешения только для
   тех, что использует; манифесты ОС не правит руками, а usage-строки переопределяет в одной
   таблице приложения;
5. глубокий контроль над ОС берёт из `flui::native::<os>` за фичей `native`; метод расширения на
   окне чужого хоста возвращает `None` или `Unsupported`, не паникует;
6. вводит текст с IME и пользуется скринридером на Android, iOS и Web так же, как на десктопе;
7. запускает приложение на Wayland и X11 без winit;
8. тестирует всё это в headless с подменой провайдера и скриптом ответов пользователя;
9. получает приложение, которое в простое не будит процессор опросом ОС, не разматывает панику
   своего обработчика в код ОС, не отдаёт введённый пароль обучению клавиатуры и истории буфера и
   имеет скринридер на каждой ОС, а не за выключенной по умолчанию фичей.

**Автор пакета или крейта возможности:** объявляет возможность и регистрирует её у приложения
через `Plugin`, не меняя фреймворк; держит свой OS-код у себя (сначала Rust, затем встроенный dex,
затем Gradle-модуль или SPM-пакет); пишет декларации разрешений в метаданных крейта и получает
манифесты от `flui-cli`; тестирует симулирующим провайдером на общем сценарном примитиве и
одной conformance-таблицей; выпускает крейт по своему графику.

**Контрибьютор FLUI:** по имени крейта видит, где контракт, где OS-код ядра, где необязательная
возможность; по каталогу ядра — какая это фича (`window/`, `input/`, `text/`, …), по файлу в нём —
какая платформа (`windows`, `apple`, `android`, `web`, `linux`), а внутри файла отличает чистую
часть, которую проверяет Linux CI, от FFI; знает закрытый список того, что ядро делает с ОС само,
и маршрут добавления новой ОС из `docs/extending.md`: слово из словаря std, файл на фичу и одна
таблица `hosts/<os>.rs`, в которой неназванная позиция не компилируется, а отсутствие пишется
`Absent<F>`.

## Требования

Формулировка: «верно, что …». Колонка «Проверка» говорит чем: гейт, тест или запуск.

### R1. Крейты, имена и закрытый список (D1, D2)

| ID | Требование | Проверка |
|---|---|---|
| R1.1 | Платформенный код фреймворка — два крейта: контракт `flui-platform` (tier C, layer 1, `stable`) и ядро-хост `flui-native` (tier H, layer 3, `internal`). Необязательные возможности с OS-кодом — третий класс, крейты возможностей (ADR-0154). Ни `-api`, ни `-core`, ни крейта на ОС: все нативные хосты — модули `flui-native` | `cargo xtask workspace`; `docs/crates.md`; `rg` по токенам `flui-platform-api` вне архивных корней в описании PR переименования (разовая сверка, не гейт) |
| R1.2 | Имена из двух частей `flui-<слово>`, свободны на crates.io (проверено 2026-10-20: `flui-native`, `flui-platform`, `flui` — 404) | ADR-0151; запрос к API crates.io в PR переименования |
| R1.3 | От ядра-хоста в production зависит только `flui-app` | `allowed-dependents`, `cargo xtask reach` |
| R1.4 | Ядро делает с ОС только закрытый список, и в него входит сервис по правилу членства: его доставка требует объекта, которым владеет только хост (app delegate, Activity, оконная процедура, цикл событий), или его потребляют виджеты самого фреймворка без проводки приложением. Список: запуск и активация (доставка URL и файлов при холодном и тёплом старте, reopen, передача аргументов единственному экземпляру), session end, host notices (memory pressure, сон и пробуждение), окна (состояние, команды, декорации и свой заголовок, материал фона, дочерние и всплывающие окна), дисплеи и темп кадров, ввод (указатель, клавиатура, геймпад и поворотные устройства, IME и text store, drag and drop), доступность, clipboard, haptics, system chrome, системные предпочтения, меню (модель меню, строка меню, контекстные меню, меню Dock), поверхности, каталоги данных. Jump list Windows, трей, уведомления, диалоги и открытие URL наружу — крейты возможностей. Новый пункт списка — только новый ADR | ADR-0151 §3; ревью PR; `docs/extending.md` |
| R1.5 | Каждый пункт закрытого списка, не реализованный на какой-то ОС, записан в `docs/gaps.md` и ранжирован там; публичной поверхности у нереализованного пункта нет (R10.4); место пункта в контракте и ядре указано в design | ревью `docs/gaps.md` в PR, меняющем поддержку; раздел «Чего нет в этой серии» в [tasks.md](tasks.md) |

### R2. Содержимое контракта (D3, D6, D7, D9, D10, D22; design §5)

| ID | Требование | Проверка |
|---|---|---|
| R2.1 | В контракте — словарь и трейты, которыми пользуется кто-то выше хоста (включая приложения через фасад), и типы, которыми хост передаёт факт фреймворку. Элемент без такого потребителя — в ядро как `pub(crate)` или удалён; «вернётся с первым потребителем» не бывает | `rg` потребителей в PR; гейт API-замыкания |
| R2.2 | Контракт не содержит OS-кода, `unsafe`, тестовых двойников (`InMemoryTextStore`, `FakeHaptics`, `InMemoryClipboard`), таблиц хоста (`OfferTable`, `OfferRecord`, `TransferCompleter`, `TransferUri`, `TransferImage`, `DeviceInputRegion`, `ProjectedInputRegions`, координатные помощники), `tracing`, `parking_lot`, синхронного `Clipboard`, `WindowEvent`, `WindowBackgroundAppearance`, `KeyText` | `#![forbid(unsafe_code)]`; `reach-forbid` (`accesskit`, `tokio`, `tracing`, `parking_lot`); `cargo xtask testing`; `rg` |
| R2.3 | Судьба каждого модуля контракта — как в design §5: остаётся, переделан, перенесён или удалён; новых модулей два (`unsupported`, `capability`), модуль `menu` появляется вместе с первой реализацией и потребителем | ревью по таблице §5 в PR каждого модуля |
| R2.4 | Машинерия text store разделена по потребителю: трейты, значения, `CommitGate`, `OwnerCalls`, `commit_composition_in_place`, `project_ime_event` (их вызывают хосты) — в контракте; `LockArbiter`, `CompositionLedger`, `EditGeneration`, `byte_range`, `committed_text` (их вызывает только `flui-widgets`) — в `flui-interaction::text_input` и реэкспортируются через `flui-sdk` | `cargo xtask workspace`; тесты арбитра и журнала в `flui-interaction`; строка в `tests/surface.rs` |
| R2.5 | Закрытость — решение, а не умолчание: каждое публичное перечисление контракта `#[non_exhaustive]` либо несёт `#[allow(clippy::exhaustive_enums)]` с причиной; `Platform` и `HostWindow` запечатаны явно; `PlatformWindow` открыт для тестовых двойников; грани сверх обязательного ядра — отдельные трейты через `Option` | `clippy::exhaustive_enums` на уровне deny в контракте; trybuild: внешний `impl Platform` и `impl HostWindow` не компилируются |
| R2.6 | Ни одна публичная сигнатура контракта не называет upstream-тип, кроме ADR-0089 §2 (`raw-window-handle` 0.6 traits, `serde`, `schemars`, `cursor-icon`). `ui-events`, `keyboard-types`, `dpi` перечислены в гейте поимённо с указанием спеки pointer-vocabulary, которая их убирает; список исключений только сокращается | гейт API-замыкания (ADR-0089 §6) с `--self-test`; `reach-forbid` растёт по мере ухода |
| R2.7 | Версия контракта едет с поездом, пока `cargo-semver-checks` не покажет его стабильным два поезда подряд; затем — своя (ADR-0153 вариант 1). Отчёт `cargo-semver-checks` по контракту есть в CI | отчёт в CI; ревизия ADR-0153 при переходе |
| R2.8 | `AppLifecycleState` остаётся во `flui-scheduler` (агрегат runtime, ADR-0035 §4); хосты публикуют факты presentation (`WindowExecutionState`, видимость) и от `flui-scheduler` не зависят | `cargo xtask workspace` |
| R2.9 | У каждого элемента один путь: ядро не реэкспортирует имена контракта, потребители импортируют их из `flui_platform`; фасад реэкспортирует контракт целиком как `flui::platform`. Тип, общий для производителя и потребителя, — в контракте; потребитель реэкспортирует | компиляция; `rg` по префиксу `flui_native::` с именами контракта (`WindowOptions`, `HostNotice`, `WindowCommand`, …) пуст; `cargo xtask workspace` |
| R2.10 | Дерево модулей контракта сгруппировано по областям (`window/`, `input/`, `text/`, `display/`, `preferences/`, `transfer/`, `haptics/`, `capability/`, `menu/`, `unsupported`, `notice`, `system_chrome`, `storage`, `target_platform`); корневые реэкспорты имён, которыми пользуются фасад и `flui-sdk`, остаются, так что `flui::platform::WindowOptions` не меняется | `cargo public-api diff` по корневым путям; `ls crates/flui-platform/src` |

### R3. Контракт окна, приложения и словарь отсутствия (D4, D5, D11, D12)

| ID | Требование | Проверка |
|---|---|---|
| R3.1 | Состояние окна — один снимок `WindowState` с `WindowCapabilities` внутри. Нет геттеров фактов окна, парных им `on_*`, методов-близнецов команд (`show`, `activate`, `close`), `WindowEvent` и `Platform::on_window_event` | conformance-таблица окна (снимок согласован с readback ОС); `rg` по удалённым именам; потребители читают снимок |
| R3.2 | Команда доходит до хоста только как `AdmittedCommand`, который выдаёт `admit`; ошибки разделены по тому, кто их вызывает: `AdmissionError { Unsupported, InvalidGeometry }` для вызывающего, `ApplyError { Closed, Superseded, Platform }` для хоста, где `Superseded` — возможности сменили ревизию между допуском и применением на следующем owner-ходе, и команду надо допустить заново (гонка не прячется в `Platform` и отличима от сбоя ОС); `capabilities()` честен | trybuild: `AdmittedCommand` нельзя построить снаружи; conformance-строка «`Unsupported` ровно тогда, когда возможность ложна»; тест `Superseded`: смена возможностей между допуском и применением |
| R3.3 | Окно открывается с одним `WindowHandler`, переданным при открытии; обязательства — ввод (с результатом доставки), изменение состояния, запрос кадра, запрос закрытия (с решением), закрыто, смена поверхности. Окна без обработчика нет; метода контракта, молча выбрасывающего переданное, нет | тест: ни одного `let _ = callback` в контракте; conformance: первое событие после `open_window` доходит; очередь раннего ввода и снимок «ещё не зарегистрировано» исчезли |
| R3.4 | `run` принимает один `AppHandler` — трейт ядра, не контракта, его реализует только `flui-app` — вместо отдельных регистраций (`on_quit`, `on_reopen`, `on_session_end`, `on_host_notice`, `set_exit_policy_hook`, `set_wake_deadline_hook`) и одноразового `PlatformReadyCallback`: обязательства `ready`, `quit_requested`, `reopen`, `session_end`, `host_notice`, `exit_policy`, `wake_deadline`, без `on_preferences_changed` (доставка предпочтений — ADR-0172); принятое событие не теряется при замене или отсутствии обработчика. Обязательство `opened(urls, files)` добавляется к трейту вместе с фичей активации, а не инертным умолчанием | headless-тест доставки; `rg` по удалённым регистрациям; `app_handler_obligation_table` |
| R3.5 | `PlatformWindow` — `!Send + !Sync`; потоку растеризации отдаётся отдельный `SurfaceTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static`, отвечающий `HandleError::Unavailable` после закрытия окна; `unsafe impl Send` хоста — только у цели поверхности | `static_assertions::assert_not_impl_any!`, trybuild; тест `SurfaceTarget` после закрытия; Win32: `state_changed` синхронно внутри OS-колбэка живого resize |
| R3.6 | Покадровые глаголы (`set_frame_rate_hint`, `set_input_regions`) и команда материала фона возвращают `Result<(), Unsupported>` и имеют `Availability` в возможностях окна; материал фона — команда с возможностью, имён Windows (`Mica…`) в контракте нет | conformance; тест `NotOnThisDevice` на таблице версий (`Since<F>`) без реальной старой Windows |
| R3.7 | Один словарь отсутствия: `Unsupported { reason }` с причинами `NotOnThisPlatform`, `NotOnThisDevice` (ОС или железо этого устройства не умеет; номер версии не называется), `NotDeclared { what }`, `Unavailable` (временно, повторить), `NotRegistered`, `NoWindow`; встроен во все ошибки вместо девяти unit-вариантов; `Availability` есть и в `WindowCapabilities`; номер версии ОС в контракт не попадает и ветвиться по нему приложение не может (ADR-0244) | `rg` по прежним unit-вариантам; `clippy.toml`: запрет литералов версии; таблица «ошибка несёт причину» |
| R3.8 | `Platform` содержит только то, у чего есть потребитель. Удалены `background_executor`, `BackgroundExecutor`, `Task`, `active_window`, `write_to_clipboard`, `read_from_clipboard`, `on_window_event`, `compositor_name`; `open_url`, `reveal_path`, `open_path`, `prompt_for_paths`, `prompt_for_new_path`, `app_path` живут в `flui-system`; `keyboard_layout` и его событие, которых выше хоста никто не читает, — `pub(crate)` ядра (их используют раскладка клавиш и TSF), публичными их сделает фича меню, когда появится потребитель (подписи сочетаний) | ревью по списку; компиляция |
| R3.9 | Escape hatch опубликован нарочно: `as_any` — на `HostWindow`, не на `PlatformWindow`; расширения ОС публикует фасад как `flui::native::<os>` за `cfg` и фичей `native` (Evolving); их методы принимают и возвращают только типы FLUI (прогресс на панели задач, материал, вкладки macOS), отвечают `Option` или `Unsupported` и не паникуют на окне чужого хоста; сырые handle окна и дисплея отдаются только через трейты `raw-window-handle` (ADR-0089 §2); опции создания одной ОС — типизированное значение этой ОС в `WindowOptions`, не поля в общей структуре | тест: расширение Windows на окне headless даёт `None`; rustdoc-JSON: пути `flui::native` существуют только под `cfg` и фичей, OS-типов в сигнатурах нет |
| R3.10 | `WindowOptions::input_latency` задаётся из конфигурации окна приложения: у опции есть потребитель выше хоста, и хосты, которые её читают (Web, Android), получают её значение | тест: окно с `InputLatency::Low` из конфигурации приложения доходит до хоста; `rg` по `input_latency` находит запись выше хоста |
| R3.11 | `WindowControl::submit` возвращает `Result<(), SubmitError>` (`Admission(AdmissionError)` — отказ при подаче, `Closed` — окна нет); `WindowCommand::Close` заменяет метод `close`; `pre_present_notify` — обязательный метод `SurfaceTarget`, который поток растеризации зовёт перед каждым present | тест `submit_reports_a_refusal_and_a_closed_window`; conformance-строка `pre_present_notify` на headless |

### R4. Устройство ядра-хоста (D13, D30)

| ID | Требование | Проверка |
|---|---|---|
| R4.1 | Дерево `flui-native/src` устроено по фичам: `lib.rs` (`current_platform()` отдаёт `Box<dyn Platform>` из `HostPlatform<H>`), `host.rs` (`trait Host`, `Absent<F>`), `platform.rs` (`Platform`, `AppHandler`, `HostPlatform<H>`), `host_window.rs`; `hosts/` — таблица на хост: `windows`, `apple`, `android`, `web`, `linux`, `winit`, `headless`; `owner/` — owner-поток и цикл каждой платформы, единая точка вызова пользовательского кода, `journal/`; по каталогу на каждую фичу закрытого списка — `window/`, `input/`, `text/`, `display/`, `clipboard/`, `drag/`, `accessibility/`, `haptics/`, `preferences/`, `lifecycle/` — с моделью фичи и одним файлом на платформу со словарём std (`windows`, `apple`, `android`, `web`, `linux`, `winit`, `headless`; `apple/` и `linux/` раскрываются в `macos`/`ios` и `wayland`/`x11` только там, где код расходится); `version/`; `storage/`. `headless` собирается всегда и выбирается во время работы; `winit` живёт до нативного Linux (R7). Каталогов `shared/`, `traits/` и `platforms/` нет | `ls`; `cargo xtask module-dag`; ревью |
| R4.2 | Чистая часть файла платформы (перевод событий, геометрия, режим из флагов) — модуль под `cfg(any(<os>, test))` с `forbid(unsafe_code)`; FFI и `unsafe` — модули под `cfg(<os>)`. Половины — модули внутри файла платформы фичи, а не каталоги, названные по механизму; чистая часть не называет FFI своего файла, крейты привязок и другие платформы | сборка и тесты на Linux CI; доказательство посаженным `unsafe` в чистом модуле (PR механизма); `cross-typecheck` вычисляет `const _` утверждения в FFI |
| R4.3 | Направления зависимостей модулей: `owner/` и `version/` не зависят ни от одной фичи; файл платформы в фиче зависит от модели своей фичи, `owner/`, `version/` и моделей других фич, никогда от файла платформы другой фичи, от файла другой платформы и от `hosts/` (мультиплексоры ОС — оконная процедура, широковещательный приёмник, DirectManipulation, responder'ы UIKit — диспетчируют через трейты-хуки `owner/`, которые реализуют файлы платформы фич и регистрирует таблица хоста); `hosts/` зависит от всех фич, от него не зависит никто; исключения — `macos`/`ios` → `apple/mod.rs` и `wayland`/`x11` → `linux/mod.rs` той же фичи | `cargo xtask module-dag` (расширен на `flui-native`) с `--self-test` |
| R4.4 | Размещение: код, общий для двух и более платформ одной фичи, — модель фичи (`mod.rs` и соседи) рядом с файлами платформ; общий для macOS и iOS — `apple/mod.rs` этой фичи, для Wayland и X11 — `linux/mod.rs`; отдельного каталога для кода разных фич нет: гейты версий — `version/`, лента реестра дисплеев — `display/source.rs`, сборка указателя — `input/assembly/`; механизм попадает в имя только одним уровнем ниже и только для одного объекта ОС (`text/windows/tsf/`) | `module-dag`; fan-out сверка в PR перемещений (`rg` по `owner::`, `version::` и моделям фич из каждого файла платформы) |
| R4.5 | Тесты моделей и чистых частей — модулями рядом с ними (`pub(crate)`); `tests/` хранит conformance и записи нативных трасс и не называет `flui_native::shared` и `flui_native::platforms`; в AGENTS.md «Definition of Done» это исключение записано | `rg` по `tests/`; текст AGENTS.md; тесты моделей и чистых частей идут в Linux-сборке `cargo nextest run -p flui-native` |
| R4.6 | Пользовательский код (обработчики приложения и окна, `Drop` заменённого обработчика) вызывается через одну точку в `owner/`, которая отпускает локи и заёмы до вызова; шести аренд и ручных take/restore нет; ни один хост не вызывает пользовательский код под локом (Android `invoke_quit`, headless `Created`, замена обработчика на Android, iOS, headless) | на каждом хосте тест повторного входа (обработчик вызывает платформу изнутри вызова; `Drop` обработчика вызывает платформу), падающий без исправления; `rg` по именам аренд |
| R4.7 | Общий код не размножен: режим окна из флагов, период обновления, `Location` клавиши, шаг колеса и видимость — по одному определению в модели своей фичи (`window/`, `display/`, `input/`) | `rg` по сигнатурам (одно определение); табличные тесты |
| R4.8 | Каждый файл `flui-native` укладывается в предел `file-length` (3 000 производственных строк), allowlist пуст | `cargo xtask file-length` |
| R4.9 | Нет заглушек `unimplemented!()` в хостах: `LinuxPlatform` и `LinuxWindowExt` удалены, живой слой Linux (дисплеи, logind и memory pressure, жизненный цикл, AT-SPI) сохранён файлами `linux` своих фич | `rg 'unimplemented!'` по `flui-native/src`; тесты слоя Linux |
| R4.10 | Ни один публичный путь ядра-хоста не называет OS-тип (`HWND`, `NSApplication`, `AndroidApp`, `web_sys::HtmlCanvasElement`, типы `accesskit`, `tokio`) и не открывает тестовые двойники | rustdoc-JSON с `--document-hidden-items` (расширение гейта API-замыкания); allowlist пуст |
| R4.11 | **Полнота хоста — тип.** `hosts/<os>.rs` — только `impl Host for <Os>` и конструктор. `trait Host` называет ассоциированный тип на каждую позицию закрытого списка (`Loop`, `Window`, `Displays`, `Input`, `Text`, `Accessibility`, `Clipboard`, `Drag`, `Haptics`, `Preferences`, `Lifecycle`, `Chrome`, `Storage`), поэтому хост, не назвавший тип для какой-либо фичи, не компилируется; `Absent<F>` — единственная форма отсутствия («на этой платформе нет»: `Unsupported { NotOnThisPlatform }`), инертных умолчаний у фич нет; `HostPlatform<H>` — единственная реализация `Platform`; модели фич трейтом `Host` не параметризованы, наружу идёт `Box<dyn Platform>` | `rg "impl.* Platform for"` по ядру даёт одну строку (шаблон берёт и обобщённую реализацию `impl<H: Host> Platform for HostPlatform<H>`); trybuild: таблица без одного ассоциированного типа не компилируется (E0046); `cargo xtask module-dag --self-test`; ревью `hosts/*`: ни одной функции, кроме конструктора |

### R5. Шов и крейты возможностей (D8, D17, D18, D19, D31)

| ID | Требование | Проверка |
|---|---|---|
| R5.1 | Платформенная возможность доходит до виджета через `cx.capability::<C>()` на `LifecycleContext`; на `BuildContext` — не компилируется | trybuild (E0599) |
| R5.2 | Отсутствие — `Unsupported` с причиной (`NotRegistered`, `NoWindow`, `NotOnThisPlatform`, `NotDeclared`), никогда паника | тест на каждую причину |
| R5.3 | Провайдер получает `ProviderContext`: `owner()` — `Send`-poster на owner-поток, `window()` — `Option<WindowRef>` (id, raw-handles, масштаб; не `PlatformWindow`); возможность без окна работает до первого окна | тест; компиляция |
| R5.4 | Каждый асинхронный вызов возвращает `Completion<T>`: owner-local (`!Send`), разрешается на границе хода владельца, потребляется колбэком, опросом или адаптером `Future`; колбэк ОС только постит на owner | trybuild (E0277 на `Send`); тест порядка разрешения |
| R5.5 | Handle держит подписку ОС и завершает её, когда удалён последний клон; без процесс-глобального состояния: реестр — значение per-realm владельца, провайдеры без `static` | `cargo xtask globals`; тест `Drop` handle |
| R5.6 | Типов разрешений в контракте нет, пока их не читает ни одна возможность: `PermissionState { Granted(Grant), NotDetermined, Denied(Denial) }`, `Grant`, `Denial`, `Scope` и `request_permissions` в форме ADR-0152 п. 2.8 приходят с первой возможностью, которой они нужны (уведомления или геолокация); `launch_for_result` для результатов Activity в серии есть, потребитель — выбор файла в `flui-system` | `rg` по `PermissionState` и `request_permissions` в контракте и ядре пуст; тест `launch_for_result` на Android |
| R5.7 | Реестр провайдеров — значение owner-потока, передаётся в конструктор realm; конфликт провайдеров или повтор `NAME` — `AppRunError` до первого окна, в детерминированном порядке; `CapabilityProvider<C>` dyn-compatible | тесты реестра; `const _: fn(&dyn CapabilityProvider<…>)` |
| R5.8 | `Plugin` и `CapabilityRegistrar` — в контракте, реестр и поиск конфликтов — во `flui-runtime`; пакет на `flui-sdk` + контракте и лист-крейт только на контракте объявляют, регистрируют и получают свою возможность без правки фреймворка | fixture-пакеты вне зависимостей фреймворка |
| R5.9 | Clipboard и storage доходят до виджетов только через шов: маркер и handle — у потребителя (`ClipboardCapability` во `flui-interaction`), OS-объекты — в ядре (owner-local clipboard с чтением через колбэк, `FileStore`), встроенные провайдеры собирает `flui-app`; синхронный `Clipboard`, `Platform::clipboard` и `clipboard_handle` удалены; шов сливается вместе с clipboard как первым потребителем, на `main` нет шва без потребителя | `rg` по удалённым именам; ревью PR |
| R5.10 | Крейт возможности — один на набор разрешений ОС: маркер, handle, провайдеры ОС, симулирующий провайдер, conformance-таблица, декларации; зависит от контракта (и от `flui-sdk`, если несёт виджеты), не зависит от других крейтов возможностей; ни один крейт фреймворка не зависит от него, кроме фасада `flui`, который подключает его необязательной фичей (R5.15). Маркер и handle возможности, которую называют виджеты фреймворка, живут в нижнем крейте, который их называет (clipboard — во `flui-interaction`), провайдер ОС — в листе | kind-правила `cargo xtask workspace` с `--self-test`; reach-таблица |
| R5.11 | Нативный код крейта возможности — по уровням: Rust; затем встроенный dex; затем Gradle-модуль или SPM-пакет. Никогда `javac`/`swiftc` из `build.rs`; `links` и `build.rs` — только у крейта, экспортирующего глобальный нативный символ | kind-правило; `rg` по `javac`, `swiftc`, `d8` в `build.rs`; ревью |
| R5.12 | Декларации: строгие `[package.metadata.flui.capability]` с фичами; `flui-cli` по `cargo metadata` (транзитивные крейты, включённые фичи) генерирует `AndroidManifest`, `Info.plist`, entitlements, `PrivacyInfo.xcprivacy`, `appxmanifest`; приложение переопределяет usage-строки и отключает возможность; режим verify перечисляет недостающее; `flui add <capability>` включает фичу фасада (или делает `cargo add` приложению без фасада), пишет декларации и строку регистрации; недекларированное — `Unsupported(NotDeclared)`, не отказ ОС | golden-тесты генератора на fixture-крейтах; тест verify; тест провайдера с `NotDeclared` |
| R5.13 | Сценарный примитив (упорядоченный скрипт ответов на запросы, переключатели службы, отзывы разрешений, внедрённые ошибки, виртуальное время; всё разрешается на границе хода владельца) — в фиче `scripted` контракта; каждый крейт возможности даёт симулирующий провайдер на нём и проходит одну conformance-таблицу; провайдер не имитирует успех, а возможность с деградирующей гарантией сообщает уровень на handle | тест примитива; таблица в CI против симулятора |
| R5.14 | Первый крейт — `flui-system`: диалоги, URL, файлы, reveal, trash, share, message box, автозапуск, глобальные сочетания, прогресс и значок на панели задач, jump list, пути и сведения об ОС — на каждой ОС, где она это даёт; где не даёт — `Unsupported` с причиной. Трей входит в `flui-system` вместе с фичей меню: без модели меню трей — только значок. Поддержка заявлена только по исполненному прогону | conformance-таблица `flui-system`: симулятор в CI, реальные провайдеры ОС на раннере или устройстве; `docs/platform-parity.md` |
| R5.15 | Официальные крейты возможностей — по одному на набор разрешений ОС: `flui-system` (оболочка, без разрешений), затем `flui-notify`, `flui-device` (состояние устройства и сенсоры), `flui-vault`, `flui-location` (ADR-0154), а дальнейшие (Bluetooth, камера, …) каждый своим ADR с набором разрешений и заявленными платформами; haptics, clipboard, storage, меню и активация остаются в ядре (R1.4). Единое пространство имён даёт фасад: каждый крейт стоит за фичей фасада с тем же именем, которая подключает его и реэкспортирует как `flui::services::<имя>` (`flui = { features = ["location", "bluetooth"] }` даёт `flui::services::location::*`), как `material` и `cupertino` стоят за фичами фасада; крейты остаются отдельными, поэтому несовместимое изменение в одном задевает только приложения, включившие его фичу. Фасад — единственный крейт фреймворка с ребром к крейту возможности, и ребро необязательное; `flui add <capability>` включает фичу фасада | kind-правило `cargo xtask workspace` с `--self-test` (ребро «фасад → крейт возможности» только за фичей; у каждого официального крейта возможности есть фича фасада); `facade-combos`, `feature-matrix`; `tests/surface.rs`: `flui::services::<имя>` есть ровно при включённой фиче; golden-тест `flui add` на fixture-крейте |

### R6. Хосты Android, iOS и Web (D14, D15, D16, D23; ADR-0255)

| ID | Требование | Проверка |
|---|---|---|
| R6.1 | Android: `FluiActivity extends NativeActivity` из APK (`hasCode="true"`, готовый dex, без JDK у потребителя); очередь Java → Rust пересылает записи, у которых в серии есть потребитель — `onActivityResult` (выбор файла), `onTrimMemory` (host notice), `onMultiWindowModeChanged`, `onTopResumedActivityChanged`, `onUserLeaveHint` (факты состояния окна), — буферизует до `android_main` и будит owner; dex грузит загрузчик Activity (`InMemoryDexClassLoader` и его запасной путь удалены); константа протокола Java проверяется против Rust до привязки natives | `cargo xtask android-dex`; тест рукопожатия (несовпадение — ошибка запуска, называющая оба значения, до привязки natives); Dev: Android — ручной прогон ввода на устройстве, результат activity (выбор файла), записи очереди до `android_main` |
| R6.2 | Манифест Android генерируется из базы FLUI с блоками, которые вносят крейты возможностей (`hasCode`, имя Activity, `enableOnBackInvokedCallback`, `configChanges`, `launchMode="singleTop"`, `<queries>`, тема); `targetSdk` закреплён в `platform-support.toml` и берётся оттуда упаковщиком, а не «последний установленный» | golden-тест манифеста; тест упаковщика на закреплённом `targetSdk`; `cargo xtask platform-floor` |
| R6.3 | Android: доступность через низкоуровневый `accesskit_android::Adapter`; hover приходит из нативной очереди; запрос дерева будит owner и получает ответ не позднее следующего обновления дисплея; идентификатор ADR-0240 доходит до `viewIdResourceName` | юнит-тесты моста; Dev: Android с TalkBack |
| R6.4 | Android: insets и анимация IME приходят слушателями и публикуются в снимке окна (safe area и inset клавиатуры), откуда их читает `MediaQuery` (padding и view insets); опрос `getRootWindowInsets` удалён | юнит-тесты; Dev: Android (счётчик `WindowResized` при анимации клавиатуры) |
| R6.5 | iOS: чистый Rust на objc2, без Swift и Objective-C; `UITextInput` в отдельном невидимом responder-view с `UITextInteraction`, доказанный запуском на Simulator с CJK-клавиатурой до полной реализации; доступность через `accesskit_ios::Adapter`; клавиатура — уведомлениями (граница iOS 13), её inset публикуется в снимке окна; генерируемый `Info.plist` несёт `UIApplicationSupportsIndirectInputEvents` и ориентации iPad | Dev: iOS Simulator (CJK, VoiceOver, iPad); T на остальном |
| R6.6 | Web: Rust владеет DOM — корень с canvas, хостом ввода текста и хостом семантики (в этом порядке); страница, поставляющая свой canvas, работает как раньше; `<textarea>` — контракт (шрифт не менее 16 px на iOS, `enterkeyhint` из назначения поля), `EditContext` — улучшение для Chrome; ARIA DOM через `accesskit_consumer`, включается лениво; страницу генерирует `flui-cli`; safe area и inset клавиатуры (`visualViewport`) публикуются в снимке окна | браузерные тесты (`web_browser`, wasm); Dev: Web (Chrome, Safari, Firefox) |
| R6.7 | Версии ОС: решение ADR-0244 остаётся (`platform-support.toml`, типизированные гейты `Since<F>`, версия ОС в контракт не попадает); добавляется только закреплённый `targetSdk` | `cargo xtask checks`, `platform-floor`; `clippy.toml` |
| R6.8 | Хост не ставит регистрацию, у которой в серии нет производителя или потребителя: `onNewIntent`, колбэки сцены iOS (URL, user activity, shortcut), `onRequestPermissionsResult` и `request_permissions` приходят с функцией активации и с первой возможностью, которой нужны разрешения; `OnBackInvokedCallback`, краевой свайп iOS и History API Web — с функцией «system back», чей производитель бита «можно ли вернуться» — навигатор (R10.4) | ревью PR; `rg` по именам этих регистраций в `flui-native/src` пуст |

### R7. Нативный Linux и удаление winit (D20)

| ID | Требование | Проверка |
|---|---|---|
| R7.1 | Нативные хосты Wayland и X11 построены на живом слое Linux (`display/linux/`, `lifecycle/linux.rs`, `accessibility/linux.rs`: дисплеи, logind, AT-SPI); выбор — во время работы в `current_platform()`; оба называют в `hosts/linux.rs` тип на каждую позицию таблицы и реализуют полный контракт: окна, состояние и команды, ввод (указатель, клавиатура и раскладка, касание, перо, жесты), IME, clipboard и primary selection, drag and drop, дисплеи, доступность, поверхность для wgpu | `assert_backend_conforms` на обоих; записи нативных трасс; Dev: Linux (Wayland и X11) |
| R7.2 | CI исполняет нативный Linux вместо winit; поддержка Linux ✅ в `docs/platform-parity.md` только после прогона | задача CI; ревью parity |
| R7.3 | winit удалён: файлы `winit` в фичах ядра и таблица `hosts/winit.rs`, фича `winit-backend` и её зависимости, тесты и примеры с `required-features = ["winit-backend"]`, ссылки в xtask и документах | `cargo xtask deps`, `workspace`; `rg winit` пуст вне архивных корней и ADR |

### R8. Переименование (D21; design §9)

| ID | Требование | Проверка |
|---|---|---|
| R8.1 | Переименование — два PR: ядро (`flui-platform` → `flui-native`), затем контракт (`flui-platform-api` → `flui-platform`); каждый воспроизводим командами из описания PR (`git mv` и `sd` по целому токену, длинное совпадение первым). После второго PR имя `flui-platform` снова существует и означает контракт, поэтому второй PR начинается с проверки, что в неархивных файлах не осталось токенов `flui-platform` и `flui_platform` (кроме ADR-0151), а ветка, вливающаяся после него, применяет обе карты по порядку | описание PR; повторный прогон команд на ветке даёт пустой diff; разовая сверка `rg` в начале второго PR (не гейт) |
| R8.2 | Ни одна функциональная строка не осталась со старым смыслом: фильтры `package(...)` в nextest, `--exclude` в `weekly.yml`, исключения и команды в xtask, `reach.tier.K.forbid` (запрещает `flui-native`, не запрещает новый `flui-platform`), `SDK_SURFACE`, сгенерированные файлы (`key_vocabulary`, `platform_floor`) | `cargo xtask checks`, `workspace`, `reach`, `globals`, `cross-typecheck`, `key-vocabulary`, `platform-floor`; сверка `rg` по токену в PR |
| R8.3 | trybuild `.stderr` перегенерированы (`TRYBUILD=overwrite`), не заменены текстом; строки `flui_native_recording` и `flui-native-library-*` не тронуты; архивный `docs/plans`, текст ADR-0151 и выпущенные записи `CHANGELOG.md` не тронуты; фрагменты `changelog.d` следуют карте | `cargo nextest` на `compile_fail`; ревью |
| R8.4 | Текст, описывающий отношение двух крейтов (README и ARCHITECTURE.md обоих, `docs/crates.md`, `docs/architecture.md`, `docs/testing.md`, `design/*`, `book/src/architecture.md`), переписан по смыслу, а не подстановкой; имя файла ADR-0082 не меняется | ревью; `cargo xtask docs-paths`, `docs-links` |
| R8.5 | Переименование не меняет поведение: diff PR — имена, пути и перегенерированные снимки | ревью; те же тесты зелёные |

### R9. Системные настройки (решение Q2, ADR-0172)

| ID | Требование | Проверка |
|---|---|---|
| R9.1 | `SystemPreferences` в контракте — единая принятая модель системных настроек; один производитель и один принадлежащий хосту набор native-подписок, независимый от пользовательских окон | native lifetime; headless: одна принятая смена достигает каждого живого потребителя без дублирования источников |
| R9.2 | Значение есть в realm до первого окна | тест: realm без окна читает настройки |
| R9.3 | Потребители не используют `SystemPreferences` в своей логике напрямую: interaction строит `GestureSettings`, анимации — политику движения, `MediaQuery` — то, что видит виджет | ревью; тест на каждого потребителя, падающий без доставки |
| R9.4 | Политика приложения поверх ОС («как в системе / всегда / никогда» для reduce motion) — во фреймворке (realm), не в контракте | ревью |
| R9.5 | Типы: `Motion` с `Scaled(DurationScale)`, где `DurationScale` — конечное s > 0, а масштаб ОС отображается одним `Motion::from_duration_scale`: 0 и −0.0 → `Reduce`, ровно 1 → `NoPreference`, иное конечное > 0 (субнормальные тоже) → `Scaled`, отрицательное и неконечное → `InvalidPreference::DurationScale` (на эту таблицу ссылается R8 спеки flui-animation reduce-motion); жесты с прямоугольниками double-click и drag в логических `Size` (ADR-0098), touch slop (`Distance`) и диапазоном скоростей fling (`FlingSpeeds`, min ≤ max, логические px/с); колесо — отдельная группа (`WheelStep::Lines(n)` или `Page`, символы на горизонтальный шаг); каждое поле жестов и колеса — `Option`, `None` где у ОС нет значения; приватные поля и builder; некорректное значение — `InvalidPreference`, не паника | компиляция (E0639 на литерале), тесты валидации |
| R9.6 | Смена настройки в ОС перестраивает виджет, читающий её, и меняет поведение распознавателя | тест headless, падающий без доставки |
| R9.7 | Принятый snapshot и delivery debt различаются; commit до callback/wake; отсутствие/замена/panic hook и competing failures не теряют принятый хвост | одна ошибка, две конкурирующие и следующее здоровое обновление через public host path |
| R9.8 | Начальный seed, два runtime, поздние runtime/presentation; отказ или закрытие sibling не прекращает fanout; старый revision не откатывает новый seed | headless frame/consumer proof; отдельный откат доставки |
| R9.9 | FIFO сохраняет input/frame barriers; coalescing касается только соседних совместимых принятых снимков; native invalidation не выдаётся за атомарный снимок ОС | порядок через реальные consumers; invalidation во время sampling |
| R9.10 | Native mouse-геометрия сохраняет свои единицы до проекции для конкретного presentation/DPI; ширина rectangle и допустимое смещение различаются; wheel preference применяется один раз | границы rectangle, разные DPI, disabled/page/line и pixel-based input |
| R9.11 | Изменение gesture settings имеет явную политику для активной последовательности; используется существующий GestureArenaScope, authored override сохраняется | callbacks текущей/следующей последовательности и live dependency update |
| R9.12 | Политика размеров текста сохраняет authored styles, описывает разные размеры и native mapping; layout, intrinsics, hit testing и editable caret согласованы при live update | реальные текстовые потребители; scoped override; возврат настройки; revert proof |
| R9.13 | Мёртвый словарь (`AccessibilityFeatures`) получает производителя и потребителя или удаляется | `expect(dead_code)` исчезает |

Уточнение R9.5 по ADR-0172: logical `Size` описывает результат проекции для
потребителя. Native-источник не получает право объявить пиксели одного монитора
host-global logical геометрией. Сигнатуры эскиза §6 design.md уточняются при
реализации; этот текст не утверждает представление нелинейного font sizing.

### R10. Правила перехода

| ID | Требование | Проверка |
|---|---|---|
| R10.1 | Каждое перемещение, удаление и смена формы — с ADR (новый ADR, `Superseded-by` в старом: ADR-0082 §1, §2 и §4 шаг 2, ADR-0142, ADR-0237, ADR-0063, ADR-0247 п. 1, 2, 9, ADR-0084, ADR-0039 §2–§3, ADR-0213 п. 3 и 6, ADR-0222 и ADR-0236 (форма доставки), ADR-0078 §1, ADR-0081 §2–§3, ADR-0088), миграцией всех вызовов и `changelog.d/<branch-slug>.md` для изменения, видимого потребителю | ревью, `cargo xtask checks`, `cargo xtask changelog` |
| R10.2 | Изменение поддержки платформы обновляет `docs/platform-parity.md` и `docs/gaps.md` в том же PR; ✅ ставится только после запуска на этой ОС | ревью PR |
| R10.3 | Новых `static`/`thread_local!` нет; записи `globals` переезжают вместе с модулем (ADR-0097) | `cargo xtask globals` |
| R10.4 | На `main` не попадает публичная поверхность без production-потребителя; если потребитель — следующая задача, PR называет её | ревью PR |
| R10.5 | Платформа объявляется поддержанной только по запуску; Win32, AppKit, Android, iOS, Linux и Web без запуска — «только cross-typecheck» или «собрано под wasm32»; PR пишет, что запускалось, а что компилировалось | ревью PR; `cargo xtask cross-typecheck`, `wasm-check` |
| R10.6 | Задача не трогает файлы активной ветки до её слияния или без согласия владельца | ревью |
| R10.7 | Новые публичные элементы — rustdoc с `# Errors`, `# Panics` (если паникуют), `# Examples`; каждый Rust-набросок в ADR и design собран в scratch-крейте, вывод сборки — в PR | `missing_docs`, ревью |
| R10.8 | Новое поведение имеет тест, который падает без него (изолированный checkout с откатом), написанный через публичный API из `tests/`; исключение — модели фич и чистые части ядра-хоста (R4.5) | ревью PR |
| R10.9 | В коде и документах нет маркеров процесса; ID задач и требований живут только в файлах этой спеки | `cargo xtask markers` |

### R11. Планка качества (D24, D25, D27–D29)

Качества, которые должны быть у каждой ОС одновременно. Это свойства существующих областей
закрытого списка (дисплеи, ввод, текст, lifecycle, каталоги данных), а не новые пункты списка;
у каждого есть гейт или прогон, иначе оно остаётся пожеланием.

| ID | Требование | Проверка |
|---|---|---|
| R11.1 | Доступность не опциональна: фичи `a11y` нет в ядре, `flui-app` и фасаде; адаптеры AccessKit собираются на каждой ОС всегда (на Linux через D-Bus, который нативному хосту нужен и так) | `cargo xtask feature-matrix`, `facade-combos`; `rg` по `feature = "a11y"` пуст; `cargo xtask deps` |
| R11.2 | Ноль CPU в простое: каждый сигнал ОС — уведомление, опрос по таймеру — дефект, а не запасной путь (предпочтения macOS и Android, цикл Android после Home) | счётчик пробуждений цикла в виртуальном времени на headless: простой без события — ноль пробуждений; Dev: macOS и Android — пробуждения за минуту после Home |
| R11.3 | Паника пользовательского кода никогда не разматывается в C: каждая `extern "C"` и каждый OS-колбэк проходит `panic_boundary`, и это доказывает гейт, а не ручной обход мест вызова | гейт xtask с `--self-test`: посаженная `extern "C"` без границы не проходит |
| R11.4 | Один монотонный таймлайн: ввод, кадры и показ меряются `EventTime` | тест: метки показа и ввода сравнимы без преобразования |
| R11.5 | Обратная связь о времени показа кадра питает frame clock (DXGI frame statistics, `addPresentedHandler`, `wp_presentation_time`, frame timelines); где ОС её не даёт (Web: метки `requestAnimationFrame` — начало кадра, не показ; X11) — `Unsupported` с причиной | conformance-строка на каждой ОС; headless-инъекция; Dev по ОС |
| R11.6 | HDR, широкий охват и цветовой профиль дисплея — словарь контракта (`SurfaceColorSpace`, headroom, смена профиля), который читает поверхность движка | conformance; headless-инъекция смены профиля; `Unsupported` где ОС не даёт |
| R11.7 | Доступ к файлам учитывает песочницу: слой не отдаёт путь, которым процесс не может пользоваться (порталы, security-scoped bookmarks, SAF, AppContainer); `data_dirs` и диалоги это знают | таблица `flui-system` на симулируемой песочнице; Dev: Flatpak, macOS в песочнице, Android |
| R11.8 | Защищённый ввод пароля и скрытые записи буфера: `EnableSecureEventInput` на macOS, `isSecureTextEntry` на iOS, `IME_FLAG_NO_PERSONALIZED_LEARNING` на Android; запись буфера можно пометить скрытой, чтобы её не взяли история и менеджеры буфера | таблицы соответствия на хосте; Dev по ОС |
| R11.9 | Активация следует правилам ОС о фокусе: `xdg-activation` на Wayland, передача права на передний план на Windows; окно не крадёт фокус | тесты на токене и порядке команд; Dev: Wayland, Windows |
| R11.10 | Conformance каждой ОС исполняется с настоящей ассистивной технологией (Narrator, VoiceOver, Orca, TalkBack) через `os`-lane драйвера (ADR-0238) | записанный прогон в `docs/platform-parity.md` |
| R11.11 | Записи нативного ввода воспроизводятся на headless детерминированно и покрывают IME, lifecycle, предпочтения и дисплеи, а не только указатель, клавиши и колесо | `native_recording` и `cargo xtask recording-vocabulary`; replay-тест на каждый вид записи |
| R11.12 | Conformance-таблица хоста исполняется сценариями `flui-driver`: на headless в CI и через `os`-lane на устройстве; зелёная ячейка parity — `os`-lane или ручной прогон | сценарии в крейте `flui-driver` (его спека); отчёт драйвера называет lane |
| R11.13 | Журнал нативного ввода — модуль ядра (`owner/journal/`), пишется во время работы под фичей `driver`; схема записи объявлена во `flui-protocol`, `flui-driver` читает записи без ядра | `cargo xtask workspace` (у `flui-driver` нет ребра к ядру); golden-записи читаются драйвером |
| R11.14 | Сценарный примитив контракта — под фичей `scripted`, доступен `flui-testing` и `flui-app[driver]`; двойник `PlatformWindow` один — во `flui-testing` | `cargo xtask testing` (ADR-0243); `rg "impl .*PlatformWindow for"` вне ядра и kit даёт ноль |

## Вне объёма

- Крейты возможностей после `flui-system` (`flui-notify`, `flui-device`, `flui-vault`,
  `flui-location`, …): у каждого своя спека; здесь — класс крейта, шов, декларации, сценарный
  примитив и мост к хосту, на которых они строятся (R5); типы разрешений ОС приходят с первой
  такой возможностью (уведомления или геолокация).
- Функции ядра, которых ещё нет на части ОС (меню и трей с меню, дочерние и всплывающие окна,
  свой заголовок, системная «назад» как функция с регистрациями хоста, активация с `onNewIntent`,
  колбэками сцены и `opened` в `AppHandler`, inset клавиатуры в раскладке, drag наружу, богатый
  clipboard, схемы URL и ассоциации файлов, …): у каждой своя спека, ранжированная в
  [docs/gaps.md](../../../gaps.md). Серия даёт им место в контракте и ядре (R1.4, R1.5), но не
  реализует их; список и граница — в разделе «Чего нет в этой серии» [tasks.md](tasks.md).
- Перенос `PlatformAccessibility` из `flui-semantics`: требует своего словаря дерева доступности
  (ADR-0089 §3).
- Свой словарь указателя и клавиатуры: ведёт работа interaction pointer-vocabulary; R2.6 держит
  `ui-events`, `keyboard-types` и `dpi` в поимённом списке исключений гейта, пока она не закрыта.
- Нативные виды внутри UI (web view, карта, видео): решение владельца, не пункт закрытого списка.

## Критерии готовности

Спека закрыта, когда закрыты R1–R8, R10 и R11 задачами LY20–LY101 и R9 задачами LY8/LY9 по ADR-0172;
гейты зелёные на каждом PR; каждый PR честно говорит, что запускалось (L, W, T или устройство);
заявленная ОС получает ✅ в `docs/platform-parity.md` только после прогона на ней; каждая
официальная возможность проходит свою conformance-таблицу на каждой заявленной ОС, где есть
раннер или устройство.
