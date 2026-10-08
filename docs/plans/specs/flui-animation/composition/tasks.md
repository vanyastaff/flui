# flui-animation / composition — задачи

- **Статус:** черновик
- **Дата:** 2026-10-06
- **Design:** [design.md](design.md), [requirements.md](requirements.md), [../tasks.md](../tasks.md).
- **Порядок:** ветка и worktree на задачу; первый коммит — типы с инертными телами и красные по
  assertion тесты (вывод — в PR), второй — поведение. ID R/T — только в этом каталоге.

## Граф

```text
Q0 ─► T1 контракт ─► T2 дорожка+stagger ─┬─► [P] T3 cubic ─► (curves: удаление CatmullRom*)
                                          ├─► [P] T4 удаление TweenSequence (после F interpolation)
                                          ├─► T5 ActivityIndicator+RefreshIndicator (после W2 ownership, W3 integration)
                                          ├─► [P] T6 LinearProgressIndicator (после W2)
                                          └─► [P] T7 CupertinoActivityIndicator (после W2)
T2..T7 ─► T8 поверхность SDK, changelog, docs (строки ARCHITECTURE.md — через Z)
```

## Задачи

### T1 — контракт `Keyframes`/`Stagger` (после Q0)

- **Файлы:** `crates/flui-animation/src/keyframes.rs`, `src/stagger.rs` (новые), `src/lib.rs`
  (только свои `pub mod`/`pub use`), `tests/contracts/keyframes.rs`, строка `mod` в
  `tests/main.rs`.
- **Что:** сигнатуры design «Публичный API»; тела: `value_at` → `start.clone()`, `build` → `Ok`,
  `delay` → `ZERO`. Таблицы и proptest R1–R12, R21; эталоны Motion и Эрмита считаются в тесте.
- **DoD:** каждая строка компилируется и красная по assertion (строк, верных для заглушки,
  нет — проверено); fmt и clippy зелёные.

### T2 — поведение дорожки и stagger

- **Файлы:** `src/keyframes.rs`, `src/stagger.rs`.
- **Что:** `to`/`hold`/`jump`, правая непрерывность, `value_at_looped` по модулю `u128`,
  `Animatable`, ошибки builder (первая ошибка копится), замена неконечных сэмплов, `Stagger` с
  насыщением. Строка аллокаций `keyframes_value_at` в тесте аллокаций Q0.
- **DoD:** все строки T1, кроме cubic, зелёные; каждая падает при откате своего фрагмента
  (проверено в изолированном checkout); `cargo test --doc -p flui-animation`; `check-changed`.

### T3 [P] — cubic-сегменты (закрывает M-CRV-9)

- **Файлы:** `src/keyframes.rs`.
- **Что:** касательные Catmull-Rom по времени ключей, скорости стыков с соседями (`curve'`
  разностью `1e-4` при `build`), Эрмит в векторе, fallback на `p0`. Doctest R9.
- **DoD:** `cubic_keyframes_pass_through_keys`, `cubic_keyframes_are_c1_at_joins` зелёные и
  красные при откате; в PR — сообщение для спеки `curves`: удаление `CatmullRomCurve`/
  `CatmullRomSpline` (`curve.rs:796`, `:869`) разблокировано.

### T4 [P] — удаление `TweenSequence` (после слияния F interpolation; правка `tween_types.rs` через её владельца)

- **Файлы:** `src/tween_types.rs` (`:293-480`), `src/lib.rs:157-158` (и `:193`, если prelude ещё
  жив), `tests/contracts/tween.rs` (`weighted_sequences_*` → строки `keyframes_*` в
  `tests/contracts/keyframes.rs`: эндпоинты, относительный прогресс, огромные длительности),
  `README.md`, `docs/GUIDE.md`, `docs/PERFORMANCE.md`.
- **DoD:** `rg TweenSequence` вне `docs/plans` и `changelog.d` пуст; имена
  `weighted_sequences_*` в `docs/ARCHITECTURE.md:918-919` переданы задаче Z; `flui-sdk`
  `tests/surface.rs` и `packages/` не ссылаются (проверено `rg`); `check-changed`.

### T5 — `ActivityIndicator` и спиннер `RefreshIndicator` (после T2, W2, W3)

- **Файлы:** `crates/flui-widgets/src/controls/activity_indicator.rs` (новый),
  `controls/mod.rs`, `src/lib.rs` (строка реэкспорта), `scroll/refresh_indicator.rs` (`:23-24`,
  `:465`, `:491-497`), `tests/activity_indicator.rs` + `mod` в `tests/main.rs`.
- **Что:** три дорожки design «Потребители», один контроллер с `repeat`, painter с `repaint()`,
  `SemanticsConfiguration` с ролью `LoadingSpinner`; замена `ColoredBox` в `RefreshIndicator`.
- **Тесты:** `activity_indicator_arc_follows_keyframes` (R13, R22: `DrawOp::Arc` в 0/300/1500/
  1800/3000/4800/6000 мс, `zero_dt`, `ten_hours`, build-счётчик), `refresh_indicator_spins_while_refreshing`
  (R14), `indicator_unmount_mid_frame_releases_controller` (R18), `two_indicators_share_one_vsync`
  (R19), `indicator_realm_stop_mid_repeat` (R20), `indicator_paused_paints_static_frame` (R23),
  строка a11y R17.
- **DoD:** каждый тест красный без production-фрагмента (семантика, repaint, регистрация);
  `cargo xtask module-dag` разрешает `scroll → controls`; `check-changed`.

### T6 [P] — `LinearProgressIndicator` (flui-material, после T2, W2)

- **Файлы:** `packages/flui-material/src/progress_indicator.rs` (новый), `src/lib.rs`,
  `tests/progress_indicator.rs` + `mod` в `tests/main.rs`.
- **Что:** `value: Option<f64>` (NaN и вне `[0,1]` — clamp, NaN → 0, документировано); без
  значения — четыре дорожки с ведущими `hold`; роли R17. Только `flui_sdk::…`.
- **Тесты:** `linear_progress_indeterminate_keyframes` (R15: концы полос на плато задержек и в
  концах сегментов), `linear_progress_switches_to_determinate` (контроллер снят, доля нарисована).
- **DoD:** как T5; `cargo xtask reach` зелёный (только `flui-sdk`).

### T7 [P] — `CupertinoActivityIndicator` (flui-cupertino, после T2, W2)

- **Файлы:** `packages/flui-cupertino/src/activity_indicator.rs` (новый), `src/lib.rs`,
  `tests/activity_indicator.rs` + `mod` в `tests/main.rs`.
- **Что:** дорожка `jump`+`hold` по таблице альф, `Stagger(125 мс, First)` + `value_at_looped`,
  радиус по умолчанию 10, роль `LoadingSpinner`.
- **Тесты:** `cupertino_activity_indicator_ticks_step` (R16: альфа каждого тика в 0, 124, 125,
  999, 1000 мс против формулы Flutter, посчитанной в тесте), строка a11y.
- **DoD:** как T5.

### T8 — поверхность SDK, changelog, документация

- **Файлы:** `crates/flui-sdk/tests/surface.rs` (`Keyframes`, `KeyframesError`, `Stagger`,
  `StaggerOrigin` в `measured`), `changelog.d/<branch-slug>.md` (текст — design), `README.md` и
  `docs/GUIDE.md` крейта (раздел keyframes); строки для `docs/ARCHITECTURE.md` (решения и имена
  тестов) — задаче Z.
- **DoD:** `cargo xtask checks`; `market.md` M-CMP-1/2/4, M-CRV-9 → «есть» с именами тестов.
