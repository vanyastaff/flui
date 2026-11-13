# activation: дизайн

- **Статус:** утверждена 2026-11-13 (решение координатора по поручению владельца); исполнение
- **Требования:** [requirements.md](requirements.md); задачи: [tasks.md](tasks.md)
- **База:** `main` @ `8a1c46f68`. Пути проверены на этой базе; перед правкой сверить.

## 1. Цепочка

```text
ОС ──► хост flui-native ──► AppHandler::opened(owner, Opened) ──► flui-app RunnerHandler
        (scene, intent,        (очередь app_delivery,              │ адресат: PresentationId
         openURLs, argv,        как reopen)                        │ файлы: Application::on_open_files
         D-Bus, pipe, location)                                    ▼
                                                     UiRuntime::open_url(presentation, &Url)
                                                        │ схема объявлена? URL → RoutePath
                                                        ▼
                                  владелец ещё не зарегистрирован?
                                   да ─► начальный адрес в UrlOwnerSlot ─► Router открывается по нему
                                   нет ─► navigate(Go(path)) ─► Router::go_path / on_unknown
```

Соответствие Flutter: `Opened` — то, что движок Flutter получает от платформы
(`FlutterViewController`/`FlutterActivity`); начальный адрес в слоте — `defaultRouteName`;
`navigate(Go)` — `didPushRouteInformation` и `RouterDelegate.setNewRoutePath`; разбор пути —
`RouteInformationParser`, у FLUI выведенный из маршрута (`Routable::from_path`, `back_stack`).

## 2. Контракт хоста (`flui-native`)

```rust
// crates/flui-native/src/platform.rs, рядом с reopen
pub trait AppHandler: 'static {
    // ...
    /// The platform opened URLs or files for the application.
    fn opened(&self, owner: &OwnerPlatform, opened: Opened);
}

#[non_exhaustive]
pub struct Opened {
    pub urls: Vec<String>,         // URL как его дала ОС, не длиннее MAX_URL_BYTES; разбор выше хоста
    pub files: Vec<OpenedFile>,    // путь или handle с доступом песочницы (security-scoped, SAF)
    pub target: OpenTarget,        // Window(WindowId) | Application
    pub launch: Launch,            // Cold | Running
}
```

- Метод без тела по умолчанию, как остальные (`AppHandler` требует каждый метод явно).
- Очередь: `owner/app_delivery` получает строку `Opened` рядом с `reopen`. Принятое до `ready`
  ждёт и доставляется сразу после `ready`; холодный URL поэтому всегда приходит после того, как
  приложение создало основное окно, но до первого кадра (см. §4).
- Границы (R7): `MAX_OPENED_ITEMS = 64` элементов на одно `Opened`, `MAX_URL_BYTES = 8192`;
  лишнее отбрасывается в хосте с `tracing::warn`, до разбора. Числа живут в одном месте,
  в `flui-native`, и headless применяет те же.
- Headless: `HeadlessPlatform::open(Opened)` ставит строку в ту же очередь; это вход для тестов и
  для `flui_testing`.

`OpenTarget::Window` несёт окно, которому ОС отдала ссылку (сцена iOS, `Activity` Android).
Настольные хосты ставят `Application`: окно выбирает `flui-app` (§3), потому что «активное
последним» — факт презентаций, а не хоста.

## 3. Маршрутизация (`flui-app`, `flui-runtime`)

`RunnerHandler::opened`:

1. Адресат. `Window(id)` → презентация этого окна. `Application` → презентация, чьё окно было
   активным последним (`window_focused` с отметкой порядка), иначе основная. Нет ни одной
   презентации → `opened` ждёт первую установленную (держатель на один `Opened`, не очередь).
2. Файлы → `Application::on_open_files(|ctx: &AppContext, files: &[OpenedFile]|)`; без колбэка —
   `tracing::info` со счётом.
3. URL → `UiRuntime::open_url(presentation, &str)` по одному, в порядке ОС.

`UiRuntime::open_url` (новый, рядом с `navigate`) владеет политикой, чтобы headless и все хосты
делили её (AGENTS, «Before each push», 7):

- Разбор. `flui_view::navigation::OpenedUrl::try_parse(&str)` делит строку по RFC 3986,
  приложение B (схема, authority, путь, query, fragment) и отдаёт путь в `RoutePath::parse`.
  Зависимости `url` нет: WHATWG-разбор с `idna` не нужен для сравнения схемы и домена с
  объявлениями (домен сравнивается без учёта регистра, punycode приходит от ОС готовым), а
  ворота `size` его бы посчитали.
