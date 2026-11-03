# linux-native-hosts — дизайн

- **Статус:** утверждена владельцем 2026-11-03
- **Дата:** 2026-11-03
- **База:** `main` @ `57495bdf1`
- **Требования:** [requirements.md](requirements.md); задачи — [tasks.md](tasks.md)

## Как сейчас

- **Выбор хоста.** Выбор делает `current_platform` (`crates/flui-native/src/lib.rs`): сначала
  `FLUI_HEADLESS`, затем `choose_linux_host(environment, LinuxHosts { wayland, x11, winit })`.
  Первая строка `choose_linux_host` возвращает `LinuxHost::Winit`, если winit собран. Документация
  этой функции и строка таблицы хостов в документации `current_platform` устарели: в них сказано,
  что нативные хосты без ввода.
- **Признаки сборки.** `flui-native` по умолчанию собирается с `desktop = ["wayland", "x11"]`
  без winit. Признак `winit-backend` включает `winit`, `arboard`, `crossbeam-channel`,
  `ui-events-winit`, `keyboard-types`, а также `wayland` и `x11`. Включает `winit-backend`
  только `flui-app`, в секции `[target.'cfg(target_os = "linux")'.dependencies]`.
- **Код winit.** Условие `winit_host` (`build.rs`) задано на Windows, macOS и Linux. Код хоста:
  `hosts/winit.rs`, `owner/winit*`, `window/winit*`, `input/winit.rs`, `text/winit.rs`,
  `clipboard/winit.rs`, `display/winit.rs`, `drag/winit.rs`,
  `preferences/windows/winit_tests.rs`. Всего около 36 файлов `src`, которые упоминают
  `winit_host` или `winit-backend`, и записи в пяти проверках `xtask`.
- **AT-SPI.** `accesskit_unix` и `zbus` уже стали безусловными зависимостями Linux. Часть
  ADR-0151 §15 о том, что они едут с `winit-backend`, на `main` больше не действует. Это надо
  отметить в ADR.

## Форма решения

Два PR. Причина такого деления: переключение и удаление проверяются по-разному. Переключение
проверяется запуском приложения, удаление проверяется тем, что сборка и проверки проходят без
winit.

### PR 1: переключение

- **`flui-app/Cargo.toml`.** Целевая секция Linux больше не включает `winit-backend`. Признак
  `desktop` (`wayland`, `x11`) приходит через признаки `flui-native` по умолчанию, которые
  `flui-app` не отключает.
- **`choose_linux_host`.** Строка `if built.winit` уходит. Хост выбирается по сеансу, как в
  остальной части функции сейчас (R1, R2). `LinuxHost::Winit` остаётся, пока существует код
  winit, но `current_platform` на Linux его не выбирает. Табличный тест
  `choose_linux_host` получает строки «winit собран, сеанс Wayland → Wayland» и «winit собран,
  сеанс X11 → X11». Строки, которые ждали winit, удаляются.
- **Документация `current_platform`.** Строка Linux в таблице хостов и документация
  `choose_linux_host` переписываются по состоянию хостов. Комментарий в `flui-app/Cargo.toml`
  уходит вместе с секцией.
- **Сквозной запуск (R4).** В `crates/flui-app/tests/main.rs` появляется модуль
  `native_linux`. Тест запускает приложение через публичный API `flui-app` без
  `FLUI_HEADLESS`, с одним окном. Тест ждёт первый показанный кадр, закрывает окно и проверяет
  код выхода 0. Сеанс тест поднимает сам, как `tests/support/x11_session.rs` в `flui-native`:
  - для X11 это свой Xvfb, `DISPLAY` из него и пустой `WAYLAND_DISPLAY`;
  - для Wayland это Weston на X11-бэкенде.

  GPU берётся из lavapipe, как у наборов чтения пикселей. Без Xvfb или Weston тест пропускается,
  а при `FLUI_REQUIRE_WAYLAND` проваливается. Тест падает, если вернуть `winit-backend` в
  `flui-app`: тогда выбранный хост не равен ожидаемому нативному. Хост тест читает из
  диагностики запуска, без отдельного API.
- **`linux_preferences` (R5).** Набор теряет `required-features = ["winit-backend"]` и поднимает
  нативный хост X11 на своём Xvfb. Проверки остаются прежними: портал на частной шине, языки,
  форматы и тема.
- **`backend_conformance` (R5).** Профиль X11 берёт возможности ввода у нативного X11
  (`input_availability` для X11 вместо `winit(WinitSession::X11)`), как профиль Wayland уже
  делает. Заметку «winit reports no outgoing drag there» заменяет правда: X11 начинает XDND.
