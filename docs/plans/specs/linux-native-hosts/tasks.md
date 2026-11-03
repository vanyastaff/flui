# linux-native-hosts — задачи

- **Статус:** утверждена владельцем 2026-11-03; исполнение. Сделаны T3, T4, T5 и T6; T7 готов и ждёт T1 и T2.
- **Design:** [design.md](design.md); требования — [requirements.md](requirements.md)
- **Порядок:**
  1. Сначала условия T1–T3, их ведут другие ветки.
  2. Затем подготовка T4–T6. Её можно делать параллельно с условиями.
  3. Затем переключение T7.
  4. Последним идёт удаление T8.

## Условия переключения

- [ ] **T1. Рамка приложения на нативном X11** (ADR-0286 §10). PR #2236; ведёт ветка
  «Scrollable и остатки ревью ввода».
- [ ] **T2. Рамка приложения на нативном Wayland** (ADR-0286 §11). Это следующий PR той же
  ветки, сразу после #2236.
- [x] **T3. Drag and drop на нативном Wayland.** Приёмник — #2208, источник — #2242, оба на `main`.

## Подготовка

- [x] **T4. `linux_preferences` на нативном X11** (R5). Набор теряет требование
  `winit-backend` и идёт на X-сервере прогона (`xvfb-run`). Проверки остаются прежними. Проверка готовности:
  набор проходит под `cargo nextest run -p flui-native --test linux_preferences` без
  `winit-backend`.
  - Сделано: набор требует `x11` и запускает `X11Platform` через новый
    `HostPlatform::run_shared` (им же теперь идёт `Platform::run`). На нативном X11 провалились
    два случая, оба исправлены в хосте:
    - `live`, `recovery`, `restart`: X11 отвечал на чтение настроек, когда цикл уже закрыл
      допуск. Теперь он отказывает, как Wayland.
    - `color-scheme`: окно, открытое при запуске, начинало со светлой схемы. Теперь оно
      получает схему портала до запуска.
  - Проверка: 17 из 17 случаев, три прогона подряд под `xvfb-run`.
- [x] **T5. Появление и уход устройств ввода** (R3).
  - Сверено, оба нативных хоста сообщают об изменениях, кода не потребовалось.
    - X11: `X11Input::device_changes` на `XI_HierarchyChanged` выдаёт `DeviceAdded` и
      `DeviceRemoved` и обновляет возможности окон с присутствующими видами. Модельный тест —
      `devices_come_and_go_with_the_hierarchy`.
    - Wayland: `refresh_capabilities` в `input/linux/wayland/dispatch.rs` пересчитывает
      возможности по присутствующим устройствам и глобалам и раздаёт их окнам.
  - Нативный X11 уже проходит conformance: строка `native x11` (`X11Decoder`). Профиль `x11`
    в `backend_conformance` — это winit на X11 (`SharedAssembly` с
    `input_availability::winit`). Он удаляется в T8 вместе с winit.
  - Заметка в `docs/platform-parity.md` («the other hosts only their initial table so far»)
    устарела для нативных хостов. Её переписывает T7.
- [x] **T6. AT-SPI на окне Wayland** (R3). Нужен запуск `linux_atspi` или его случая на окне
  нативного Wayland под Weston. Тест должен проваливаться, если адаптер не подключён к окну
  Wayland.
  - Сделано: случай `a_wayland_window_reads_back_its_tree` в `linux_atspi`. Сеанс набора
    поднимает ещё и Weston на headless-бэкенде, окно открывает `WaylandPlatform`, клиент
    AT-SPI находит окно по имени корня и читает его узел. Без Weston при
    `FLUI_REQUIRE_WAYLAND` случай проваливается.
  - Найдено попутно: с Weston в сеансе `at-spi-bus-launcher` не держит X-соединения. Тогда
    Xvfb, у которого ушёл последний клиент, сбрасывается и отказывает следующему случаю. Xvfb
    теперь запускается с `-noreset`.
  - Проверка: 6 из 6, десять прогонов подряд.

## Переключение

- [ ] **T7. `flui-app` на нативных хостах** (R1, R2, R4, R7).
  - `flui-app/Cargo.toml` без `winit-backend`.
  - `choose_linux_host` без строки winit, с новыми строками табличного теста.
  - Сквозной тест `native_linux` в `flui-app/tests` для Wayland (Weston) и X11 (Xvfb). Он
    проваливается, если вернуть `winit-backend`.
  - Документация `current_platform`.
  - `docs/platform-parity.md`: столбцы Wayland и X11 становятся нативными, сводка
    пересчитывается.
  - `docs/gaps.md`: обновляются записи 5, 8, 9 и 10.
  - `ARCHITECTURE.md`, `changelog.d`.
  - Строка «Режимы окна» из сверки: запускается winit под sway. Если winit тоже не разворачивает
    окно, это запись в gaps, а не условие.
  - Сделано (ветка `platform/linux-native-switch`): `flui-app` без `winit-backend`;
    `choose_linux_host` без winit; сквозной тест `native_linux` (Weston и Xvfb, свой сервер в
    каждом случае) проходит на нативных хостах. Тест нашёл дефект хоста Wayland: окно
    считалось перекрытым до первого кадра, `flui-app` не рисовал его и зависал. Исправление
    отдельным PR #2256.
  - Документация: `current_platform`, `docs/platform-parity.md` (столбцы нативные, сводка
    пересчитана), `docs/gaps.md` (записи 5, 6, 9, 23, 26, 29), `ARCHITECTURE.md`,
    `changelog.d`. Строка перекрытия X11 опиралась на прогон winit; добавлен случай
    `a_covered_window_reads_occluded_and_shown` для нативного X11.
  - «Режимы окна» под sway: нативное окно не разворачивается, sway (`xdg_wm_base` v2, без
    `wm_capabilities`) игнорирует запрос, и клиент не может узнать это заранее. Это запись
    в gaps (запись 5), а не условие.
  - Проверка: `native_linux` 2 из 2; `wayland_window` под Weston, `x11_window` под Xvfb.

## Удаление

- [ ] **T8. Удалить winit** (R6, R7).
  - Удаляются код хоста, признак `winit-backend`, условие `winit_host` и трассы `winit_*`.
  - Удаляются зависимости `winit`, `ui-events-winit` и `arboard`, а `crossbeam-channel` и
    `keyboard-types` — если других пользователей нет.
  - Удаляются записи `xtask`: `globals`, `reach`, `send-impls`, `panic-boundary`,
    `module_dag`.
  - `winit` попадает в запрещённые зависимости `reach`.
  - У `input::regions::presses` и у `ProjectedInputRegions::sent` из `cfg` убирается
    `winit_host`.
  - Отметки в ADR-0151 §11 и §15, ADR-0082 §5 и ADR-0286 §9.
  - Строки `docs/crates.md`.
  - Проверка: `cargo xtask checks`, clippy и nextest по рабочему пространству;
    `cargo tree -i winit` пуст.

## Проверка спеки

Спека удаляется из репозитория, когда T8 на `main`.