- Схема. `http`/`https` проходят, только если домен объявлен в `associated-domains`; своя схема —
  если объявлена в `url-schemes`. Объявления — один источник, метаданные крейта: `#[flui::main]`
  читает `[package.metadata.flui.declares]` из `CARGO_MANIFEST_DIR` при компиляции и передаёт
  `Application` константу; `UiRuntime` получает её при запуске (вопрос О5). Не объявленная схема,
  и строка `argv`, которая не URL и не существующий файл, — отказ с журналом.
- URL → `RoutePath`: путь URL, как у Flutter и go_router (`uri.path`). `myapp://note/3` даёт `/3`,
  потому что `note` — host; в документации — форма `myapp:///note/3` или `myapp://app/note/3`.
  Query входит в `RoutePath` (§3a); fragment отбрасывается с `tracing::debug`, пока его нет в
  `RoutePath` (фича истории).
- Холодный старт. Если в слоте ещё нет владельца, путь кладётся в слот как начальный адрес
  (`UrlOwnerSlot::open_at(path)`; второй холодный URL заменяет первый). `Router` в `create_state`
  забирает его: `back_stack(path)` даёт стек вместо `initial`; ошибка разбора — `on_unknown` и
  `initial`. Начальный адрес, который никто не забрал за первый кадр, сбрасывается с журналом,
  чтобы позднее смонтированный `Router` не открылся по старой ссылке.
- Работающее приложение. Владелец есть → `navigate(Go(path))`. Ответ `Err` пишется в журнал с
  причиной; `NoMatch` уже доставлен в `on_unknown`.

`HeadlessHost::open_url(window, url)` вызывает `UiRuntime::open_url`, так что весь путь от URL до
стека тестируется без хоста; `HeadlessPlatform::open` проверяет очередь хоста.

## 3a. Query в `RoutePath`

Ссылки несут параметры (`https://example.com/search?q=rust&page=2`), и go_router отдаёт их как
`state.uri.queryParameters`. `RoutePath` получает query сейчас, а не с фичей истории:

- `RoutePath` хранит путь и query в одной канонической строке (`/search?q=rust&page=2`);
  `segments()` не меняется, `query()` отдаёт пары `(&str, &str)` в порядке строки, раскодированные
  из процентов; повтор ключа сохраняется. `with_query(key, value)` добавляет пару. Пустой query
  печатается без `?`.
- Разбор пути маршрутом смотрит только на сегменты. Поле варианта с `#[query]` читается из query
  по имени поля (`#[query(rename = "…")]` меняет ключ): `Option<T>` — нет ключа даёт `None`,
  `Vec<T>` — все значения ключа, `T: Default` — нет ключа даёт `Default`; значение, которое не
  разбирается `FromStr`, — `RouteParseError::Param`. `to_path` печатает поля query в порядке
  объявления, `None` и пустой `Vec` не печатаются.
- `back_stack(path)` даёт query только полному пути: нижние страницы стека открываются без него.
- Query в шаблоне `#[route("…")]` остаётся ошибкой компиляции (`pattern_query`): query
  описывается полями, а не шаблоном.

Fragment в `RoutePath` не входит: для ссылки он ничего не адресует, а для Web его добавит фича
истории.

## 4. Хосты

| Хост | Холодный старт | Работающее приложение | Адресат |
|------|----------------|-----------------------|---------|
| iOS | `scene:willConnectToSession:options:` → `connectionOptions.URLContexts`, `userActivities` (`NSUserActivityTypeBrowsingWeb`) | `scene:openURLContexts:`, `scene:continueUserActivity:` | окно сцены |
| Android | `FluiActivity.onCreate`: `getIntent()` с `ACTION_VIEW` и `data`; `ACTION_SEND`/`content://` — файлы через SAF | `onNewIntent` (`launchMode="singleTop"` уже в шаблоне); `setIntent(intent)` | окно `Activity` |
| macOS | `application:openURLs:` до `applicationDidFinishLaunching:` (AppKit доставляет его первым); файлы — те же `NSURL` с `file:` | `application:openURLs:` | `Application` |
| Windows | `argv` процесса (`CommandLineToArgvW`), ключ регистрации `"%1"` | второй экземпляр → именованный канал первого (§5) | `Application` |
| Linux | `argv` (`%u`/`%F` в `.desktop`) | второй экземпляр → D-Bus `org.freedesktop.Application.Open` (§5) | `Application` |
| Web | `location.pathname` при запуске страницы | история браузера — вне объёма | страница |

Каждый вход ОС — вызов `os_entry` (ADR-0151 §16): `xtask panic-boundary` это проверяет. Разбор
`NSURL`, `Uri`, `argv` в `OpenedUrl`/`OpenedFile` — чистая часть файла хоста, `pub(crate)` модуль с
тестами на Linux CI (AGENTS, «Definition of Done»).

## 5. Один экземпляр (Windows, Linux)

Включается сам, когда приложение объявило `url-schemes` или `file-types`; `Application::single_instance(false)`
отключает его, `Application::single_instance(true)` включает без объявлений.
Имя экземпляра — `AppIdentity` (app id в обратной DNS-форме).