- **Документы (R7).**
  - В `docs/platform-parity.md` столбцы Wayland и X11 становятся нативными. Каждая ячейка
    пересчитывается по заметкам нативных хостов: ✅ только для строк, у которых есть запуск,
    🟡 для строк, проверенных только трансляцией. Строка сводки пересчитывается.
  - В `docs/gaps.md` запись 5 теряет пункт «`flui-app` строит winit».
  - В `ARCHITECTURE.md` `flui-native` нативные хосты становятся основными для Linux.

### PR 2: удаление winit

- Удаляются код хоста winit (список выше), признак `winit-backend`, условие `winit_host`, записи
  `xtask` и записанные трассы `native_recording/winit_*`. Удаляются зависимости рабочего
  пространства `winit`, `ui-events-winit` и `arboard`. `crossbeam-channel` и `keyboard-types`
  удаляются, если у них нет других пользователей.
- Общая модель нажатий рамки `input::regions::presses` остаётся. Из `cfg` у `mod presses` и у
  `ProjectedInputRegions::sent` убирается `winit_host`. Тест X11-рамки winit уходит вместе с
  winit.
- В ADR, по правилам `docs/adr/README.md`, тексты сохраняются, меняются только заголовки и
  пометки:
  - **ADR-0151:** у §11 статус становится «implemented». В заголовок добавляется пометка, что
    §15 больше не привязан к `winit-backend`.
  - **ADR-0082:** у §5 строка Linux помечается как заменённая ADR-0151 §11; эта связь уже
    названа в ADR-0151.
  - **ADR-0286:** §9 помечается удалённым вместе с хостом.
- Проверка `cargo xtask workspace` (`reach`) запрещает `winit` в рабочем пространстве: запрет
  кладётся в список запрещённых зависимостей, чтобы winit не вернулся тихо.

## Сверка с winit (R3)

Строки, которые на Linux сейчас даёт winit, и их состояние на нативных хостах.

| Возможность | winit сейчас | Нативный X11 | Нативный Wayland | Что нужно до переключения |
|---|---|---|---|---|
| Рамка приложения (`AppTitleBar`, ADR-0286) | есть (#2222) | #2236, открыт | готов локально, следующий PR | слить оба (ведёт ветка «Scrollable и остатки ревью ввода») |
| Drop в приложение | файлы на X11 | XDND-приёмник | `wl_data_device`, #2208 | слить #2208 |
| Drag из приложения | нет | XDND-источник | ветка `platform/wayland-dnd-source` | слить PR источника |
| Буфер обмена | `arboard`, без data-control на Wayland не работает | `CLIPBOARD` и `PRIMARY` | `wl_data_device` и primary selection | — |
| IME | `Ime` winit | IBus/Fcitx5 по D-Bus и XIM | `text-input-v3` | — |
| Мышь, клавиатура, колесо, курсор | есть | XInput 2 и XKB, запуск на Xvfb | запуск на Weston | — |
| Касание, перо, щипок | касание без запуска, пера нет | декодер без запуска | трансляция без запуска | — (не хуже winit) |
| Появление и уход устройств ввода | сообщает | только начальная таблица | только начальная таблица | сверить и при отсутствии сделать (задача T5) |
| Режимы окна, полноэкранный режим | через winit, полноэкранный режим только без рамки | проходит conformance под openbox | `SetMode(Maximized)` проваливается под sway | сверить с winit под sway; если winit тоже не может, это запись в gaps, а не регрессия |
| Настройки, тема, языки | портал | портал | портал | перевести `linux_preferences` (T4) |
| AT-SPI | общий адаптер | запуск `linux_atspi` | не запускался на окне Wayland | запуск на Weston (T6) |
| Обратная связь показа кадра | нет | нет на платформе | `wp_presentation_time` | — |
| Дисплеи, жизненный цикл, сеанс | общий код `display::linux`, logind | то же | то же | — |
| Диалоги файлов | портал, не модальный на Wayland | модальный | не модальный, пока `xdg-foreign` не дошёл до crate возможностей | — (не хуже winit) |

## Отказы и пограничные случаи

- **Сеанс Wayland без `text-input-v3`.** IME нет. Предупреждение при запуске и типизированное
  отсутствие уже есть (gap 5). winit на таком композиторе тоже без IME, так что это не регрессия.
- **Нет дисплея.** Ошибка `choose_linux_host` доходит до `flui-app` как ошибка запуска (R2).
  Тест таблицы её уже покрывает.
- **Сборка приложения с `default-features = false` у `flui-native` и без `wayland`/`x11`.**
  `current_platform` возвращает ошибку «хост не собран» с названием признака. Тихого перехода на
  headless нет.
