# activation: задачи

- **Статус:** утверждена 2026-11-13 (решение координатора по поручению владельца); исполнение. Сделано: AC1, AC2, AC3, AC4; AC5 без чтения `[package.metadata.flui.declares]` в `#[flui::main]`. В работе: —; следующие остаток AC5, AC11.
- **Дизайн:** [design.md](design.md); требования: [requirements.md](requirements.md)
- **База:** `main` @ `8886a05e3`

## Правила исполнения

- Задача = ветка = PR; проверки по `AGENTS.md`, «Checking a change» и «Before each push».
- Тест задачи падает при откате её производственной правки и идёт через публичный API: путь от
  `Opened` до стека — через `HeadlessHost`/headless-платформу, модель хоста — `pub(crate)`
  модулем рядом с файлом хоста на Linux CI.
- Код хоста для macOS, iOS, Android и Windows пишется по документации, без железа; тело PR называет
  платформы, на которых он ещё не запускался, а нужные запуски становятся строками
  `crates/flui-native/platform-parity.toml` (✅ только после запуска).
- Каждый PR правит запись «No deep links, URL schemes, file associations or single instance» в
  `docs/gaps.md` и строки parity своей платформы: убирает построенное, называет остаток.
- Спека не добавляет ADR, пока ответ на О4 «нет»; правка решения переписывает раздел ADR, а не
  дописывает его.

## Задачи

