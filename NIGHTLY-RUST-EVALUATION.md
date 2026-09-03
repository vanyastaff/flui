# Nightly Rust для FLUI: оценка перехода, feature-варианта и канарейки

Дата: 2026-09-03. Документ только оценочный — код, манифесты и CI не менялись.

| Что | Значение | Источник |
|---|---|---|
| Dev-toolchain (пин) | `1.98.1` (stable, релиз 2026-09-01) | `rust-toolchain.toml` |
| MSRV | `1.97` | `[workspace.package].rust-version`, `clippy.toml`, `msrv` CI job |
| Текущий stable / beta / nightly | 1.98.1 / 1.99 (стабилизируется **2026-10-01**) / 1.100 (**2026-11-12**) | [releases.rs](https://releases.rs/) |
| Проверенный nightly | `rustc 1.100.0-nightly (2e2b193f8 2026-09-02)` — уже установлен локально | `rustup toolchain list` |
| Где nightly уже используется | только advisory `miri` job (`continue-on-error`) и `just miri`; `#![cfg_attr(docsrs, feature(doc_cfg))]` в `flui-foundation`/`flui-log` | `.github/workflows/ci.yml`, `justfile` |
| Действующая политика | «Code MUST use stable Rust. Nightly features require an accepted ADR and a separate compatibility strategy» | `STYLE.md` §2 |

---

## 1. Короткий ответ

**Не переходить на nightly как основной toolchain и не добавлять `nightly` cargo-feature.** Ни одна из целей проекта (порт Flutter-семантики, детерминированный pipeline, MSRV-обещание потребителям) не требует unstable-фич, а nightly сейчас находится в самом нестабильном состоянии за несколько лет: в течение одного месяца на нём включили по умолчанию сразу три крупнейших изменения компилятора (Polonius Alpha — с nightly-2026-08-06, новый trait solver — с 2026-08-22, parallel frontend — на подходе). Оптимизаций *кода* nightly не даёт — backend тот же LLVM.

**Что стоит сделать вместо этого** (все пункты — stable-совместимы, детали в §6):

1. **Добавить advisory nightly-канарейку** в `weekly.yml` (`cargo +nightly check/clippy`, `continue-on-error`). Один такой прогон, сделанный для этого документа, уже нашёл две проблемы, которые ударят по stable-CI в известные даты (п. 2–3).
2. **До 2026-10-01** заменить 8 вызовов `Atomic::fetch_update` на `Atomic::try_update` (stable с 1.95 ≤ MSRV; `fetch_update` объявлен deprecated с 1.99). Все PR-джобы ставят *latest stable* и работают под `CARGO_BUILD_WARNINGS: deny` → в день релиза 1.99 CI станет красным.
3. **До стабилизации нового solver'а («в ближайшие месяцы»)** закрыть future-incompat lint `recursion_depth_exceeding_limit` в `flui-engine`/`flui-app` (+4 корневых примера): станет hard error.
4. Исправить гигиену манифестов, которую показали cargo-lints на nightly: два примера без `[lints] workspace = true` (нарушение `STYLE.md`), `serde_json` в `flui-devtools` не завязан на feature `timeline`, три неиспользуемых `[workspace.dependencies]`.
5. Nightly-only инструменты оставить **изолированными по одному** (как уже сделано с miri) — не через канал всего workspace.

Если когда-нибудь появится реальная потребность в unstable-фиче — путь один: ADR (как требует `STYLE.md`) + `cfg(...)`-гейт с полностью протестированным stable-путём, а не `channel = "nightly"`.

---

## 2. Что показал прогон на nightly 1.100 (эксперимент, а не мнение)

Методика: холодный `cargo +nightly check --workspace --all-targets --locked` в scratch-`CARGO_TARGET_DIR` под `/tmp`, `RUSTC_WRAPPER=""` (без sccache), `CARGO_INCREMENTAL=0`, 32 ядра / `jobs = 16`. Репозиторий не тронут (`git status` чистый). Артефакты удалены.

| Прогон | Результат | Время |
|---|---|---|
| nightly 1.100, обычный | **0 ошибок**, 74 предупреждения (см. ниже) | 62 s |
| nightly 1.100, `RUSTFLAGS=-Zthreads=8` (parallel frontend) | 0 ошибок | 64 s |
| stable 1.98.0 (baseline) | не получен: локальный `1.98.0` установлен частично (rustup не смог восстановить `rust-std`) | — |

Выводы из этого:

- **Workspace компилируется под новым solver'ом и Polonius Alpha без ошибок.** Это лучший из возможных сигналов о том, что стабилизация этих двух вещей (обе запланированы до конца 2026) пройдёт для FLUI безболезненно.
- **Parallel frontend на этом workspace не даёт выигрыша** для `check`: 28 крейтов уже насыщают 16 job'ов, а `-Zthreads=8` поверх `jobs=16` — это переподписка. Он помог бы только на критическом пути одиночного тяжёлого крейта (`flui-widgets`, `flui-rendering`), когда всё остальное уже собрано. Для `build` цифры могут отличаться, но ожидать «в разы» не стоит.
- **Новые диагностики** (ни одной нет на stable 1.98):

| Диагностика | Где | Что это значит |
|---|---|---|
| `deprecated: fetch_update → try_update` ×8 | `flui-foundation/src/key.rs:190,578`, `flui-hot-reload/src/plugin.rs:365`, `flui-interaction/src/arena/mod.rs:328,852`, `flui-platform/src/platforms/headless/platform.rs:53`, `flui-widgets/src/navigator/navigator.rs:399` | `try_update` — `#[stable(since = "1.95.0")]`, `fetch_update` — `#[deprecated(since = "1.99.0")]` (проверено по `library/core/src/sync/atomic.rs` nightly). Ударит по stable-CI 2026-10-01. Комментарий в `ci.yml` (miri job) о том, что `try_update` «does not exist on the declared MSRV yet», устарел с момента переезда MSRV на 1.97 |
| `recursion_depth_exceeding_limit` (future-incompat) ×6 | `flui-engine/src/raster.rs:87` (`Renderer: Send`), `flui-app/src/app/direct.rs:172`, примеры `wgpu_window`, `scene_render`, `filter_demo`, `color_filter_demo` | Новый solver честно считает глубину рекурсии при доказательстве `wgpu::Renderer: Send` (цепочка через `wgpu_core::Global → Hub → Registry → RwLock<Storage<…>>`); старый её недосчитывал. Станет hard error ([tracking #159228](https://github.com/rust-lang/rust/issues/159228)). Рекомендуемый фикс — ручные `unsafe impl Send/Sync` для промежуточных типов-обёрток над wgpu (короткое замыкание вывода) или `#![recursion_limit = "256"]` на крейт. Та же проблема у других wgpu-потребителей ([#160036](https://github.com/rust-lang/rust/issues/160036)) |
| `cargo::missing_lints_inheritance` ×2 | `examples/painting_demo/Cargo.toml`, `examples/web_demo/Cargo.toml` | Прямое нарушение `STYLE.md` §2 («Every crate MUST inherit workspace lints»), которое stable-cargo сегодня не видит |
| `cargo::unused_dependencies` | `flui-devtools`: `serde_json` | Подтверждено: `serde_json` используется только в `src/timeline.rs`, который под `#[cfg(feature = "timeline")]`, а зависимость обязательная → должна быть `optional` и входить в `timeline = [...]` |
| `cargo::unused_workspace_dependencies` ×3 | `dyn-clone`, `downcast-rs`, `flui-testing` в `[workspace.dependencies]` | Подтверждено: члены объявляют их напрямую (`flui-rendering`/`flui-view`: `dyn-clone = "1.0"`, `downcast-rs = "2.0"`; `flui-testing` — везде `path = "../flui-testing"`), а не через `.workspace = true`. Либо перевести на `workspace = true`, либо убрать из корня |
| `cargo::manual_readme` ×15 | `readme = "README.md"` в 15 крейтах | Косметика: cargo выводит это сам |

cargo-lints (`unused_dependencies`, `missing_lints_inheritance`, …) на nightly cargo 1.100 сработали **без** `-Zcargo-lints` — система линтов стабилизирована upstream ([cargo#17298](https://github.com/rust-lang/cargo/pull/17298); FCP — в [TWiR 665](https://this-week-in-rust.org/blog/2026/08/19/this-week-in-rust-665/), merge — в списке [TWiR 667](https://this-week-in-rust.org/blog/2026/09/02/this-week-in-rust-667/) за неделю до 2026-09-02). Master в этот момент = 1.100 → ожидать в **stable 1.100 (2026-11-12)**; в опубликованном changelog строки пока нет, проверить при бампе.

---

## 3. Почему nightly не даёт «лучших оптимизаций»

Это главный миф, который стоит развеять до обсуждения вариантов.

- **Codegen одинаковый.** Stable и nightly одной версии используют один и тот же LLVM; `-C opt-level`, `lto`, `codegen-units`, `target-cpu`, PGO (`-Cprofile-generate/-use`) — всё stable. В `Cargo.toml` уже стоят `lto = "thin"`, `codegen-units = 1`, `strip = "debuginfo"`. Следующие ступени для *runtime* — PGO и `-C target-cpu` для собственных бинарей — не требуют nightly.
- **Что nightly даёт для runtime — узко и не про FLUI:** `portable_simd` (у нас уже `glam` с SIMD), `allocator_api` (Slab-арены не выиграют заметно), `#[optimize(size|speed)]` (PR стабилизации открыт), `build-std` (пересборка std с `opt-level = "z"`/`panic = "abort"` — актуально только для размера wasm/Android-бинарей, см. §5).
- **Что nightly даёт для *времени сборки*** — это единственная реальная область, и она измеряется, а не предполагается: parallel frontend (измерено: 0 % на `check`), Cranelift backend (по данным проекта — 5–15 % на инкрементальных debug-сборках, до ~30 % на отдельных крейтах при `line-tables-only`, [goal](https://goals.rust-lang.org/2026/improve-cg_clif-performance.html)), `hint-mostly-unused` для тяжёлых редко используемых зависимостей (`windows-sys`, `web-sys`, `js-sys`).

---

## 4. Варианты

### A. Перевести workspace на nightly (`channel = "nightly-YYYY-MM-DD"`)

Против (каждый пункт — самостоятельный блокер):

- **Ломает контракт MSRV.** `rust-version = "1.97"` — обещание потребителям (`docs/PORT.md` §MSRV: «cheap to give, expensive to retract»). Как только в коде появляется `#![feature(...)]`, крейт не собирается ни одним stable → `msrv` job, шаблоны `flui-cli` (`rust-version = "1.97"`), `cargo add flui` из README — всё теряет смысл.
- **Максимальная волатильность именно сейчас.** Новый trait solver включён по умолчанию с nightly-2026-08-22 ([blog](https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/), «largest single change to the Rust compiler since its initial stable release»; авторы прямо предупреждают: код на nightly может начать полагаться на вывод типов, которого нет на stable). Polonius Alpha — с nightly-2026-08-06 ([blog](https://blog.rust-lang.org/2026/08/04/enabling-polonius-alpha-on-nightly/)), допустимый регресс времени компиляции 10–20 %, редкие worst-case 2–3× ([goal](https://goals.rust-lang.org/2026/polonius.html)). Parallel frontend — MCP принят 2026-08-17, план: включить на nightly по умолчанию и собирать фидбек ≥ 3 месяца ([compiler-team#1005](https://github.com/rust-lang/compiler-team/issues/1005)). Быть тест-полигоном для всех трёх одновременно — не задача UI-фреймворка.
- **`-D warnings` + nightly clippy/rustc = постоянно красный CI.** Каждый nightly приносит новые линты (пример из этого же workspace: `unused_async_trait_impl` появился в pedantic 1.98 и уже потребовал `allow` в корневом `Cargo.toml`). На пине даты это лечится, но каждый бамп пина превращается в отдельную PR-волну.
- **rustfmt.** Пин nightly = nightly rustfmt; любой контрибьютор на stable будет форматировать иначе → `fmt-check` падает. `style_edition = "2027"` и `imports_granularity`/`group_imports`/`wrap_comments` по-прежнему unstable ([rustfmt Configurations.md](https://github.com/rust-lang/rustfmt/blob/HEAD/Configurations.md)).
- **Две даты вместо одной.** miri требует свой nightly-компонент; сегодня это `+nightly` поверх stable-пина. С nightly-пином появятся два пина (dev и miri), которые надо двигать согласованно.
- **Что бы это дало:** TAIT (убрать часть `Box<dyn FnMut>` в колбэках), `portable_simd`, `trait_alias`, `try_blocks`, `never_type`. Ничего из этого не нужно Prime Directive; TAIT к тому же заблокирован до стабилизации нового solver'а и намечен на «late this year» ([goal](https://goals.rust-lang.org/2026/rtn.html)) — то есть придёт в stable сам.

Вердикт: **нет.**

### B. Добавить `nightly` как cargo-feature (`#![cfg_attr(feature = "nightly", feature(...))]`)

Прецеденты: `hashbrown` (`nightly`), `smallvec` (`specialization`, `may_dangle`) — низкоуровневые библиотеки с крошечной гейтированной поверхностью. Для FLUI это плохо ложится:

- **Feature unification.** Features аддитивны и объединяются по всему графу: один крейт (или downstream-приложение), включивший `flui-foundation/nightly`, ломает сборку на stable для всех. Именно поэтому экосистема ушла от `nightly`-features к `--cfg` через `RUSTFLAGS`/`build.rs`-детекции (`rustversion::nightly`).
- **Конфликт с существующим CI.** `feature-matrix` job гоняет `cargo hack --each-feature` на **stable** → включит `nightly` и упадёт. Придётся `--exclude-features nightly`, и тогда nightly-путь **не проверяется ничем** — прямое нарушение «Done means verified»: второй код-путь без тестов. Чтобы его тестировать, нужен второй набор job'ов на nightly → матрица удваивается.
- **`unexpected_cfgs = "deny"`.** Если делать через `--cfg flui_nightly`, cfg надо зарегистрировать: `[lints.rust] unexpected_cfgs = { level = "deny", check-cfg = ['cfg(flui_nightly)'] }` — иначе deny сработает. Решаемо, но это ещё один контракт, который надо поддерживать.
- **Что реально можно было бы гейтить:** `portable_simd` в `flui-geometry` со stable-фолбэком (спорная польза при уже имеющемся `glam`); `doc_cfg` — **уже сделано правильно** через `cfg(docsrs)` (docs.rs собирает nightly, PR стабилизации `doc_cfg` открыт).

Вердикт: **нет** как общий механизм. Единственный уместный «feature-подобный» гейт — уже существующий `cfg_attr(docsrs, feature(doc_cfg))`.

### C. Stable-пин + advisory nightly-канарейка + изолированные nightly-инструменты — **рекомендуется**

Это ровно текущая модель (`miri` job), расширенная одним прогоном:

- **Канарейка** ловит будущие поломки заранее, с датой. Прогон для этого документа нашёл два таких случая (§2). Стоимость — один job в `weekly.yml`, `continue-on-error`, `CARGO_BUILD_WARNINGS: warn` (как у miri, чтобы не фрагментировать кеш). Полезно также снимать `cargo report future-incompatibilities` и иметь тумблеры для бисекции: `-Zpolonius=off`, `-Znext-solver=coherence`.
- **Инструменты — по одному, каждый со своей причиной:** miri (есть), `-Zrandomize-layout` (можно добавить в *существующий* miri job — ловит скрытые допущения о layout в `unsafe`/FFI-коде `flui-platform`), `-Zdirect-minimal-versions` (проверяет честность объявленных floors вроде `tokio = "1.43"` — раз в неделю), cargo-lints (пока на stable не приедут — раз в неделю на nightly), Cranelift **локально по желанию разработчика** (`cargo +nightly build -Zcodegen-backend`, не в CI).

Эскиз job'а (предложение, не применено):

```yaml
  nightly-canary:
    name: nightly canary (check + clippy, advisory)
    runs-on: ubuntu-latest
    timeout-minutes: 30
    continue-on-error: true
    env:
      CARGO_BUILD_WARNINGS: warn   # как у miri: nightly deprecates раньше stable
    steps:
      - uses: actions/checkout@<sha>
        with: { persist-credentials: false }
      - uses: dtolnay/rust-toolchain@<sha> # nightly
        with: { components: clippy }
      - run: cargo +nightly check --workspace --all-targets --locked
      - run: cargo +nightly clippy --workspace --all-targets --locked
      - run: cargo +nightly report future-incompatibilities || true
```

И локальный рецепт для `justfile`: `nightly-check: cargo +nightly check --workspace --all-targets --locked` (рядом с `miri`).

### D. Агрессивнее двигать *dev-пин* stable — уже политика

`docs/PORT.md`: MSRV бампается не позже 6 недель после релиза; dev-пин может бежать впереди. 1.99 выходит 2026-10-01 — это и есть момент, когда п. 2 из §1 обязателен, а не желателен.

---

## 5. Инвентарь unstable-возможностей, релевантных FLUI

Статусы — на 2026-09-03, по первичным источникам (tracking issue / project goal / release notes).

| Возможность | Статус | Польза для FLUI | Вердикт |
|---|---|---|---|
| **Next-gen trait solver** | По умолчанию на nightly с 2026-08-22; стабилизация «в ближайшие месяцы» ([#160895](https://github.com/rust-lang/rust/issues/160895)) | Workspace уже компилируется под ним (§2). Один FCW (`recursion_depth_exceeding_limit`) | Закрыть FCW сейчас; больше ничего |
| **Polonius Alpha** | По умолчанию на nightly с 2026-08-06; цель — stable до конца 2026 | Разрешает NLL problem case #3 / lending-iterator паттерны — актуально для `get_two_mut`-стиля в аренах | Компилируемся; ждать stable |
| **Parallel frontend** (`-Zthreads`, будущий `--jobs-frontend`) | MCP принят 2026-08-17; nightly-тест ≥ 3 мес. | Измерено 0 % на `check` этого workspace | Не трогать; перемерить на `build` когда станет default |
| **Cranelift backend** (`-Zcodegen-backend=cranelift`, компонент `rustc-codegen-cranelift-preview`) | Nightly-компонент; Linux/macOS x86_64+aarch64; unwinding «experimental», debuginfo нет ([goal 2025h2](https://goals.rust-lang.org/2025h2/production-ready-cranelift.html)) | 5–15 % на инкрементальных debug-сборках. Но `dev` профиль тут `opt-level = 1`, deps `opt-level = 2` — Cranelift не оптимизирует, так что тесты замедлятся | Только локально, по желанию; не в CI |
| **`hint-mostly-unused`** (`-Zprofile-hint-mostly-unused`) | Nightly-only; `[hints] mostly-unused = true` в манифесте крейта безвреден для старых cargo ([Cargo unstable](https://doc.rust-lang.org/cargo/reference/unstable.html)) | Кандидаты: `windows-sys`, `web-sys`, `js-sys` | Померить локально; ждать stable |
| **build-std** | RFC 3874/3875 приняты; реализация в cargo идёт ([cargo#17398](https://github.com/rust-lang/cargo/pull/17398)) | Нужен для многопоточного wasm32 (`+atomics` — закомментировано в `.cargo/config.toml`) и размера wasm/Android | Ждать; это единственная unstable-вещь, которая когда-нибудь понадобится по-настоящему |
| **TAIT / RTN** | Nightly; заблокированы новым solver'ом; цель — «late this year» | Именовать типы замыканий без `Box<dyn FnMut>` в колбэках | Ждать stable |
| **Specialization / `min_specialization`** | `min_specialization` unsound ([#149257](https://github.com/rust-lang/rust/issues/149257)); 2026 — только design work | Соблазн для диспетчеризации render-объектов | **Нет** |
| **`generic_const_exprs`** | T-types планирует убрать и заменить (упомянуто в [#160895](https://github.com/rust-lang/rust/issues/160895)) | Соблазн для Arity-системы | **Нет**, тупиковая ветка |
| **`portable_simd`** | Nightly; RFC ещё не написан ([#86656](https://github.com/rust-lang/rust/issues/86656)) | Геометрия/painting | `glam`/`wide` на stable |
| **Sanitizers** | Tier-2 таргеты `x86_64-unknown-linux-gnu{asan,msan,tsan}` смержены (ASan — [#149644](https://github.com/rust-lang/rust/pull/149644), MSan/TSan — [#152757](https://github.com/rust-lang/rust/pull/152757), 2026-03); стабилизация флага ждёт [#123617](https://github.com/rust-lang/rust/pull/123617) | Дополнение к miri для арены и FFI. По тексту goal'а ASan-таргет задуман для работы со stable-компилятором — проверить | Опционально, отдельным advisory job'ом |
| **Miri** | Nightly-only, уже используется | — | Оставить; добавить `-Zrandomize-layout` |
| **rustfmt unstable** (`imports_granularity`, `group_imports`, `wrap_comments`, `style_edition = 2027`) | Nightly-only | Косметика | Нет — ломает `fmt-check` у stable-контрибьюторов |
| **cargo-lints** | Стабилизированы upstream; на nightly cargo уже включены | Находки в §2 | Исправить находки; включить `[lints.cargo]` когда приедет stable |
| **`doc_cfg`** | PR стабилизации открыт | Уже используется через `cfg(docsrs)` | Ничего не делать |
| Ожидаемые stabilization PR (по [releases.rs](https://releases.rs/)): `mpmc_channel`, `core::mem::DropGuard`, `#[optimize]`, `Result::into_ok`, `Box::take`, `debug_closure_helpers`, `#![rustfmt::skip]` inner-attr, `-Cdebuginfo-compression` | Открыты | `mpmc_channel` в std — потенциальная замена `crossbeam-channel`; `DropGuard` — для frame-transaction guards | Следить |

### 5a. Что добавляет This Week in Rust (выпуски 664–667, 12 авг – 2 сен 2026)

[TWiR](https://this-week-in-rust.org/) — правильный *второй* источник для канарейки: job показывает, что уже сломалось, а разделы **Calls for Testing** и **Final Comment Period** показывают, что сломается или стабилизируется через 6–12 недель. Читать раз в неделю вместе с результатом weekly-прогона. Что в последних четырёх выпусках касается FLUI напрямую:

| Пункт | Выпуск | Значение для FLUI |
|---|---|---|
| **cargo-lints стабилизированы** (`diag: Stabilize cargo-lints` в merged) | 667 | Закрывает открытый вопрос: ждать в stable 1.100. Находки §2 можно закрывать уже сейчас |
| **`never_type` — T-types FCP на стабилизацию** | 667 (FCP) | `!` как тип после ~10 лет; пригодится для `Result<T, !>` в инфраллибельных путях. Ждать stable, не гейтить |
| **`core::mem::DropGuard` — FCP на стабилизацию** | 667 (FCP) | Кандидат для frame-transaction / borrow-checkout guard'ов (`PipelineCell`) вместо ручных `impl Drop` |
| **RFC `hints.min-opt-level` принят** | 664 (Approved) | Крейт сможет объявить минимальный `opt-level` для себя — прямая замена ручному `[profile.dev.package."*"] opt-level = 2`, которое сейчас стоит в корневом `Cargo.toml`. Когда доедет до stable — упростить профили |
| **RFC 3416 «feature descriptions» реализуется** (`manifest!: implement feature-metadata`), RFC «Cargo feature descriptions» в FCP | 667 | Описания features прямо в `Cargo.toml` — заменит комментарии над `material`/`cupertino`/`hot-reload` в фасаде |
| **`-Zembed-metadata` — Call for Testing**; в 1.100 nightly включён `-Zembed-metadata=no` по умолчанию | 665, 666 | Метаданные не дублируются в `.rlib` → меньше `target/` (для этого workspace — 19 ГБ, отдельная боль по `AGENTS.md`). Проверить локально размер `target/` под nightly |
| **`-Zprofile-sample-use` стабилизирован** (AutoFDO) | 665 | Ещё один stable-инструмент runtime-оптимизации без nightly (рядом с PGO) |
| **Polonius Alpha дал +3.0 % времени компиляции** на nightly; LLVM 23 дал «massive» улучшения | 664 (perf triage) | Подтверждает оценку «10–20 % допустимо»; регресс частично компенсируется LLVM 23, который придёт в stable сам |
| **Clippy: PGO-сборка; новые линты** (`option_zip_none`, nonzero operators, `manual_contains`/`needless_bool` изменения) | 665, 666 | Это и есть источник churn'а под `-D warnings`: каждый бамп dev-пина — ревизия новых pedantic-линтов (как с `unused_async_trait_impl` в 1.98) |
| Cargo: `allow overriding inherited default-features in 2024`, built-in profile `debug`, `min-publish-age` стабилизирован | 664, 666 | `min-publish-age` — supply-chain защита, в связке с `deny.toml`; built-in `debug` не конфликтует с локальным `[profile.dbg]` |
| `i686-pc-windows-msvc` → Tier 1 без host tools | 667 (Approved) | Не влияет: `cross-typecheck` использует `x86_64-pc-windows-msvc` |

Ни один из этих пунктов не требует nightly сегодня и не меняет вердикт §1 — но каждый из них появился бы в stable-CI внезапно, если бы за FCP-разделом никто не следил.

### 5b. Что сделали соседи за последний год (changelog'и egui, Bevy, GPUI, Xilem/Masonry, Iced, Dioxus, Slint, Leptos)

Смотрел релизы с осени 2025 по сентябрь 2026 — только первичные источники (release notes / CHANGELOG / блог проекта). Цель не «скопировать фичи», а увидеть, куда сходится рынок и где FLUI уже впереди, а где отстаёт. Сопоставление с FLUI — по `grep` по `crates/`.

| Проект | Последний релиз | Что важно для FLUI |
|---|---|---|
| **egui** | 0.34 (2026-03), 0.35 (2026-06-25) | 0.34: рендер шрифтов **`ab_glyph` → `skrifa` + `vello_cpu`** — hinting и variable fonts; `Ui` вместо `Context` как точка входа. 0.35: **`egui_inspection` протокол + `egui_mcp`** — агент читает accesskit-дерево запущенного приложения, шлёт события, снимает состояние (`EGUI_INSPECTION=1`, порт 5719); CSS-like classes; переделанный IME-композинг |
| **Bevy** | 0.19 (2026-06-19) | **`cosmic-text` → `parley`** («meaningfully better documentation, nicer to use»); `FontSource::{Handle, Family, Monospace…}` + system font discovery; variable fonts (`weight`/`width`/`style`); `FontSize::{Px, Vh, Rem}`; `LetterSpacing`; **`EditableText`** с IME, bidi, multi-click, фильтрами; **`RenderErrorHandler`** — политика на `DeviceLost`/OOM (Recover / StopRendering / Ignore); `AccessibleLabel` как отдельный компонент; более гранулярные feature-коллекции |
| **GPUI** (Zed) | crates.io 0.2.2 (2025-10-22, с тех пор **не публиковался**) | На main: Linux-рендерер **blade → wgpu** (2026-02, крейт `gpui_wgpu`); **AccessKit** (2026-05-27); split на `gpui` + `gpui_platform`/`gpui_macos`/`gpui_linux`/`gpui_windows`/`gpui_web` + отдельный `scheduler`; таффи-layout. Текст: CoreText/DirectWrite нативно, cosmic-text 0.19 на Linux/wgpu. macOS-слой **до сих пор на `cocoa`/`objc`**. Changelog'а нет — сообщество ведёт `gpui-release-notes` |
| **Xilem / Masonry** | 0.4 (2025-10-29); Q1-2026 отчёт | 0.4: shaping **swash → HarfRust**; первичная поддержка **multi-window**; `masonry_testing`. Q1 2026: Masonry ушёл с hardcoded Vello на абстракцию **`imaging`** (можно рендерить через `vello_cpu` без GPU); IME и системная интеграция через **`ui-events`** — «reduces the dependency on winit»; новая layout-система. Parley 0.10 (2026-06-01): harfrust 0.8, `complex-scripts` (словарный перенос для CJK/Thai/Khmer); все AccessKit text properties. Vello Hybrid — «roughly beta quality», sparse-strips 0.0.9 (2026-05) **без гарантий API** |
| **Iced** | 0.14 (2025-12-07) | Reactive rendering; **`comet`** — time-travel debugger с presentation metrics; **headless mode** `iced_wgpu`; hot reload; IME (размер курсора, preedit); `Send` для `Renderer` через `Arc` в кешах; один SDF-шейдер для quad'ов; фикс `SurfaceError::Lost`/`Outdated` (реконфигурация); wgpu 27, cosmic-text 0.15, Rust 2024 |
| **Dioxus** | 0.7 (2025-10-31), 0.8-alpha.1 (2026-07-31) | **Subsecond** — hot-patching Rust-кода в рантайме (в 0.8 включён по умолчанию); **Dioxus Native / Blitz** на `vello_hybrid`/`vello_cpu` — incremental rendering, custom elements; `LLMs.txt`; **порт `cocoa`/`objc` → `objc2`** (0.8) |
| **Slint** | 1.15 (2026-02), 1.16 (2026-04), 1.17 (2026-06-24) | 1.15: text/images на пиксельных границах, `oklch`; 1.16: `StyledText` + `@markdown`, multi-touch `ScaleRotateGestureHandler`, **swash для растеризации глифов**, wgpu-Skia на LinuxKMS; 1.17: **встроенный MCP-сервер** (инспекция через a11y-дерево, инъекция ввода, скриншоты) + skills для AI-ассистентов; drag-and-drop, system tray, tooltips; cross-axis alignment; remote viewer для телефона |
| **Leptos** | 0.8.19; 0.9-beta (2026-07-18) | Web-домен, прямого пересечения мало, но одна вещь по теме этого документа: **сигналы теперь вызываются как функции на stable** через `Deref<Target = dyn Fn>` — Leptos убрал историческую причину требовать nightly (`fn_traits`). Ещё один пример, что экосистема уходит *от* nightly-only фич, а не к ним |

**Что из этого следует для FLUI — по темам:**

1. **Текстовый стек сходится к Linebender** (`skrifa`/`harfrust`/`parley`/`glifo`): egui, Bevy, Xilem — все перешли за год; Slint взял `swash`; на `cosmic-text` остались Iced и GPUI-Linux. FLUI на `cosmic-text` 0.19 + `glyphon` 0.12. У FLUI уже есть типы `PlaceholderSpan`/`PlaceholderDimensions` (`flui-types`, `flui-painting/text_painter`) — то есть Flutter-контракт inline placeholders описан, но реализуется поверх библиотеки, у которой нет inline boxes. **Font hinting в FLUI — 0 упоминаний**; color emoji (COLR) — 0. Это подтверждает и усиливает spike из §6b: parley закрывает и placeholders, и hinting, и variable fonts, и complex-scripts одним переходом.
2. **Рендер: wgpu — консенсус, vello — ещё нет.** GPUI выбросил blade ради wgpu; Iced, egui, Slint (LinuxKMS) — на wgpu. Vello Hybrid принят только Dioxus/Blitz и Masonry (через `imaging`), и сам Linebender называет его beta без API-стабильности. Собственный wgpu-растеризатор FLUI + lyon — та же лига, что Iced и GPUI. **Не менять; наблюдать** sparse-strips до заявленной стабилизации API.
3. **Инспекция и MCP стали конвенцией за одно лето** (egui 0.35 — июнь, Slint 1.17 — июнь, Dioxus `LLMs.txt`). Схема у всех одна: читать accessibility-дерево запущенного приложения + инъекция ввода + снимок. У FLUI все три составляющих уже есть по отдельности — `flui-semantics` (133 файла), `HeadlessBinding`/синтетические события, `examples/screenshot.rs`, `flui-devtools` timeline — но **нет протокола, который их выставляет** (0 упоминаний MCP). Это дешёвая и высоко-левереджная вещь: `AGENTS.md` сам описывает боль «OS screenshot tools can't grab the live window». Это и есть тот «devtools»-слой, который Flutter даёт через VM Service. Предложение: ADR на inspection-протокол поверх семантического дерева (транспорт — локальный TCP/stdio, как у egui), `flui-mcp` как отдельный бинарь-потребитель.
4. **Устойчивость к потере устройства — FLUI уже на уровне.** Bevy 0.19 только сейчас добавил `RenderErrorHandler`; Iced/egui чинили `SurfaceError::Lost` и surface lifecycle патчами. У FLUI `device_recovery.rs`, `Occluded` (8 файлов) и live-smoke с реальной окклюзией — это впереди рынка, стоит это явно зафиксировать в README/ROADMAP как отличие.
5. **Тестирование и отладка:** Iced `comet` (time-travel + presentation metrics), egui `kittest`+`kitdiff`, `masonry_testing`. У FLUI headless-bootstrap, insta-снапшоты, live-smoke под Xvfb/weston, render-harness — по глубине не хуже; нет только пошагового просмотра кадров — он естественно ложится на протокол из п. 3.
6. **Платформенный слой:** философия FLUI (нативные Win32/AppKit, winit как fallback) совпадает с GPUI и с тем, куда идёт Masonry (`ui-events`, меньше winit). FLUI уже использует `ui-events`/`keyboard-types`/`dpi`/`cursor-icon` — правильно. При этом Dioxus уже перешёл на `objc2`, GPUI ещё нет — миграция из §6b ставит FLUI в первую группу.
7. **Hot reload:** Dioxus Subsecond (hot-patching функций, по умолчанию с 0.8) — принципиально другой подход, чем dlopen-плагины `flui-hot-reload`. Не менять, но сравнить в ADR по hot-reload: Subsecond — независимый крейт, его можно оценить как второй режим.
8. **Кадэнс и стабильность API** как рыночная позиция: GPUI не публикуется 10+ месяцев и без changelog'а; Xilem — alpha; Dioxus Native — alpha. Регулярные релизы с changelog'ом и MSRV-политикой (§4D) — конкурентное преимущество, которое стоит меньше любой фичи. Это ещё один аргумент против nightly-пина (§4A): он сделал бы FLUI похожим на то, от чего рынок устал.

### 5c. Остальной рынок: новые и не рассмотренные ранее фреймворки

Главный источник — [«A 2026 Survey of Rust GUI Libraries»](https://blog.wybxc.cc/blog/rust-gui-survey-2026/) (2026-08-23, продолжение обзора boringcactus 2025): автор прошёл **все** библиотеки с Are We GUI Yet одной задачей — текстовое поле + картинка из `image`-крейта — и мерил то, что «труднее всего подделать»: **IME (composer) и screen reader**. Плюс собственные release notes проектов.

**Прямые архитектурные соседи (Flutter-подобные, three-tree):**

| Проект | Статус | Чем отличается от FLUI |
|---|---|---|
| **Vexo** | Появился в 2026, 2★, «early-stage but real» | Тоже widget → element → render-object, wgpu. Но layout — **Taffy** (flexbox/grid), т. е. Flutter-протокол constraints-down/sizes-up, intrinsics, baselines и slivers не воспроизводится. FLUI — единственный активный порт, лояльный box/sliver-протоколу |
| **Frui** | PoC, спорадическая разработка, требует **nightly** (`#![feature(min_specialization)]`) | Иллюстрация к §4A: Flutter-порт на nightly-фичах остался PoC. `min_specialization` при этом признан unsound (§5) |
| **rinf**, **flutter_rust_bridge** | Активны, в обзоре — «✅ OK» по IME и a11y | Это *конкурирующая стратегия*: настоящий Flutter (Dart VM, Skia/Impeller) + Rust-логика. IME и accessibility «бесплатно», цена — Flutter SDK и Dart в поставке. FLUI обязан явно отвечать в README, почему pure-Rust с той же ментальной моделью лучше этого пути |

**Новые или заметно изменившиеся за 2025–2026:**

| Проект | Что произошло | Оценка обзора |
|---|---|---|
| **Freya** 0.4 (2026-07-16), 0.5-rc.4 (2026-08-23) | **Ушёл с Dioxus** на собственное реактивное ядро; Skia; layout **Torin** (`Content::wrap`); hot reload через **Subsecond** и `dx`; **Claude Code skill** в репозитории; `llms.txt` | IME ✅, screen reader ❌ |
| **Blinc** (первый релиз — начало 2026) | wgpu, реактивная модель, быстрая итерация (примеры в доках уже устарели) | `TextInput` «unfinished», composer не выровнен, CJK-шрифт отсутствует, a11y ❌ |
| **Tessera** (с июля 2025, уже v2.5) | Immediate-mode + pluggable shaders; цель 3.0 — Material Design | На macOS не запускается (ошибки wgpu) |
| **Ribir** 0.4-alpha.60 (2026-03) | 60+ alpha за два года; текст — **swash-растеризация вместо тесселяции путей**, multi-tier atlas; **MCP-инструменты `start_app`/`stop_app`/`attach_app`** и нативный MCP-сервер (alpha.57) | Криптичный макро-синтаксис (`@ { pipe!($read(image).clone()) }`), a11y ❌, IME ✅ |
| **Vizia** 0.4 (2026-04-23) | Линзы → **сигналы**; CSS-переменные; RTL и fluent-локализация; Skia | Screen reader вызывает **crash** (stale a11y-данные при обновлении `Signal` из `on_edit`) |
| **Makepad** 1.0 (2025-05) → «2.0 AI Native» | Live-DSL **Splash**, «модели стримят UI в нативные Rust-приложения»; Robrix, Moly как flagship-приложения | Composer скрыт, a11y ❌, «poor documentation» |
| **gpui-component** 0.5.1 (2026-02) | 60+ виджетов поверх GPUI, Apache-2.0 | В **winners' circle** (добавлен 2026-08-25): IME ✅, a11y ✅ — но только через `text!`-макрос, обычный текст screen reader не видит |
| **Floem** (Lapce) | Стагнирует | IME ❌ («не могу даже переключить раскладку в поле»), a11y ❌ |
| **Cushy** | Последний релиз — 2024 | Самый лаконичный код в обзоре; первый, кто принимает `image::DynamicImage` напрямую; composer скрыт, a11y ❌ |
| **KAS** | Tracking issues на IME и a11y открыты с июня 2025 (реакция на обзор 2025) | IME ❌, a11y ❌ |
| **Rui** (автор Audulus) | Репозиторий оживился в 2025 после 3 лет | Нет виджета изображения; IME ❌, a11y ❌ |
| **Azul** 0.2, **Pane UI**, **Pax**, **Maycoon** | Не читает системные шрифты / не вставляет картинки в рантайме / не компилируется два года / deprecated | Выбыли |

**Вердикт обзора (2026-08-23):** winners' circle — **Slint, egui, GPUI Component**. «Just short» — Cushy, Freya, Floem, Iced, Relm4, Xilem: хорошие API, но провал по IME или accessibility. Отдельно про Masonry: «если мне когда-нибудь придётся писать свою GUI-библиотеку — начал бы с Masonry: IME и accessibility из коробки».

**Что это значит для FLUI (сверено с кодом):**

1. **Планка «пригодности» в 2026 — не количество виджетов, а IME composer + screen reader.** Это единственный критерий, по которому обзор делит 50 библиотек на две группы. У FLUI есть всё для верхней группы — `flui-semantics`, feature `a11y` с адаптерами `accesskit_unix`/`_windows`/`_macos`, IME с composing-регионом (511 упоминаний) — но **`a11y` не в default-features**. egui попал в winners именно потому, что у него a11y включён по умолчанию; GPUI Component — потому что через `text!`. Предложение: (а) включить `a11y` по умолчанию в фасаде `flui` (или обосновать в ADR, почему нет); (б) добавить в `flui-testing`/live-smoke **acceptance-сценарий обзора** — `TextField` + `Image` из `image::DynamicImage`, проверяемый на: выровненный composer, CJK-текст с системным fallback-шрифтом (egui и Xilem получили 🟡 именно за CJK), чтение содержимого окна screen reader'ом через accesskit-дерево. Это дёшево и ровно тот тест, которым рынок будет мерить FLUI.
2. **Агентные affordances стали нормой у соседей**, а не только у больших (§5b п. 3): Freya — Claude Code skill + `llms.txt`, Ribir — MCP-инструменты жизненного цикла, Makepad — «AI-native» как позиционирование. У FLUI `AGENTS.md` — *внутренний* документ для контрибьюторов; для *потребителей* фреймворка нет ни `llms.txt`, ни skill'а. Это час работы поверх уже существующих README/docs.
3. **Layout как отличие.** Vexo, GPUI, Dioxus/Blitz, Bevy UI — все на Taffy (CSS flexbox/grid). FLUI — единственный с полноценным Flutter box/sliver-протоколом (intrinsics, baselines, `RenderSliver`, `performResize`). Это следует явно назвать в README как *причину* выбирать FLUI, а не только как деталь реализации: у Taffy нет slivers и intrinsic-запросов, а у Flutter-разработчика, который приходит в Rust, это главное ожидание.
4. **Ответ на «почему не Flutter + Rust».** rinf/flutter_rust_bridge получили ✅/✅ в обзоре бесплатно. Позиция FLUI — та же ментальная модель без Dart VM, Flutter SDK и второго тулчейна, с `cargo build` как единственной сборкой и `wgpu` как единым рендером — должна быть сформулирована в README одним абзацем; сейчас её нет.
5. **Что не делать:** не гнаться за Skia (Freya, Vizia, Slint-опция) — wgpu-консенсус подтверждён (§5b п. 2); не вводить DSL (Slint, Makepad Splash) — противоречит Prime Directive; не строить собственное реактивное ядро «как Freya» — three-tree reconciliation уже есть и лояльна Flutter.

### 5d. Где быть лучше Flutter: карта контрактов (вдохновение, а не порт)

Позиция «Flutter — источник вдохновения, но решения там 2014–2018 годов и мы делаем лучше с учётом рынка 2026» уже частично закодирована в репозитории: ADR-0008 (field-granular inherited, «Flutter's `InheritedModel`, but type-checked»), ADR-0027 (realms), ADR-0018/21/30/37 (capability-scoped `BuildContext`), damage-tracking в `flui-layer`, `Result` вместо исключений. До 2026-09-03 она была размазана по 47 ADR и **противоречила букве `AGENTS.md`**: правило №1 говорило «ported 1:1» и «"Make the core better" reverts to Flutter semantics». В этот день формулировка переписана в `AGENTS.md` (Prime Directive №1–3, §Flutter as Reference, Definition of Done), `STRATEGY.md`, `docs/PORT.md` §Mapping rules, `docs/FOUNDATIONS.md`, `STYLE.md` §2 и цитатах в `ARCHITECTURE.md` крейтов: Flutter — референс и оракул (нижняя граница поведения + тестовый корпус), не потолок; улучшения ожидаются в функционале, архитектуре и стиле кода, при условии ledger'а — что лучше и почему (ADR / `## Mapping decisions`), какой Flutter-тест заменён каким тестом FLUI, какие edge cases сохранены. Ниже — какие контракты действительно стоит превзойти, какие — нет, и почему.

**A. Где лояльность и есть лучшее решение — не трогать.** Критерий: контракт не устарел (никто на рынке не предложил лучшего) **и** за ним стоит оракул — ~3000 тестов `rendering/`+`widgets/`, 105 файлов которых уже портированы. Отказ от контракта = потеря оракула без замены.

| Контракт | Почему он не устарел |
|---|---|
| Box-протокол (constraints ↓, size ↑, один проход, intrinsics, baselines, relayout boundaries) | Jetpack Compose (2021) спроектировал layout по той же модели; Taffy — это CSS, шаг назад для приложений (§5c п. 3) |
| Sliver-протокол | Единственный в индустрии формальный протокол для виртуализированной прокрутки с pinned/floating/overlap; аналогов нет ни в Compose, ни в SwiftUI |
| Hit-test с `HitTestResult` и трансформами; gesture arena (semantics разрешения конфликтов) | Проблема арены решена правильно; менять надо *предсказание* (velocity, resampling), не разрешение конфликтов |
| Dirty-marking (`markNeedsLayout`/`markNeedsPaint`, repaint boundaries) | Минимальная корректная модель инвалидации retained-дерева; сигналы её обходят, а не улучшают (ADR-0008 §rejected) |
| Focus traversal, `Directionality`, semantics как отдельное дерево | Контракты a11y и RTL прошли 10 лет продакшна; AccessKit — новый *транспорт*, не новая модель |

**B. Где сам Flutter от контракта отказался или страдает — здесь «лучше» определено и проверено рынком.**

| Контракт Flutter | Что с ним не так (по Flutter же) | Лучшее решение 2026 | Статус в FLUI |
|---|---|---|---|
| Один UI-isolate: build/layout/paint строго последовательно на одном потоке | Ограничение Dart, не архитектуры; Flutter не может параллелить layout | Layout независимых relayout-boundaries параллельно: Rust доказывает `Send`/`Sync` на компиляции — единственный фреймворк, где это безопасно. Целевой профиль — деревья с десятками независимых boundaries (dashboard, split-view) | `rayon` — 0 упоминаний; raster-thread есть (ADR-0002). **Исследовать с измерением** (§8a #7 даёт baseline) |
| Skia raster cache + runtime shader compilation | Impeller выбросил raster cache и компилирует шейдеры заранее — jank первого кадра был главной жалобой 5 лет | Precompiled pipelines + persist `PipelineCache`; partial repaint через damage-регионы, отдаваемые в present (Wayland `wp_presentation`/Vulkan incremental present) | Damage-tracking в `flui-layer` есть (232 упоминания); **не проверено**, доходит ли damage до present и до scissor в wgpu (§8a #8) |
| Partial repaint только на iOS/macOS Metal | Никогда не был доделан для остальных платформ | Платформо-независимый damage → scissor + present-region | см. выше |
| `InheritedWidget` всё-или-ничего; `InheritedModel` со строковыми aspects; read ≠ depend | Flutter добавил `InheritedModel` как патч, `select`-паттерн живёт в provider/riverpod | Field-mask typed aspects, read == depend | **Сделано** (ADR-0008) |
| `BuildContext` как god-object | Любая capability доступна из любой фазы → rebuild-циклы, re-entrancy | Capability-scoped context, lifecycle-only ручки | **Сделано** (ADR-0018/21/30/37, trigger #22) |
| Исключения → `ErrorWidget.builder` / `FlutterError.onError` | Паника в build красит поддерево серым; в release — красным экраном | `Result` в API + error boundary как явный виджет с типизированной ошибкой; матрица «паника в фазе X → кадр N+1 живой» | `catch_unwind` 278, `ErrorWidget` 13; **контракт не задокументирован** (§8a #3) |
| `AnimationController` + `Curve` как основа; `SpringSimulation` — опция | SwiftUI (iOS 17) сделал spring дефолтом, Compose — `animate*AsState` с retarget; кривые по времени плохо прерываются | Physics-first: прерываемые, ретаргетируемые анимации с сохранением скорости как дефолт; curves — частный случай | `Spring` 188, `interrupt|retarget` 186 — механика есть; **проверить, что именно дефолт** в `AnimatedFoo`-виджетах |
| `WidgetSpan` через placeholders; `TextPainter` без inline boxes | Baseline-выравнивание inline-виджетов — известная боль; нет hinting/COLR | `parley` inline boxes + `harfrust` + `fontique` (§6b) | ADR запланирован |
| `const` виджеты как оптимизация rebuild | Хак компилятора Dart; забыть `const` = лишний rebuild | Статически известные поддеревья с нулевой стоимостью реконсиляции (тип виджета без состояния и без замыканий → `'static`, сравнение по указателю) | **Не исследовано** |
| Stateful hot reload как фича Dart VM | Недостижимо для native Rust напрямую | `subsecond`-стиль hot-patching функций (Dioxus/Freya) поверх существующего dlopen-пути | `flui-hot-reload` на dlopen; §5b п. 6 |
| Frame pipeline: vsync → build → … → present, без late-latch ввода | `PointerEventResampler` добавлен в 2020 как патч; латентность ввода — постоянная жалоба | Frame pacing с бюджетом и поздним сэмплом ввода; VRR-aware scheduler | Resampler есть (259); pacing — leapfrog-зона ADR-0027, **не начато** |
| DevTools как отдельный Dart-инструмент | Не доступен агентам/CI | Inspection-протокол + MCP (§5b п. 3) | все части есть, протокола нет |

**C. Территория без контракта Flutter** — multi-window, realms, concurrency topology, presentation — уже санкционирована ADR-0027; сюда же логично отнести frame pacing и parallel layout из таблицы B.

**Как это оформить.** Один документ `docs/BEYOND-FLUTTER.md` (или раздел в `STRATEGY.md`) с тремя колонками из таблицы B: контракт Flutter → задокументированная проблема (ссылка на Flutter issue/design doc) → решение FLUI → **чем заменён оракул** (какие Flutter-тесты перестают применяться и какой тест FLUI занимает их место). Это одновременно (1) маркетинговый ответ «почему FLUI, а не клон» для README, (2) защита от агентов, откатывающих улучшения к Flutter-семантике, и (3) честный учёт того, что при каждой дивергенции теряется. Параллельно поправить формулировку правила №1 в `AGENTS.md`: «loyal to behavior» — про *наблюдаемый результат* для пользователя (layout, порядок событий, edge cases), а не про внутренние механизмы; «better» разрешено везде, где есть ADR с заменой оракула.

**Следствие для лицензии (§8a #2):** позиция «вдохновение» снижает юридическую экспозицию, но не отменяет `NOTICE` — 72 файла буквально написаны как «Ported from …test.dart», и это правильно: портированные тесты — лучший оракул, и их не надо переписывать ради формы. Нужно лишь, чтобы самоописание было согласованным: не «1:1 port» в `AGENTS.md` и «просто вдохновились» в разговоре с юристом, а одно и то же везде — «behavioral reference with documented divergences».

---

## 6. Что stable уже даёт, а workspace не использует (без nightly, в пределах MSRV 1.97)

Grep по `crates/**/*.rs` — 0 использований у каждого:

- `cfg_select!` (stable 1.95) — для цепочек `cfg(target_os = …)` в `flui-platform`/`flui-log` там, где сейчас if/else-каскады `cfg_attr`.
- `core::hint::cold_path` (1.95) — маркировка холодных веток в layout/hit-test (`expect("BUG: …")`-ветки).
- `fmt::from_fn` (1.93) — компактные `Debug` impl'ы под `missing_debug_implementations = "warn"`.
- `<[T]>::get_disjoint_mut` (1.86) — там, где арена хранит `Vec`-slab и нужны два `&mut` на разные индексы без raw-pointer кода (применимость к `storage/tree.rs` надо проверять по месту — если хранилище не slice-backed, не подходит).
- `Atomic::try_update` (1.95) — п. 2 из §1.
- Cargo 1.97: `build.warnings` — уже используется (`CARGO_BUILD_WARNINGS`), это правильно.

### 6a. Грамотные stable-решения, которые можно принять сейчас (проверено по workspace)

Всё ниже — stable ≤ 1.97, без новых контрактов на nightly. Отсортировано по соотношению «польза / риск». Цифры — `grep` по `crates/**/*.rs` и манифестам на 2026-09-03.

| # | Решение | Факт из workspace | Почему грамотно |
|---|---|---|---|
| 1 | **`resolver = "3"` в `[workspace]`** (MSRV-aware resolver, stable 1.84) | Сейчас `resolver = "2"` при `edition = "2024"` | Resolver 3 при `cargo update`/`cargo add` учитывает `rust-version = "1.97"` и не подтягивает версии зависимостей с более высоким MSRV → `msrv` CI job перестаёт быть единственной защитой от «сломали MSRV бампом зависимости». Edition 2024 делает это дефолтом для *пакетов*, но для **виртуального workspace** нужно указать явно — cargo как раз об этом предупреждает |
| 2 | **`fetch_update` → `try_update`** | 13 вхождений (включая тесты) | Уже в §1; deprecated с 1.99 |
| 3 | **`#[expect(...)]` вместо `#[allow(...)]`** (stable 1.81) + `clippy::allow_attributes` / `allow_attributes_without_reason` в `[workspace.lints.clippy]` | `#[allow(` — 542, `#[expect(` — 50 | `expect` падает с `unfulfilled_lint_expectations`, когда подавленный линт перестал срабатывать → «мёртвые» allow не накапливаются. С таким объёмом `allow` это лучший способ узнать, сколько из них уже не нужны. Делать по крейту, начиная с `flui-foundation` |
| 4 | **`#[diagnostic::on_unimplemented]`** (1.78) и **`#[diagnostic::do_not_recommend]`** (1.85) на публичных трейтах фреймворка (`View`, `ViewState`, `RenderObject`/`RenderBox`, Arity-трейты, `ParentData`) | 0 использований | Это DX-фича *для потребителей фреймворка*: вместо трёх экранов «the trait bound … is not satisfied» — «`Foo` is not a View; did you forget `impl View for Foo`?». Для проекта, который позиционирует себя как Flutter-для-Rust, это заметнее любой оптимизации. Нулевой риск: атрибуты влияют только на текст ошибок |
| 5 | **Удалить устаревший комментарий про lld в `.cargo/config.toml`**, оставить только mold как опцию | `rustc --print target-spec-json` для `x86_64-unknown-linux-gnu`: `linker-flavor = gnu-lld-cc`, self-contained | С 1.90 `rust-lld` — **линкер по умолчанию** на Linux x86_64, блок «uncomment lld» ничего не даёт. Единственный оставшийся апгрейд — mold (`clang -fuse-ld=mold`), и его стоит измерить на `flui` с `enable-wgpu-tests` перед тем как рекомендовать |
| 6 | **`.config/nextest.toml`** (профиль `ci`) | Файла нет; nextest — основной раннер и локально, и в CI | `slow-timeout = { period = "60s", terminate-after = 3 }` (зависший GPU/xvfb тест сегодня висит до `timeout-minutes` job'а), `leak-timeout`, `fail-fast = false` в `ci`, `junit` для артефактов, `[[profile.ci.overrides]] filter = 'test(/gpu|wgpu/)' threads-required = N` — чтобы GPU-тесты не толкались за один адаптер. Это конфигурация, не код, и она напрямую отвечает на «Test flake» пункт из `AGENTS.md` |
| 7 | **`cfg_select!` (1.95)** в `flui-platform`/`flui-log` | 0 использований при 17 `std::env::var` и множестве `cfg(target_os)`-каскадов | Один `match`-подобный блок вместо цепочки `#[cfg]`/`#[cfg(not(any(...)))]` — исчезает класс ошибок «забыли ветку для новой ОС», который `unexpected_cfgs = "deny"` не ловит |
| 8 | **`core::hint::cold_path()` / `#[cold]`** на `expect("BUG: …")`-ветках в layout/paint/hit-test | `#[cold]` — 4, `cold_path` — 0, при 275 `debug_assert!` и 58 `unreachable!` | Дешёвая подсказка LLVM для веток, которые по `PANIC-POLICY.md` не должны исполняться; влияет на размещение кода горячих циклов. Только с измерением (criterion в `flui-rendering`) — иначе это карго-культ |
| 9 | **`fmt::from_fn` (1.93)** | 0; `missing_debug_implementations = "warn"` уже включён | Убирает boilerplate `struct DebugHelper; impl Debug for …` там, где `Debug` для арен/слабов пишется вручную |
| 10 | **`cargo-machete`/`cargo-shear` в `just ci`** — временно, до stable cargo-lints (1.100) | На nightly уже найдены `serde_json` в `flui-devtools` и 3 неиспользуемых `[workspace.dependencies]` | Stable-инструмент, который ловит то же самое сегодня; убрать, когда `[lints.cargo] unused_dependencies` приедет в stable |
| 11 | **`cargo-hakari` (workspace-hack)** — только по измерению | CI и `justfile` гоняют много `-p <crate>` и `cargo hack --each-feature` — каждый набор features пересобирает зависимости заново | Устраняет пересборки deps из-за feature-унификации между `-p` инвокациями. Выигрыш зависит от графа; измерить на `just test-crate` × 3 крейта до/после |

**Что не рекомендуется, хотя выглядит заманчиво:**

- Заменить `static_assertions` на `const { assert!(…) }`: из 40 использований 32 — `assert_not_impl_any!` (проверка *отсутствия* `Send`/`Sync`), которой у std нет. Крейт остаётся.
- `-C target-cpu=native` в `.cargo/config.toml`: ломает переносимость бинарей и кеш sccache между машинами.
- `split-debuginfo = "unpacked"` для dev: при `debug = "line-tables-only"` выигрыш минимален; на macOS это уже дефолт.
- `codegen-units = 1` / `lto = "fat"` для dev/test: замедлит `just ci` без пользы для корректности.

Что *уже* сделано правильно и трогать не надо: `LazyLock`/`OnceLock` вместо `once_cell`/`lazy_static` (0 зависимостей на них), `is_some_and`/`is_none_or` (207), strict-provenance API (`.addr()` — 104), `#[must_use]` (3453), `unsafe_op_in_unsafe_fn = "deny"`, `unexpected_cfgs = "deny"`, `CARGO_BUILD_WARNINGS`, `debug = "line-tables-only"` + `[profile.dev.package."*"] opt-level = 2`, `strip = "debuginfo"` в release, `cargo-deny` в PR и weekly.

### 6b. Зависимости: что заменить, что оставить (аудит графа на 2026-09-03)

Метод: `[workspace.dependencies]` + прямые зависимости 28 крейтов, `Cargo.lock` (697 пакетов), `cargo tree -d` (дубликаты), `cargo info` (актуальные версии на crates.io), места использования по `grep`. Замена рекомендуется только там, где есть один из трёх поводов: библиотека **deprecated**, библиотека **не подходит по семантике** к тому, что портируем из Flutter, или она **дублирует** другую уже имеющуюся. «Более быстрая» без измерения — не повод.

#### Заменить (обоснование фактическое, не вкусовое)

| Сейчас | На что | Где | Почему |
|---|---|---|---|
| **`cocoa` 0.26 + `objc` 0.2** | **`objc2` 0.6 + `objc2-app-kit` 0.3 + `objc2-foundation` 0.3** | `flui-platform/src/platforms/macos/*` — 6 файлов, 164 `msg_send!` | `cocoa` **официально deprecated** с мая 2025 ([servo/core-foundation-rs#729](https://github.com/servo/core-foundation-rs/issues/729), README: «deprecated in favour of the objc2 crates»). `objc2`/`objc2-app-kit` **уже в графе** (через `winit`, `arboard`, `accesskit`) — сейчас линкуются две несовместимые обвязки ObjC-рантайма. `objc2` даёт `Retained<T>` вместо сырых `id`, `MainThreadMarker`, `define_class!`, типизированные сигнатуры — то есть закрывает целый класс UB в единственном backend'е, который CI только компилирует (`cross-typecheck`), но не запускает. Честная оговорка: без macOS-раннера миграция верифицируется только компиляцией; нужен один ручной smoke на Mac |
| `cosmic-text` 0.19 + `glyphon` 0.12 | **`parley` 0.11** (+ `fontique`, `skrifa`, `swash`) — как **spike под ADR**, не как замена «сейчас» | `flui-painting` (cosmic-text), `flui-engine` (glyphon, optional) | Семантическое несовпадение с Flutter: `Paragraph` во Flutter — дерево `TextSpan`/`WidgetSpan` с **inline placeholders** (`PlaceholderSpan`, `PlaceholderDimensions`), bidi, locale-aware word segmentation. Это ровно `parley::TreeBuilder` + `InlineBox` + ICU4X; у cosmic-text inline boxes нет, а его editor-модель — «один большой буфер» (терминал/редактор), не «много `TextField`». Bevy по этим же причинам мигрирует ([bevy#21765](https://github.com/bevyengine/bevy/issues/21765)); у parley есть first-party интеграция с `accesskit` (у нас 121 использование). Цена: `glyphon` — это ещё и glyph-atlas рендерер, его придётся заменить своим атласом или `vello`. Это ADR-решение уровня «presentation architecture» — санкционированная leapfrog-зона по ADR-0027 |
| `lasso::Rodeo` за `RwLock` | `lasso::ThreadedRodeo` (тот же крейт, feature `multi-threaded` уже включён) | `flui-assets/src/types/key.rs:12` | Глобальный `RwLock<Rodeo>` — это ambient-reach, который `docs/runtime-contract.toml` уже ратчетит; `ThreadedRodeo` убирает лок, ничего не добавляя в граф |

#### Обновить (рутинные бампы, версии на crates.io на 2026-09-03)

| Крейт | В workspace | Актуально | Замечание |
|---|---|---|---|
| `accesskit` / `accesskit_unix` | 0.24 / 0.22 | 0.25 / 0.23 | Проверить совместимость с `accesskit_winit` |
| `winit` | 0.30.13 | 0.31.0-beta.2 | **Ждать stable 0.31.** Он же уберёт дубликаты `smol_str 0.2`, `thiserror 1` (через smithay), `rustix 0.38`. winit у нас fallback — не приоритет |
| `wgpu` | 30.0.1 | 30.0.1 | Актуально |
| `bon` | 3.8 | 3.9 | Рутина |
| `tokio` | floor 1.43 | 1.53 | Floor честный; `-Zdirect-minimal-versions` (§4C) это подтвердит или опровергнет |

#### Дубликаты в графе (`cargo tree -d`)

`skrifa` 0.40/0.44 и `read-fonts` 0.37/0.41 (cosmic-text vs swash), `hashbrown` 0.14/0.16/0.17 (dashmap+lasso / gpu-allocator / wgpu), `rustc-hash` 1/2 (naga/wgpu-core vs наш код), `smol_str` 0.2/0.3 (winit vs cosmic-text и `flui-semantics`), `thiserror` 1/2, `syn` 2/3, `rustix` 0.38/1, `getrandom` 0.3/0.4, `downcast-rs` 1/2 (wayland-backend), `codespan-reporting` 0.12/0.13 (naga_oil vs naga). Почти все — upstream-driven (winit 0.30, cosmic-text, naga_oil), а не наши. Действие: не «чинить» патчами, а добавить `cargo tree -d -e normal` в weekly-job как наблюдаемую метрику и снимать дубликаты бампами upstream'а (winit 0.31, cosmic-text/parley, naga_oil под wgpu 30).

#### Оставить (проверено, замена не оправдана)

| Крейт | Соблазн | Почему нет |
|---|---|---|
| `dashmap` 6.2 | `papaya` (lock-free reads, [benchmarks](https://github.com/ibraheemdev/papaya/blob/master/BENCHMARKS.md)) | Все 7 `DashMap` — крошечные карты по `PointerId`/`CallbackId` в `flui-interaction`/`flui-scheduler`, с realm-scoped владением и без read-heavy контенции; papaya выигрывает на тысячах читателей, а здесь скорее уместен вопрос «нужен ли вообще concurrent map, если владелец — один realm» (`Mutex<FxHashMap>`) — это вопрос дизайна, не библиотеки |
| `lru` 0.18 + `moka` 0.12 | Свести к одному (`quick_cache`) | `lru` — одно место (`flui-widgets` decode cache под `Mutex`), `moka::future` — `flui-assets`. Разные семантики (sync LRU vs async TinyLFU с TTL). Консолидация возможна, но выигрыш не измерен → отложить, пока asset-pipeline не стабилизируется |
| `parking_lot` 0.12 | std `Mutex` (futex с 1.62) | std не даёт `Mutex` без poisoning на stable (`nonpoison` — nightly), а 309 использований `parking_lot::` опираются именно на это + `MappedMutexGuard`. Оставить |
| `lyon` 1.0 + `kurbo` 0.13 | `vello_hybrid`/`vello_cpu` 0.2 (sparse strips, без тесселяции) | Это замена **растеризатора**, не библиотеки. Обоснованный spike в той же ADR, что и parley (vello — первый потребитель parley); до него lyon остаётся |
| `rustc-hash` 2 | `foldhash` (дефолт hashbrown 0.15+) | Fx оптимален для ключей-целых (`NonZeroUsize` ID — наш основной случай); foldhash лучше для строк. Смешивать ради строк — нет |
| `glam` 0.33, `bytemuck`, `bitflags` 2, `smallvec` 1, `slab` 0.4, `image` 0.25 (`zune-jpeg` внутри), `reqwest` 0.13/rustls, `thiserror` 2, `tracing`, `criterion` 0.8, `insta`, `proptest` | — | Рыночный стандарт своего класса; замена ничего не даёт |
| `static_assertions` | `const { assert!() }` | 32 из 40 — `assert_not_impl_any!`, аналога в std нет |
| `crossbeam-channel` 0.5 | `std::sync::mpsc::sync_channel` | **Снято при исполнении (2026-09-03).** Первичный список пропустил два сайта — `flui-rendering/pipeline/handle.rs` (`PipelineOwnerHandle`) и `pipeline/owner/mod.rs` — и, главное, все пять сайтов опираются на `Sender::len()`/`Receiver::len()`: для `Debug`, для `pending()`-метрик и для ограничения drain-цикла в точке коммита (`try_iter` — не снимок; см. комментарий в `ui_realm.rs` у `self.rx.len()`). У std-`mpsc` `len()` нет; замена потребовала бы собственный атомарный счётчик поверх канала — больше кода и новый инвариант ради минус одной хорошо поддерживаемой зависимости. Пересмотреть, когда `std::sync::mpmc` стабилизируется с `len()` |
| `regex` 1 в `flui-engine/build.rs` | `regex-lite` | **Снято при исполнении (2026-09-03).** Тип `regex::Regex` диктуется API `wgsl_bindgen` (`OverrideTextureFilterability`/`OverrideSamplerType`/`add_custom_padding_field_regexp` принимают именно его — см. комментарий у `regex` в корневом `Cargo.toml`), и `regex` уже сидит в графе `wgsl_bindgen`. Замена невозможна без смены bindgen'а |

---

## 7. План действий (всё на stable)

| Когда | Что | Зачем |
|---|---|---|
| до **2026-10-01** | `fetch_update` → `try_update` (8 сайтов, §2); обновить устаревший комментарий в miri job (`ci.yml`) | Иначе PR-CI красный с релизом 1.99 |
| до **2026-10-01** | Бамп dev-пина на `1.99.0` по процедуре из `rust-toolchain.toml` | Политика `docs/PORT.md` |
| ближайший спринт | `recursion_depth_exceeding_limit`: `unsafe impl Send/Sync` для промежуточных обёрток над `wgpu::Renderer` (предпочтительно) или `#![recursion_limit = "256"]` в `flui-engine`, `flui-app` и 4 примерах | Станет hard error со стабилизацией нового solver'а |
| ближайший спринт | `[lints] workspace = true` в `examples/painting_demo`, `examples/web_demo`; `serde_json` в `flui-devtools` → `optional` + в `timeline`; `dyn-clone`/`downcast-rs`/`flui-testing` — `workspace = true` в членах или удалить из корня | `STYLE.md` §2; гигиена |
| ближайший спринт | `weekly.yml`: job `nightly-canary` (§4C); `justfile`: `nightly-check` | Ранее предупреждение, датированное |
| ближайший спринт | `resolver = "3"`; `.config/nextest.toml` с профилем `ci`; убрать устаревший lld-комментарий из `.cargo/config.toml` (§6a #1, #5, #6) | Конфигурация без изменения кода; MSRV-aware resolver и таймауты зависших тестов |
| следующие 2–3 спринта | `#[allow]` → `#[expect]` покрейтно + `clippy::allow_attributes`; `#[diagnostic::on_unimplemented]` на `View`/`RenderObject`/Arity-трейтах (§6a #3, #4) | Чистка мёртвых подавлений; DX для потребителей фреймворка |
| следующие 2–3 спринта | `cocoa`/`objc` → `objc2`/`objc2-app-kit` в macOS backend; `Rodeo` → `ThreadedRodeo` (§6b). `crossbeam-channel` и `regex` — сняты, см. таблицу «Оставить» в §6b | Deprecated-зависимость; убрать глобальный лок с интернера |
| ADR (отдельно) | Spike: `parley` для `Paragraph`/`WidgetSpan`-паритета и `vello_hybrid` как растеризатор — один ADR на presentation architecture (§6b, §5b п. 1–2) | Санкционированная leapfrog-зона (ADR-0027); текущий text-stack не выражает inline placeholders, hinting и COLR; egui/Bevy/Xilem уже перешли |
| ADR (отдельно) | Inspection-протокол поверх `flui-semantics` + `HeadlessBinding` (чтение дерева, инъекция ввода, снимок) и `flui-mcp` как потребитель (§5b п. 3) | egui 0.35 и Slint 1.17 сделали это конвенцией за одно лето; у FLUI все части есть, нет только протокола |
| README / ROADMAP | Зафиксировать device-recovery + occlusion gating + live-smoke как отличие (§5b п. 4); абзацы «почему не Taffy» и «почему не Flutter + Rust» (§5c п. 3–4) | Bevy получил `RenderErrorHandler` только в 0.19; FLUI здесь впереди. Единственный лояльный box/sliver-порт — это причина выбора, а не деталь |
| ближайший спринт | Acceptance-сценарий обзора 2026 (`TextField` + `image::DynamicImage`; composer, CJK fallback, screen reader через accesskit) в `flui-testing`/live-smoke; решить про `a11y` в default-features фасада (§5c п. 1) | Именно этим критерием рынок делит фреймворки на «пригодные» и «почти» |
| ближайший спринт | `llms.txt` + agent skill для *потребителей* фреймворка поверх README/docs (§5c п. 2) | Freya, Ribir, Makepad, Dioxus уже дают это; час работы |
| еженедельно | `cargo tree -d -e normal` в weekly-job как метрика дубликатов (§6b) | Дубликаты снимать бампами upstream (winit 0.31, cosmic-text/parley), не патчами |
| еженедельно | Просматривать разделы *Calls for Testing* и *Final Comment Period* в [This Week in Rust](https://this-week-in-rust.org/) вместе с результатом канарейки (§5a) | Канарейка показывает, что сломалось; TWiR — что сломается через 6–12 недель |
| по желанию | В miri job добавить `-Zrandomize-layout`; отдельный weekly job `cargo +nightly -Zdirect-minimal-versions check` | Проверка `unsafe`-допущений о layout; честность floors зависимостей |
| когда стабилизируется | `[lints.cargo]` в корне; `hint-mostly-unused` для `windows-sys`/`web-sys`; `build-std` для wasm32 `+atomics`; `mpmc_channel` вместо `crossbeam-channel` | По мере появления в stable |
| никогда без ADR | `#![feature(...)]`, `channel = "nightly"`, `nightly` feature | `STYLE.md` §2 |

---

## 8. Что не подтверждено / открытые вопросы

- cargo-lints: merge зафиксирован в TWiR 667 (неделя до 2026-09-02), значит ожидается 1.100 — но в опубликованном changelog строки пока нет; подтвердить при бампе.
- `-Zembed-metadata=no` (default на nightly 1.100): реальный эффект на размер `target/` этого workspace не измерен.
- Можно ли использовать Tier-2 `x86_64-unknown-linux-gnuasan` со stable-компилятором без `-Zsanitizer` — по формулировке goal'а да, но не проверено на практике.
- Выигрыш Cranelift и `-Zthreads` на `cargo build`/`cargo test` (а не `check`) этого workspace — не измерялся; `check` показал 0 % для `-Zthreads`.
- Stable-baseline времени сборки не снят: локальный toolchain `1.98.0` был установлен частично (rustup при `toolchain list` попытался «recover from a partially installed toolchain» и не смог; `cargo check` падал с `can't find crate for std`, `cargo fmt` — с «`cargo-fmt` … is not applicable to the toolchain», из-за чего `just gate` и pre-push hook были красными). Починено 2026-09-03 через `rustup component remove rustfmt --toolchain 1.98.0 && rustup component add rustfmt rust-std --toolchain 1.98.0`; baseline всё ещё не снят (§8a #6).

---

## 8a. Что ещё стоит исследовать / какие аудиты провести дальше

Этот документ закрывает четыре вопроса: toolchain, stable-приёмы, зависимости, рынок. Ниже — то, чего он **не** касался и где у проекта либо нет данных вовсе, либо есть тревожный сигнал. Каждый пункт помечен: что это даст, чем измерять, и цена. Факты о репозитории проверены на 2026-09-03 (`grep`/`ls`, без изменения кода).

### Тир 1 — есть конкретный сигнал, что здесь проблема

| # | Аудит / research | Сигнал в репозитории | Метод | Цена |
|---|---|---|---|---|
| 1 | **Платформенная матрица CI: тесты идут только на Linux.** | `ci.yml`: test-матрица `os: [ubuntu-latest]`; комментарий на строке 191 «Re-add `windows-latest` … once» (снят из-за H9 `STATUS_HEAP_CORRUPTION`); **macOS не запускается вообще** — ни линковка, ни тесты, только `cargo clippy` под `aarch64-apple-darwin` в cross-typecheck. При этом `macos-latest` для публичных репозиториев на GitHub бесплатен. | (a) Добавить `macos-latest` в `test`-матрицу хотя бы для `flui-platform --all-features` + headless-набора; (b) H9 расследовать по-настоящему: Windows ASan (`-Zsanitizer=address` — это единственное место, где nightly в CI *обоснован*, см. §4) или Application Verifier / `_CrtSetDbgFlag` в отдельном canary-job на `windows-latest`. | Средняя. H9 — открытый crash в ROADMAP-TRACKER, который сейчас ничем не гейтится. |
| 2 | **Правовой аудит порта: атрибуция Flutter отсутствует.** | `LICENSE` (MIT, 1 KB) и `LICENSE-APACHE`; слово «flutter» не встречается ни в одном из них; файлов `NOTICE`/`THIRD-PARTY` нет. Flutter — BSD-3-Clause, а 105 файлов в `crates/` явно ссылаются на `.flutter/packages/flutter/test/` как источник портированных тестов. Перевод поведения — не копирование, но портированные *тесты* (константы, сценарии, ожидаемые значения) ближе к производной работе. | Юридический review одним пунктом: добавить `NOTICE` с BSD-3 текстом Flutter и перечнем крейтов, содержащих портированные тест-сценарии; решить, нужно ли упоминание в `Cargo.toml` `license = "MIT OR Apache-2.0"` (для крейтов с портированными тестами — возможно `AND BSD-3-Clause` только для test-таргетов). | Низкая по трудозатратам, **обязательна до публикации на crates.io** (`cargo info flui` показывает: не опубликован). |
| 3 | **Panic-поверхность против `docs/PANIC-POLICY.md`.** | `unwrap_used = "warn"` (не `deny`) в workspace-lints; 652 вхождения `.unwrap()` вне `tests/`/`benches/`/`examples/` (грубый grep — часть в inline `#[cfg(test)]`-модулях, но не все); `expect("BUG:` по соглашению из AGENTS.md — 212 вхождений (соглашение соблюдается; вопрос только в остатке `unwrap()`). `catch_unwind` — 278 упоминаний: есть error-boundary, но нет документа, какие фазы (build/layout/paint/raster) он покрывает и что рисуется вместо упавшего поддерева (`ErrorWidget` — 13 упоминаний; у Flutter это `ErrorWidget.builder` + `FlutterError.onError`). | (a) Перевести `unwrap_used` в `deny` покрейтно и посчитать реальный остаток через `cargo clippy --message-format=json`; (b) написать один тест-матрицу «паника в `build` / `perform_layout` / `paint` / raster-thread → приложение выживает, frame N+1 рисуется» и сверить с `.flutter/packages/flutter/test/widgets/framework_test.dart` (error-handling секции). | Средняя. Это прямой критерий «пригоден для продакшна». |
| 4 | **Concurrency-аудит raster-handoff и lock-порядка.** | ~400 `Arc<Mutex`, 309 упоминаний `parking_lot`; frame-pump ↔ raster-thread через rendezvous-каналы (`raster_backpressure` bench есть); `loom` — 0 упоминаний, `-Zsanitizer=thread` не запускался. Есть `miri` только для `pipeline::owner`. | (a) Задокументировать lock-ordering (какие мьютексы могут держаться одновременно) — одна страница в `flui-engine/ARCHITECTURE.md`; (b) `loom`-модель для протокола `raster_owner`/backpressure (маленькая, 2–3 потока); (c) TSan-прогон `flui-engine` + `flui-app` в nightly-canary (§4, единственная стабильная причина держать nightly). | Средняя–высокая. Deadlock/race в raster-handoff — класс ошибок, который live-smoke ловит только случайно. |
| 5 | **Тестовая сила, а не покрытие: mutation testing.** | AGENTS.md прямо называет «MVP reported as parity» и fake-passing главным риском; `just coverage` есть, но покрытие не измеряет, *ловят* ли тесты изменение поведения. | `cargo-mutants` на `flui-rendering` (`protocol/box_protocol.rs`, `sliver_protocol.rs`, `storage/tree.rs`) с `--in-diff` в CI для PR и полным прогоном в weekly. Выжившие мутанты = места, где harness проходит, а поведение не проверено. | Низкая на старте (один weekly-job), даёт прямой список слепых зон. |

### Тир 2 — данных нет, а решения на них опираются

| # | Аудит / research | Почему сейчас | Метод |
|---|---|---|---|
| 6 | **Baseline производительности сборки.** Ни одного числа: время `cargo build`/`test` cold/warm, доля sccache-hit, критический путь. Все выводы §3 про Cranelift/`-Zthreads` сделаны по `check`. | Без baseline любое «стало быстрее» — мнение. | `cargo build --timings` (HTML с критическим путём по крейтам), `sccache --show-stats` после `just ci`, размер `target/` до/после `-Zembed-metadata=no`. Записать в `docs/` как таблицу и обновлять при бампах. |
| 7 | **Runtime-перф против бюджета кадра.** Бенчи есть (`flui-rendering`: layout/paint/intrinsics; `flui-interaction`: arena/resampler/velocity; `flui-engine`: throughput/backpressure), но нет (а) сквозного «frame time на demo-дереве из N виджетов» и (б) аллокационного профиля кадра. | Flutter гарантирует 16.6 мс на реальных деревьях; у FLUI нет ни одной цифры про end-to-end кадр. | Criterion-бенч через `HeadlessBinding::mount_root` + `pump_frame` на `material`/`vertical-slice`; `dhat`/`heaptrack` для аллокаций за кадр в steady-state (цель — ноль аллокаций в paint при неизменном дереве); `tracing-tracy` (1 упоминание сейчас) как постоянный профилировщик под feature-flag. |
| 8 | **Shader-jank и pipeline cache.** `PipelineCache` — 29 упоминаний, значит есть; неизвестно, персистится ли на диск между запусками и покрывает ли все `RenderPipeline`, которые создаются лениво. | Это самая известная Flutter-боль (Skia shader compilation jank), которую Impeller решил precompile'ом; у wgpu есть `Device::create_pipeline_cache` (Vulkan/Metal). | Трассировка первого кадра: сколько pipelines создаётся на cold-start, сколько мс; проверить, есть ли warm-up всех вариантов до первого present. |
| 9 | **Цветовая модель.** 244 упоминания `srgb`/`linear` в движке — есть логика, нет одного документа «в каком пространстве блендим». Bevy 0.18 исправлял bloom из-за путаницы linear/sRGB; Flutter блендит в sRGB (Impeller — wide gamut на iOS). | Порт «лояльный поведению» должен дать тот же результат для `Color.lerp`, `Opacity`, теней и градиентов. | Golden-сравнение `Colors.lerp`/`ShaderMask`/`BoxShadow` с Flutter-эталоном (снимок с `flutter test --update-goldens` на референсном приложении); зафиксировать выбор в ARCHITECTURE.md. |
| 10 | **Startup / cold-start.** Не измерен: время от `main` до первого present, время `FONT_SYSTEM` инициализации (system fonts scan через fontdb — известное узкое место cosmic-text). | Для desktop-приложения 300 мс vs 1.5 с — заметная разница; при переходе на `parley`/`fontique` (§6b) это меняется. | `tracing` span'ы вокруг фаз старта, замер на трёх ОС. |

### Тир 3 — качественные исследования (документ, не CI)

| # | Research | Зачем |
|---|---|---|
| 11 | **Систематическая матрица паритета Widget/RenderObject с Flutter.** `RENDER_OBJECT_TYPES` гарантирует, что *реализованное* протестировано, но не показывает, что *не реализовано*. Скрипт: список классов в `.flutter/packages/flutter/lib/src/{rendering,widgets}/` → есть ли порт → есть ли портированный тест. | Даёт честную карту «done vs deferred» — ровно то, что требует Definition of Done. Результат — таблица в `docs/`, обновляемая скриптом. |
| 12 | **Порт тестового набора Flutter как оракула.** 105 файлов уже ссылаются на `.flutter/…/test/`; нет подсчёта — сколько из ~3000 тестов `rendering/` и `widgets/` перенесено. | Flutter test suite — самая дешёвая проверка поведения из существующих. Метрика «% портированных тестов по файлу» должна быть в ROADMAP-TRACKER. |
| 13 | **Threat model для трёх внешних границ.** (a) `flui-hot-reload` грузит `.so` через dlopen — любой файл в watch-директории исполняется; (b) `flui-assets` + `reqwest` для network images: лимиты `image::Limits` (49 упоминаний `Limits` — проверить, что это они), decompression bomb, редиректы, размер ответа; (c) asset path traversal. | Один документ `docs/SECURITY.md` + `cargo-fuzz` таргеты на декодеры входа (asset manifest, `.flui` конфиги, поток pointer-событий в gesture arena — state machine с 0 fuzz-покрытием сейчас). `fuzz/` в репозитории отсутствует. |
| 14 | **Доступность руками.** AccessKit-адаптеры есть; ни один CI не может запустить NVDA/Orca/VoiceOver. | Чек-лист ручной проверки (5 сценариев из §5c: TextField+composer, CJK, список, диалог, фокус) по одному разу на ОС перед каждым релизом; результаты — в CHANGELOG. |
| 15 | **API-surface перед первой публикацией.** `cargo-semver-checks` в репозитории отсутствует (0 упоминаний в `Cargo.toml`/`justfile`/workflows); `cargo-public-api` для снимка публичной поверхности `flui` фасада; `#[non_exhaustive]` на enum'ах событий/ошибок; sealed-трейты для `RenderObject`-семейства. | После 0.3 на crates.io каждая правка сигнатуры — breaking. Дешевле один раз пройти `api-review` до публикации. |
| 16 | **Wasm-размер и wasm-специфика.** `wasm-check` компилирует, но размер `.wasm` для `web_demo` не измерен; `twiggy`/`wasm-opt`; `getrandom`/`web-time` уже учтены. | Размер — первое, что смотрят при выборе web-фреймворка на Rust (Leptos/Dioxus публикуют цифры). |
| 17 | **Docs-покрытие и docs.rs-сборка.** `missing_docs = warn`; `cargo doc --cfg docsrs` под `RUSTDOCFLAGS=-D warnings` в CI есть, но покрытие (`rustdoc -Zunstable-options --show-coverage`, только nightly) — не снималось. | Вторая легитимная задача для nightly-canary: число, а не ощущение. |

### Подробнее про #2: лицензия и упоминание Flutter

Не юридическая консультация; ниже — стандартная практика Rust-экосистемы и текст самих лицензий.

**Факты (2026-09-03).** FLUI: `license = "MIT OR Apache-2.0"` во всех 30 манифестах, `LICENSE` (MIT, © 2025 vanyastaff) + `LICENSE-APACHE`, README §License. Flutter: BSD-3-Clause, «Copyright 2014 The Flutter Authors». В `crates/` 72 файла (50 из них в `src/`, не в `tests/`) содержат явную ссылку вида «Ported from `.flutter/…/key_test.dart`». Выборочная проверка четырёх характерных dartdoc-фраз дала 0 совпадений — прозу документации не копировали. Ссылок на `.gpui/` в коде нет.

**Менять ли лицензию — нет.** BSD-3 — permissive и не «заразна»: она не требует, чтобы производная работа была под BSD. MIT/Apache-2.0 совместимы с ней в обе стороны. Прецеденты в экосистеме: `kurbo` (Linebender) портирует алгоритмы Skia и остаётся `Apache-2.0 OR MIT` с атрибуцией по месту; `tiny-skia` — почти дословный порт Skia — наоборот, взял BSD-3 и держит два copyright в одном `LICENSE` (Google 2011 + автор 2020). FLUI по методологии (`docs/PORT.md`: «read for what and why, do not transcribe», Rust-native структура) ближе к `kurbo`. Дополнительный аргумент оставить Apache-2.0 — явный патентный грант, которого нет ни у MIT, ни у BSD.

**Является ли FLUI производной работой — серая зона, и от неё не надо зависеть.** Авторское право защищает выражение, а не идеи, алгоритмы, поведение или API-имена (`MainAxisAlignment`, `performLayout` — не защищаемы; Google v. Oracle к тому же признал реимплементацию API fair use). Перевод кода на другой язык строка-в-строку — наоборот, классическая производная работа (перевод — исключительное право автора). FLUI между этими полюсами, а **портированные тесты — ближе всего к переводу**: те же сценарии, константы, ожидаемые значения, и 72 файла сами это декларируют. Раз стоимость выполнения условий BSD-3 — один файл, а цена спора ненулевая, условия выполняем независимо от того, «обязаны» ли.

**Что именно требует BSD-3.** (1) В исходниках — сохранить copyright notice, список условий и disclaimer; (2) в бинарной поставке — воспроизвести их «в документации или сопутствующих материалах»; (3) не использовать имена Google/контрибьюторов для продвижения. Пункт (1) закрывается файлом; (2) — тем, что файл уезжает в `.crate`; (3) — формулировками в README.

**Товарный знак.** «Flutter» — знак Google. Описательное употребление («Flutter-inspired», «порт протокола layout Flutter») — nominative fair use, допустимо и уже так в README. Нельзя: имя проекта с «Flutter» внутри, логотип, слова «official»/«endorsed»/«compatible with Flutter» в маркетинговом смысле.

**Конкретный минимум (час работы):**

1. `NOTICE` в корне (имя не случайно — Apache-2.0 §4(d) обязывает downstream сохранять именно файл `NOTICE`): абзац «Portions of this software are derived from Flutter (github.com/flutter/flutter), Copyright 2014 The Flutter Authors, BSD-3-Clause; Flutter is a trademark of Google LLC; this project is not affiliated with or endorsed by Google» + полный текст `.flutter/LICENSE` + список крейтов, содержащих портированные тесты/код.
2. Копия того же `NOTICE` в каталог каждого публикуемого крейта с портированным содержимым (как минимум `flui-rendering`, `flui-widgets`, `flui-animation`, `flui-interaction`, крейты дерева) — `cargo package` включает файлы каталога крейта, корневой файл в `.crate` не попадает.
3. README §License — второй абзац «Acknowledgements» с той же формулировкой; то же в `crates/flui/README.md`, если фасад имеет отдельный README для docs.rs.
4. Унифицировать заголовок в 72 файлах: «Adapted from Flutter `<path>` (BSD-3-Clause, © 2014 The Flutter Authors) — see NOTICE». Опционально — port-check trigger: любой файл с `.flutter/` в тексте обязан содержать эту строку.
5. Поле `license` в `Cargo.toml` **оставить** `MIT OR Apache-2.0`: оно описывает *ваш* грант, а сторонние уведомления живут в `NOTICE` (так делает `kurbo`, `wgpu`, `winit` — все содержат чужой код под другими permissive-лицензиями). Более строгая SPDX-форма `(MIT OR Apache-2.0) AND BSD-3-Clause` точнее для крейтов с переведённым кодом, но заставляет каждого потребителя добавлять BSD-3 в свой `deny.toml` allowlist (обычно уже там) — если консультироваться с юристом, спросить именно про этот пункт.
6. `.flutter/` остаётся gitignored клоном — не распространяется, ничего не требуется. `.gpui/` — Apache-2.0; если начнут портировать паттерны оттуда, тот же `NOTICE` пополняется строкой про Zed Industries.

**Чего не делать:** не перелицензировать workspace в BSD-3 (это выбор `tiny-skia` для дословного порта; FLUI им не является и теряет патентный грант Apache); не писать «clean-room» — референс читался по методологии, это задекларировано в `AGENTS.md`.

### Что из этого делать первым

1. **#2 (атрибуция)** — час работы, блокирует публикацию, юридический риск.
2. **#1 (macOS в CI + H9 через ASan)** — самый большой непокрытый класс регрессий; заодно даёт nightly-canary реальную задачу вместо декоративной.
3. **#6 + #7 (baseline сборки и кадра)** — без них половина рекомендаций §3/§6 непроверяема.
4. **#3 + #5 (panic-поверхность, cargo-mutants)** — прямо отвечают на риск «MVP reported as parity» из AGENTS.md.
5. Остальное — по мере подхода к соответствующим ADR (parley → #10, публикация → #15, web → #16).

---

## 10. Статус исполнения (2026-09-03, тот же день)

Документ выше — оценка на утро 2026-09-03; ниже — что из §7 сделано к вечеру, что при исполнении оказалось не так, и что отложено осознанно. PR-ы стеком: #808 — база, остальные ретаргетятся на `main` после его слияния.

| Пункт §7 | Статус | Где |
|---|---|---|
| `fetch_update` → `try_update` (8 сайтов) + устаревший комментарий в miri-job | **сделано** | #808 |
| Гигиена манифестов (lints inheritance в 2 примерах, `serde_json` под `timeline`, `workspace = true` для `dyn-clone`/`downcast-rs`/`flui-testing`, лишние `readme =`) | **сделано**; наследование lints вскрыло 5 реальных замечаний в двух wasm-демо — исправлены в исходниках | #808 |
| `resolver = "3"`, `.config/nextest.toml`, lld-комментарий | **сделано**. Первый прогон CI под nextest-таймаутами убил два теста `flui-cli` (сборка сгенерированного проекта, ~180 с на 4-ядерном раннере) — бюджет пересчитан по замерам CI: глобально 10 мин, компилирующим тестам 20 мин | #808 |
| `nightly-canary` в `weekly.yml` + `just nightly-check` | **сделано** | #808 |
| `recursion_depth_exceeding_limit` (6 сайтов) | **сделано** через `#![recursion_limit = "256"]`; ручной `impl Send` отвергнут — он вернул бы blanket-утверждение, которое ADR-0045 убрал, и был бы unsound на wasm32 без `fragile-send-sync-non-atomic-wasm` | #809 |
| `Rodeo` → `ThreadedRodeo` | **сделано**; `AssetKey::as_str` теперь `&'static str` (breaking, потребителей вне крейта нет) | #810 |
| `crossbeam-channel` → std mpsc; `regex` → `regex-lite` | **снято** — см. §6b «Оставить»: все сайты каналов опираются на `len()`, тип `Regex` диктует `wgsl_bindgen` | — |
| `#[allow]` → `#[expect]` покрейтно | **сделано одним проходом**: из 673 `allow`-строк осталось 414 `expect`-строк и 15 `allow`-строк. Удалены 145 мёртвых подавлений (`missing_docs` в bench-бинарях, где lint не срабатывает, `dead_code` на давно используемых элементах, `unwrap_used` в тестах, которые clippy.toml и так освобождает) и ещё 298 `expect`'ов на lints, разрешённые на уровне workspace — `expect` включает lint локально, так что такие атрибуты *требовали*, например, наличия усекающего каста там, где workspace его вообще не проверяет; 12 переведены в точный `cfg_attr(...)` (только-в-тестах, только-без-`simd`, только-под-`enable-wgpu-tests`, только-не-wasm). Обёртки над сгенерированным wgsl_bindgen-кодом и 8 подключаемых по `#[path]` демо-модулей остаются `allow` с `reason` — там набор срабатывающих lints зависит от версии генератора и включённых feature. `clippy::allow_attributes` как lint **не** включён: оставшиеся `allow` условны по построению | #814 |
| `#[diagnostic::on_unimplemented]` на публичных трейтах | **сделано** для 11 трейтов; текст закреплён trybuild-кейсами (`not_a_view`, `not_a_render_box`, `not_parent_data`); `do_not_recommend` на blanket-impl не ставился намеренно — цепочка «required for … to implement» и есть подсказка | #812 |
| Web-backend `flui-platform` под clippy | **не планировалось, найдено при проверке**: 15 deny-замечаний в коде, который ни один job не линтил (wasm-check делал только `check`). Исправлены; `wasm-check` теперь гоняет clippy по lib/bin-таргетам wasm-набора | #811 |
| `NOTICE` + README («почему FLUI», acknowledgments) | **сделано**; юридический текст — прочитать перед слиянием | #813 |
| `cocoa`/`objc` → `objc2` | **отложено**: 164 `msg_send!` в 6 файлах, верифицируется только `cross-typecheck` (компиляция без запуска); без ручного smoke на Mac это перенос UB-класса вслепую. Первый кандидат, когда появится macOS-раннер (§8a #1) | — |
| `cfg_select!` в `flui-log`/`flui-platform` | **отложено**: каскады содержат ветки `target_os = "ios"`, которые не компилирует ни один CI-job; перестройка непроверяемого кода противоречит «Done means verified» | — |
| `fmt::from_fn` | **неприменимо по факту**: grep не нашёл ни одного вспомогательного `struct …Debug` — ручных `Debug` через helper-типы в workspace нет | — |
| Бамп dev-пина на 1.99 | ждёт релиза 2026-10-01 | — |
| ADR parley/vello, ADR inspection-протокол + `flui-mcp`, `a11y` в default-features, acceptance-сценарий обзора, `llms.txt` | **не начато** — каждый из них продуктовое решение уровня ADR, а не hygiene-правка | — |

Найденное попутно и починенное: локальный toolchain `1.98.0` был установлен частично (без `rustfmt` и host `rust-std`) — из-за этого `just gate` и pre-push hook были красными на чистом `main`, а не из-за дерева; типо-чекер CI отвергал сам этот документ («Relm4»).

## 9. Источники

- Release notes: [RELEASES.md @1.98.0](https://github.com/rust-lang/rust/blob/1.98.0/RELEASES.md), [1.98 announcement](https://blog.rust-lang.org/2026/08/20/Rust-1.98.0/), [releases.rs 1.99 beta](https://releases.rs/docs/1.99.0/), [Cargo CHANGELOG (1.99/1.100)](https://doc.rust-lang.org/nightly/cargo/CHANGELOG.html)
- `fetch_update`/`try_update`: [rust#148590](https://github.com/rust-lang/rust/pull/148590); атрибуты `#[stable(since = "1.95.0")]` / `#[deprecated(since = "1.99.0")]` — `library/core/src/sync/atomic.rs` в nightly 2026-09-02 (rust-src component)
- Next-gen solver: [blog 2026-08-21](https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/), [rust#160895](https://github.com/rust-lang/rust/issues/160895), [goal #113](https://github.com/rust-lang/goals/issues/113); FCW [rust#159228](https://github.com/rust-lang/rust/issues/159228), пример с wgpu [rust#160036](https://github.com/rust-lang/rust/issues/160036)
- Polonius Alpha: [blog 2026-08-04](https://blog.rust-lang.org/2026/08/04/enabling-polonius-alpha-on-nightly/), [rust#160456](https://github.com/rust-lang/rust/issues/160456), [goal](https://goals.rust-lang.org/2026/polonius.html)
- Parallel frontend: [compiler-team#1005](https://github.com/rust-lang/compiler-team/issues/1005), [rust#160697](https://github.com/rust-lang/rust/pull/160697), [Fast Builds roadmap](https://goals.rust-lang.org/2026/roadmap-fast-builds.html)
- Cranelift: [goal 2026](https://goals.rust-lang.org/2026/improve-cg_clif-performance.html), [goal 2025h2](https://goals.rust-lang.org/2025h2/production-ready-cranelift.html), [repo](https://github.com/rust-lang/rustc_codegen_cranelift/)
- build-std: [goal](https://goals.rust-lang.org/2026/build-std.html), [RFC 3874](https://github.com/rust-lang/rfcs/pull/3874), [RFC 3875](https://github.com/rust-lang/rfcs/pull/3875), [cargo#17398](https://github.com/rust-lang/cargo/pull/17398)
- TAIT/RTN: [goal](https://goals.rust-lang.org/2026/rtn.html); Specialization: [goal](https://goals.rust-lang.org/2026/specialization.html), [rust#149257](https://github.com/rust-lang/rust/issues/149257); portable SIMD: [rust#86656](https://github.com/rust-lang/rust/issues/86656)
- Sanitizers: [goal](https://goals.rust-lang.org/2026/stabilization-of-sanitizer-support.html), [rust#152757](https://github.com/rust-lang/rust/pull/152757), [compiler-team#951](https://github.com/rust-lang/compiler-team/issues/951)
- cargo-lints: [cargo#17298](https://github.com/rust-lang/cargo/pull/17298), [Cargo lints reference](https://doc.rust-lang.org/stable/cargo/reference/lints.html); `hint-mostly-unused`: [Inside Rust 2025-07-15](https://blog.rust-lang.org/inside-rust/2025/07/15/call-for-testing-hint-mostly-unused/), [Cargo unstable](https://doc.rust-lang.org/cargo/reference/unstable.html)
- rustfmt: [Configurations.md](https://github.com/rust-lang/rustfmt/blob/HEAD/Configurations.md), [CHANGELOG](https://github.com/rust-lang/rustfmt/blob/main/CHANGELOG.md)
- Project goals 2026: [overview](https://goals.rust-lang.org/2026/), [program management Jul–Aug 2026](https://blog.rust-lang.org/inside-rust/2026/08/31/program-management-2026-jul-aug/)
- Соседние фреймворки: [egui CHANGELOG](https://github.com/emilk/egui/blob/main/CHANGELOG.md) (0.34 2026-03-26, 0.35 2026-06-25), [Bevy 0.19](https://bevy.org/news/bevy-0-19/) + [migration guide](https://bevy.org/learn/migration-guides/0-18-to-0-19/), [bevy#21765 cosmic-text→parley](https://github.com/bevyengine/bevy/issues/21765), [GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md) + [gpui-release-notes](https://github.com/gpui-archipelago/gpui-release-notes) + [zed#46758 wgpu renderer](https://github.com/zed-industries/zed/pull/46758) + [zed#56065 AccessKit](https://github.com/zed-industries/zed/pull/56065), [Xilem 0.4](https://github.com/linebender/xilem/releases/tag/v0.4.0), [Linebender Q1 2026](https://linebender.org/blog/tmil-25/), [Parley 0.10](https://github.com/linebender/parley/releases/tag/v0.10.0), [sparse-strips 0.0.9](https://github.com/linebender/vello/releases/tag/sparse-strips-v0.0.9), [Iced 0.14](https://github.com/iced-rs/iced/releases/tag/0.14.0), [Dioxus 0.7](https://github.com/DioxusLabs/dioxus/releases/tag/v0.7.0) + [0.8.0-alpha.0](https://github.com/DioxusLabs/dioxus/releases/tag/v0.8.0-alpha.0), [Slint 1.15](https://slint.dev/blog/slint-1.15-released) / [1.16](https://slint.dev/blog/slint-1.16-released) / [1.17](https://slint.dev/blog/slint-1.17-released), [Leptos 0.9.0-beta](https://github.com/leptos-rs/leptos/releases/tag/v0.9.0-beta)
- Остальной рынок: [A 2026 Survey of Rust GUI Libraries](https://blog.wybxc.cc/blog/rust-gui-survey-2026/) (2026-08-23), [Freya 0.4](https://freyaui.dev/posts/0.4), [Ribir releases](https://github.com/RibirX/Ribir/releases), [Vizia 0.4](https://github.com/vizia/vizia/discussions/656), [Makepad 1.0](https://makepad.rs/weekly/makepad/20250518) / [makepad.nl](https://makepad.nl/), [Vexo](https://github.com/vexornp/Vexo), [Frui](https://github.com/fruiframework/frui), [gpui deep-dive (July 2026)](https://github.com/GoldStrikeArch/rust-gui-desktop-ecosystem-state/blob/main/report/03-gpui.md)
- This Week in Rust: [664](https://this-week-in-rust.org/blog/2026/08/12/this-week-in-rust-664/), [665](https://this-week-in-rust.org/blog/2026/08/19/this-week-in-rust-665/), [666](https://this-week-in-rust.org/blog/2026/08/26/this-week-in-rust-666/), [667](https://this-week-in-rust.org/blog/2026/09/02/this-week-in-rust-667/)
- Локально: `rust-toolchain.toml`, `Cargo.toml`, `clippy.toml`, `rustfmt.toml`, `STYLE.md` §2, `docs/PORT.md` §MSRV, `.github/workflows/{ci,weekly}.yml`, `justfile` (`miri`), лог прогона `cargo +nightly check` (2026-09-03)
