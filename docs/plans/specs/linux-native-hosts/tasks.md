# linux-native-hosts — задачи

- **Статус:** утверждена владельцем 2026-11-03; исполнение. Ни одна задача ещё не начата.
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
- [ ] **T3. Drag and drop на нативном Wayland.** Приёмник — #2208, источник — PR с ветки
  `platform/wayland-dnd-source`, его открывают после #2208.

## Подготовка

- [ ] **T4. `linux_preferences` на нативном X11** (R5). Набор теряет требование
  `winit-backend` и поднимает свой Xvfb. Проверки остаются прежними. Проверка готовности:
  набор проходит под `cargo nextest run -p flui-native --test linux_preferences` без
  `winit-backend`.
- [ ] **T5. Появление и уход устройств ввода** (R3).
  - Сначала нужно сверить, сообщают ли нативные хосты об изменениях наличия устройств.
    - X11: `XI_HierarchyChanged`.
    - Wayland: `wl_seat.capabilities`, а также появление и уход `wl_seat`.
  - Если не сообщают, хосты начинают сообщать, как winit, и это проверяется тестом через
    публичный API, который проваливается без исправления.
  - Профиль X11 в `backend_conformance` берёт возможности у нативного X11.
- [ ] **T6. AT-SPI на окне Wayland** (R3). Нужен запуск `linux_atspi` или его случая на окне
  нативного Wayland под Weston. Тест должен проваливаться, если адаптер не подключён к окну
  Wayland.

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