| ID | Что | Зависит | Где | Проверка |
|----|-----|---------|-----|----------|
| **AC1** | Query в `RoutePath`: `RoutePath::query()`, `with_query`, печать и разбор `?k=v&…` с процентами; равенство и `back_stack` (query только у полного пути); в `#[derive(Routable)]` поле `#[query]` (`Option<T>`, `Vec<T>` или `T: Default`, ключ — имя поля, `#[query(rename = "…")]`); ADR-0330 §1 и «More information» переписаны | — | `crates/flui-view/src/navigation/path.rs`, `crates/flui-macros/src/derive_routable/`, `crates/flui-widgets/tests/routable_derive.rs`, `routable_ui/` | `derived_routable_round_trips` с полями query; разбор и печать query (порядок ключей, повтор ключа, пустое значение, проценты); `routable_ui/fail/pattern_query.rs` остаётся отказом (query в шаблоне пути), новые `fail/` на `#[query]` у поля шаблона и на неизвестный тип |
| **AC2** | `OpenedUrl::parse` (RFC 3986, прил. B) в `flui_view::navigation`, `LinkDeclarations`, `OpenUrlRefused`; `UiRuntime::open_url(presentation, &str)` с политикой схем (§3) и `set_link_declarations`; `HeadlessHost::open_url` | AC1 | `crates/flui-view/src/navigation/`, `crates/flui-runtime/src/ui_runtime/navigation.rs`, `crates/flui-testing/src/host.rs` | таблица разбора (схема без authority, `///`, проценты, пустой путь, слишком длинная строка); через `HeadlessHost`: объявленная схема переходит, не объявленная отказана, `https` без домена отказан |
| **AC3** | Начальный адрес в `UrlOwnerSlot` (`open_at`), `Router` открывается по нему в `create_state`; сброс не забранного за первый кадр; ADR-0330 §5 переписан | AC2 | `crates/flui-view/src/navigation/url_owner.rs`, `crates/flui-widgets/src/router/router.rs` | холодный `open_url` до `attach`: первый кадр строит `back_stack(path)`, страница `initial` не строилась ни разу; не разобранный путь → `on_unknown` и `initial`; позднее смонтированный `Router` не получает старый адрес |
| **AC4** | `AppHandler::opened(&OwnerPlatform, Opened)`; `Opened`, `OpenedFile`, `OpenTarget`, `Launch` и границы `MAX_OPENED_ITEMS`, `MAX_OPENED_URL_LEN` в `flui-platform`; очередь `app_delivery`; `HeadlessHostEvents::open` | — | `crates/flui-platform/src/opened.rs`, `crates/flui-native/src/platform.rs`, `owner/app_delivery*`, `lifecycle/headless.rs` | `Opened` в каждом тесте `owner/app_delivery/tests.rs` (`a_report_before_ready_waits_for_it`, `a_report_raised_inside_a_method_is_heard_after_it`, `a_report_from_another_thread_waits_for_an_owner_turn`, `a_panicking_method_loses_no_report_and_keeps_the_first_panic`); 65-й элемент и 8193-й байт отброшены |
| **AC5** | `RunnerHandler::opened`: адресат (окно, активное последним, основное, держатель до первой презентации), `Application::on_open_files`, URL по одному под `FirstPanic`; `Application::declares` и чтение метаданных в `#[flui::main]` | AC2–AC4 | `crates/flui-app/src/app/runner/host.rs`, `application.rs`, `crates/flui-macros` | через headless-платформу и `flui-app`: две презентации, ссылка идёт в активную последней; файлы в колбэк; паника на первом URL не теряет второй |
| **AC6** | macOS: `application:openURLs:` (URL и `file:`), холодный до `applicationDidFinishLaunching:` | AC4 | `crates/flui-native/src/owner/apple/macos/` | модель разбора `NSURL` на Linux; запуск — строка parity |
| **AC7** | iOS: `connectionOptions.URLContexts` и `userActivities` при подключении сцены, `scene:openURLContexts:`, `scene:continueUserActivity:`; адресат — окно сцены | AC4 | `crates/flui-native/src/owner/apple/ios.rs`, `ios/scene.rs` | модель сцены → `OpenTarget::Window`; запуск на симуляторе — nightly iOS Simulator |
| **AC8** | Android: `getIntent()` в `onCreate`, `onNewIntent` + `setIntent`, `ACTION_VIEW`/`ACTION_SEND`, `content://` через SAF; `classes.dex` пересобран (`xtask android-dex`) | AC4 | `crates/flui-native/android/java/dev/flui/platform/FluiActivity.java`, `owner/android.rs` | модель intent → `Opened` на Linux; запуск на эмуляторе — nightly Android |
| **AC9** | Windows и Linux: `argv` холодного старта (URL или существующий файл, остальное в журнал); Web: `location.pathname` как холодный URL страницы | AC4 | `owner/{windows,linux,web}*`, `flui-app` вход | модель разбора `argv` (кавычки, `%1`, не URL); Web — тест под `wasm32` на Node |
| **AC10** | Один экземпляр: Windows (мьютекс, именованный канал, `AllowSetForegroundWindow`), Linux (`org.freedesktop.Application` на `zbus`, токен активации); `Application::single_instance(bool)` (включён сам при объявлениях, `false` отключает); ADR-0254 §4 переписан | AC9 | `crates/flui-native/src/owner/{windows,linux}/` | на Linux CI: второй процесс передаёт `argv` по D-Bus и выходит 0, первый получает `Launch::Running`; Windows — модель кадра канала и запуск двумя процессами (строка parity) |
| **AC11** | Ключи ADR-0307 `url-schemes`, `associated-domains`, `file-types`: `Info.plist`, entitlements, `AndroidManifest.xml`, `.desktop`, регистрация Windows; `flui doctor` проверяет `apple-app-site-association` и `assetlinks.json` | AC5 | `crates/flui-cli`, ADR-0307 §2 таблица ключей | снимки сгенерированных манифестов в тестах CLI; отказ неизвестного ключа уже даёт строгость версии 1 |
| **AC12** | Документы: parity `deep-links-url-schemes-file-associations` и `single-instance-forward-arguments` с тем, что запускалось; `docs/gaps.md`; раздел о ссылках в документации `Router` (форма `myapp:///note/3`); спека удаляется | AC2–AC11 | `crates/flui-native/platform-parity.toml`, `docs/platform-parity.md` (`FLUI_OVERWRITE=1`), `docs/gaps.md` | `parity_document_matches_its_source`, `xtask checks` |