- **Windows.** Первый экземпляр создаёт именованный мьютекс `Local\flui-<app-id>` и сервер
  именованного канала `\\.\pipe\flui-<app-id>-<session-id>`. Второй, найдя мьютекс, вызывает
  `AllowSetForegroundWindow(pid первого)` (pid из `GetNamedPipeServerProcessId`), пишет свой `argv`
  в канал (длина префиксом, предел `MAX_OPENED_ITEMS`/`MAX_URL_BYTES` на стороне чтения) и выходит
  с кодом 0. Первый читает канал на потоке ввода-вывода и будит владельца; сообщение становится
  `Opened { launch: Running, target: Application }`, а `WindowCommand::Activate` следует правилам
  LY95 (окно выходит вперёд, только если право передано).
- **Linux.** Первый экземпляр берёт имя `<app-id>` на сессионной шине (`zbus` уже в
  `flui-native` для AT-SPI) и выставляет `org.freedesktop.Application` (`Activate`, `Open`,
  `ActivateAction`), как GApplication. Второй, не получив имя, вызывает `Open(uris, "",
  platform_data)` с `activation-token` из `XDG_ACTIVATION_TOKEN` (Wayland) или
  `desktop-startup-id` (X11) и выходит. Первый передаёт токен в `WindowCommand::Activate`.
  `.desktop` с `DBusActivatable=true` даёт тот же путь, когда ссылку открывает рабочий стол.
- **macOS, iOS, Android.** ОС держит один экземпляр; ничего не строится. Web: каждая вкладка —
  своё приложение.

## 6. Объявления (ADR-0307)

Новые ключи версии 1, каждый с первым потребителем в этой спеке:

```toml
[package.metadata.flui.declares]
url-schemes = ["myapp"]
associated-domains = ["example.com"]
file-types = [{ extension = "note", mime = "application/x-myapp-note", name = "Note" }]
```

| Ключ | iOS / macOS | Android | Windows | Linux |
|------|-------------|---------|---------|-------|
| `url-schemes` | `CFBundleURLTypes` | `intent-filter` `VIEW`/`BROWSABLE` со `scheme` | `HKCU\Software\Classes\<scheme>` (не упакованное), `uap:Protocol` (MSIX) | `MimeType=x-scheme-handler/<scheme>` |
| `associated-domains` | entitlement `applinks:` | `intent-filter android:autoVerify="true"` с `https` и `host` | — | — |
| `file-types` | `CFBundleDocumentTypes`, `UTExportedTypeDeclarations` | `intent-filter` `VIEW` с `mimeType` | `HKCU\Software\Classes\.<ext>`, `uap:FileTypeAssociation` | `MimeType=<mime>` и shared-mime-info |

Файл `apple-app-site-association` и `assetlinks.json` на домене приложения CLI не пишет: они лежат
у владельца домена. `flui doctor` проверяет, что они отдаются (задача AC11, проверка без сети в CI).

## 7. Ошибки и паника (AGENTS, «Before each push»)

- Пользовательский код на пути: `on_open_files`, `on_unknown`, `Routable::from_path`, `Drop`
  маршрутов. Каждый зовётся без займа: очередь отдаёт `Opened` по значению, `open_url` берёт слот
  и отпускает его до вызова владельца (как `navigate`).
- Паника в одном URL не теряет следующие: `RunnerHandler::opened` обходит URL под `FirstPanic` и
  поднимает первую панику после всех (ADR-0281).
- Принятая работа: каждый `Opened` либо применён, либо отклонён с журналом; держатель «нет
  презентации» отдаёт его первой установленной или пишет отказ при завершении цикла.

## 8. Решения координатора (2026-11-13)

- **О1.** URL своей схемы даёт путь `uri.path`, как у Flutter (`myapp://note/3` → `/3`, в
  документации форма `myapp:///note/3`); приложению со своей раскладкой —
  `Application::on_open_url(|url| -> Option<RoutePath>)`.
- **О2.** Query входит в `RoutePath` сейчас (§3a, задача AC1); fragment отбрасывается до фичи
  истории.
- **О3.** Один экземпляр включается сам, когда приложение объявило схемы или типы файлов;
  `Application::single_instance(false)` его отключает.
- **О4.** Новый ADR не нужен: `opened` решён ADR-0254 §4, путь URL — ADR-0330 §2 и §5, ключи —
  ADR-0307 §2. Задачи переписывают разделы: ADR-0330 §1 (query, AC1) и §5 (начальный адрес, AC3),
  ADR-0254 §4 (один экземпляр, AC10).
- **О5.** `#[flui::main]` читает `[package.metadata.flui.declares]` при компиляции; приложение
  без него передаёт `Application::declares(flui::declares!())`.
