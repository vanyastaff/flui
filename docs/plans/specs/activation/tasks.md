# activation: задачи

- **Статус:** утверждена 2026-11-13 (решение координатора по поручению владельца); исполнение. Сделано: —. В работе: AC1.
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
| **AC2** | `OpenedUrl::try_parse` (RFC 3986, прил. B) в `flui_view::navigation`; `UiRuntime::open_url(presentation, &str)` с политикой схем (§3); `HeadlessHost::open_url` | AC1 | `crates/flui-view/src/navigation/`, `crates/flui-runtime/src/ui_runtime/navigation.rs`, `crates/flui-testing/src/host.rs` | таблица разбора (схема без authority, `///`, проценты, пустой путь, слишком длинная строка); через `HeadlessHost`: объявленная схема переходит, не объявленная отказана, `https` без домена отказан |
| **AC3** | Начальный адрес в `UrlOwnerSlot` (`open_at`), `Router` открывается по нему в `create_state`; сброс не забранного за первый кадр; ADR-0330 §5 переписан | AC2 | `crates/flui-view/src/navigation/url_owner.rs`, `crates/flui-widgets/src/router/router.rs` | холодный `open_url` до `attach`: первый кадр строит `back_stack(path)`, страница `initial` не строилась ни разу; не разобранный путь → `on_unknown` и `initial`; позднее смонтированный `Router` не получает старый адрес |
| **AC4** | `AppHandler::opened(&OwnerPlatform, Opened)`, `Opened`, `OpenTarget`, `Launch`; очередь `app_delivery`; границы `MAX_OPENED_ITEMS`, `MAX_URL_BYTES`; `HeadlessPlatform::open` | — | `crates/flui-native/src/platform.rs`, `owner/app_delivery*`, `owner/headless.rs` | `Opened` в каждом тесте `owner/app_delivery/tests.rs` (`a_report_before_ready_waits_for_it`, `a_report_raised_inside_a_method_is_heard_after_it`, `a_report_from_another_thread_waits_for_an_owner_turn`, `a_panicking_method_loses_no_report_and_keeps_the_first_panic`); 65-й элемент и 8193-й байт отброшены |
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

Пока нет.