AC1, AC4 и AC11 не зависят друг от друга и идут параллельно; AC2 ждёт AC1. AC6–AC9 идут параллельно после AC4.

## Свидетельства

- **AC1.** `route_locations_preserve_encoded_segment_identity` (строки query: `+` как пробел, порядок и повторы, голый ключ, пустые пары, разделители в значении, отказ плохого escape, query только у последнего префикса, поля `#[query]` трёх видов и `Param`), `derived_routable_round_trips` с полями query, `routable_derive_ui` (`pass/query_fields.rs`; `fail/query_and_param.rs`, `fail/query_attr_arg.rs`, `fail/query_without_default.rs`, `fail/pattern_query.rs`); строка пустых пар падает при откате пропуска пустой пары в `push_query`.
- **AC2.** `opened_urls_split_into_scheme_host_and_route_location` (`///`, authority не в пути, схема без authority и путь без `/`, пустой путь, регистр схемы и хоста, IPv6, проценты и query, fragment отброшен, нет схемы, плохая схема, путь не адрес, предел `OpenedUrl::MAX_LEN` и на байт больше, объявления по схеме и домену, поддомен и `https` без домена отказаны), строка `url_owner::an_opened_url_navigates_only_when_declared` таблицы `router_and_widgets_app` через `HeadlessHost::open_url` (без объявлений ничего не открывается, чужие схема и домен и `https` без домена отказаны со стеком без изменений, `NoMatch`, объявленная веб-ссылка открывается как `go`, закрытое окно — `NoPresentation`); строка падает при откате проверки объявлений в `UiRuntime::open_url`.
- **AC3.** Строки `url_owner::` таблицы `router_and_widgets_app` через `HeadlessHost::open_url` до `attach`: `a_url_opened_before_the_first_frame_opens_the_router_on_it` (стек `back_stack`, страница `initial` и заменённая ссылка не строились), `a_cold_url_no_route_matches_reaches_on_unknown_after_the_frame` (`on_unknown` после кадра, его переход применён), `a_cold_url_no_router_took_is_dropped_after_the_first_frame` (предупреждение, затем `NoUrlOwner`, позднее смонтированный `Router` открыт на своём стеке); все три падают при откате взятия адреса в `Router::create_state` и сброса после первого кадра.
- **AC4.** `Report::Opened` в четырёх тестах таблицы `app_delivery_contracts` (`owner/app_delivery/tests.rs`): до `ready` ждёт его и слышится после (холодная ссылка), поднятый внутри метода слышится после него, с другого потока ждёт хода владельца, паникующий метод не теряет ни ссылки, ни первой паники; `opened_reports_waiting_for_ready_are_bounded` (из 17 ожидающих `ready` слышны первые 16 по порядку, затем очередь снова принимает); `opened_items_and_urls_are_bounded_before_they_are_read` (`flui-platform`): 64 элемента сохраняются по порядку, 65-й и файлы сверх предела отброшены, URL в `MAX_OPENED_URL_LEN` байт сохраняется, на байт длиннее отброшен целиком; тесты доставки падают при откате `Report::Opened` в `deliver`.
- **AC5 (маршрутизация).** Таблица `runner_open` в `flui-app` (`tests/runner_open.rs`, через
  `HeadlessRun` и `HeadlessHostEvents::open`): `a_link_that_launched_the_app_opens_its_first_router_on_it`
  (ссылка до `ready`, страница `initial` не строилась), `a_link_goes_to_the_window_focused_last`
  (два окна, ссылка в активное последним), `an_undeclared_scheme_opens_nothing`,
  `opened_files_reach_on_open_files`, `a_panic_in_the_first_urls_navigation_does_not_lose_the_second`;
  в `flui-native` — `ready_takes_what_launched_the_application` (холодные отчёты первого адресата
  слиты по порядку, другой адресат и работающее приложение слышатся после `ready`) и
  `opened_reports_waiting_for_ready_are_bounded`. Холодный тест падает без
  `OwnerPlatform::take_launch_opened` (строится `initial`).
