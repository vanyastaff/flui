# Полезные материалы для FLUI за последние три месяца

Период: **4 июля — 4 октября 2026 года включительно**. Исследование выполнено
4 октября 2026 года через Keenable и Firecrawl, даты релизов дополнительно
проверены через GitHub API. Это выборка полезных материалов, а не полный каталог
всех обновлений Rust UI. Область поиска включает язык, standard library,
Cargo, Clippy, конфигурацию сборок, дизайн публичных API и инженерные практики,
помимо библиотек и других UI frameworks.

Самые практичные находки: сентябрьские исправления AccessKit для Linux,
мобильные IME edge cases из egui, ограничения headless-проверки из опыта
Xilem/GPUI и Fenestra, а также новые возможности безопасного SIMD. Обновления
wgpu и Parley уже присутствуют в lockfile FLUI: полезно проверить их поведение,
а не повторять миграцию. Для работы над самим framework особенно полезны новые
Cargo profiles/warning policy, изменения Clippy, рекомендации по capability API
и опыт фильтрации до expensive work из rustdoc.

Документ дополняет [исследование библиотек для FLUI](wgpu-ecosystem-adoption.ru.md):
здесь отбор привязан к времени публикации и содержит статьи, реальные исправления
и идеи для проверки. Рекомендации ниже — анализ применимости, не утверждение,
что в FLUI обнаружены перечисленные дефекты. Код и зависимости не изменялись.

## Контекст проекта

Применение к обновлённому `main` и выбранные изменения описаны отдельно в
[отчёте внедрения](research-adoption-2026-10-04.ru.md). Версии ниже сохраняют
исходный снимок исследования; отчёт различает уже внедрённое и новые изменения.

Отбор ориентирован на [roadmap](../ROADMAP.md) и [критерии beta](../BETA.md):
Unicode-текст и IME, native accessibility, несколько окон, реальные GPU-проверки,
idle-поведение и возможность агента создать и проверить приложение.

Снимок checkout: `465b8e13d`. Проверка `Cargo.lock` показала wgpu **30.0.1**,
Parley **0.11.1**, harfrust **0.12.0**, AccessKit **0.25.0**,
accesskit_unix **0.23.0**, accesskit_atspi_common **0.20.0**.
В `rust-toolchain.toml` закреплён Rust **1.99.0**;
`cargo nextest --version` на этом Windows-хосте показал **0.9.146**.
Это версии проверенного checkout и хоста, а не обещание о каждой среде CI.

## Что читать в первую очередь

Приоритет «высокий» означает связь с критериями beta; «средний» — полезный
ориентир для дизайна или оптимизации; «исследовательский» — материал для прототипа.

| Материал | Дата | Приоритет | Польза для FLUI |
|---|---|---|---|
| [AccessKit Unix и AT-SPI](https://github.com/AccessKit/accesskit/releases/tag/accesskit_atspi_common-v0.21.0) | 25 сентября | Высокий | Enabled/sensitive state и Image interface |
| [egui 0.36.0](https://github.com/emilk/egui/releases/tag/0.36.0) | 5 августа | Высокий | Мобильный Web IME и сохранение контекста ввода |
| [Slint 1.18](https://slint.dev/blog/slint-1.18-released) | 16 сентября | Высокий | Текст для screen reader, z-order и большие тексты |
| [GPUI vs Xilem](https://elih.net/blog/gpui-vs-xilem-2026) | 4 сентября по индексу | Высокий | Headless fonts, композиция API и цена generic trees |
| [Fenestra](https://github.com/richer-richard/fenestra) | 9 июля по registry index | Средний | Render → interact → verify и честные ограничения golden tests |
| [wgpu 30.0.1](https://github.com/gfx-rs/wgpu/commit/40f4a34ebaf56f9a046231f54125ad046239d3f3) | 21 августа по changelog | Высокий | Vulkan fence, Metal constants и отказ WebGPU adapter |
| [Parley 0.11.1](https://github.com/linebender/parley/releases/tag/v0.11.1) | 16 августа | Средний | Согласованный стек shaping и font parsing |
| [Fearless SIMD 1.0](https://linebender.org/blog/fearless-simd-1-0/) | 22 сентября, статья | Средний | Safe SIMD, portable semantics и multiversioning |
| [Fearless SIMD 0.6](https://linebender.org/blog/fearless-simd-0-6/) | 11 июля | Средний | AVX-512 и управление dispatch конечным приложением |
| [Hyperbezier curves](https://linebender.org/blog/hyperbezier/) | 8 августа | Исследовательский | Плавные контуры и squircles |
| [nextest 0.9.145](https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.145) | 16 сентября | Высокий | Ложные leaked handles на Apple и наследование профилей |
| [nextest 0.9.143](https://nexte.st/changelog/) | 4 августа | Средний | dylib discovery, архивы и Cargo build directory |
| [Rust 1.98 и 1.99](https://blog.rust-lang.org/2026/08/20/Rust-1.98.0/) | 20 августа и 1 октября | Средний | Новые численные и FFI API, ограничения детерминизма |
| [netrender](https://github.com/merely-made/netrender) | 11 июля по registry index | Исследовательский | Display list, glyph-run boundary, capture/replay |
| [Cargo 1.99](https://github.com/rust-lang/cargo/blob/master/doc/book/src/CHANGELOG.md#cargo-199-2026-10-01) | 1 октября | Высокий | Profiles, CI incremental, inherited default-features |
| [Clippy 1.97–1.99](https://github.com/rust-lang/rust-clippy/blob/master/CHANGELOG.md) | Июль — октябрь | Высокий | Новые правила и исправленные semantics-changing suggestions |
| [Practical Rust API Design](https://corrode.dev/blog/practical-rust-api-design/) | 2 октября | Высокий | Generic relationships, ownership/capabilities и эволюция API |
| [Polonius Alpha](https://blog.rust-lang.org/2026/08/04/enabling-polonius-alpha-on-nightly/) и [trait solver](https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/) | 4 и 21 августа | Исследовательский | Borrowing, arity/GAT и compatibility probes на nightly |
| [Rustdoc performance](https://noahlev.org/blog/2026/08/27/making-rustdoc-faster) | 27 августа | Средний | Filter before materialization и hidden UI test dependencies |
| [Vello CPU/GPU 0.3](https://github.com/linebender/vello/releases/tag/sparse-strips-v0.3.0) | 2 октября | Высокий | Compositing edge cases, fallible backends и capability probes |
| [ps-qa 0.7.6](https://docs.rs/ps-qa/0.7.6/ps_qa/) | 28 сентября | Высокий | Exact selectors, effect assertions, mutation checks |

## Доступность и ввод

### AccessKit и реальные состояния Linux accessibility

**Источник:** релизы [accesskit_atspi_common 0.21.0](https://github.com/AccessKit/accesskit/releases/tag/accesskit_atspi_common-v0.21.0)
и [accesskit_unix 0.24.0](https://github.com/AccessKit/accesskit/releases/tag/accesskit_unix-v0.24.0),
25 сентября 2026 года. Исправление [PR 788](https://github.com/AccessKit/accesskit/pull/788)
публикует enabled и sensitive для узлов, которые не disabled;
добавлена поддержка AT-SPI Image interface.

**Для FLUI:** кандидат на отдельное обновление адаптеров и live-проверку Linux.
В lockfile ещё Unix 0.23.0 и atspi_common 0.20.0. Проверить через Orca/AT-SPI
обычную кнопку, disabled-кнопку и изображение с описанием. Проверка только
SemanticsConfiguration не покажет, какие состояния увидит системный клиент.

Соседний [AccessKit 0.25.1](https://github.com/AccessKit/accesskit/releases/tag/accesskit-v0.25.1)
исправляет `tree_id` в JSON schema; это отдельное изменение, не исправление
всей доступности. [Android 0.9.0](https://github.com/AccessKit/accesskit/releases/tag/accesskit_android-v0.9.0)
добавляет live regions и регулировку sliders, исправляет события при отключении
accessibility, detached host view и selection action для мёртвого узла.
Эти случаи полезны как матрица lifecycle-проверок экспериментального Android.

### egui и мобильный IME в Web

**Источник:** [egui 0.36.0](https://github.com/emilk/egui/releases/tag/0.36.0),
5 августа 2026 года по GitHub API. Релиз сообщает об улучшениях autocomplete,
autocorrect и IME для eframe web на iOS/Android.

Особенно полезен [PR 8045](https://github.com/emilk/egui/pull/8045), merged
29 июля: Samsung Cheonjiin использует предыдущий текст для корейской композиции,
и преждевременная очистка скрытого text input разрушала этот контекст. Изменение
также вводит `ImeEvent::DeleteSurrounding`. Автор отдельно отмечает оставшийся
сброс раскладки при перемещении курсора и проверки участником на Samsung.

**Для FLUI:** добавить эти сценарии в будущую live-матрицу Web text input:
композиция с повторным редактированием, удаление surrounding text, autocorrect,
перемещение caret с открытой символьной клавиатурой. Не считать окончание
одного события composition разрешением немедленно стереть весь контекст.
Это поведение browser bridge; оно не доказывает дефект Win32 TextStore.

### Slint 1.18 как список desktop acceptance cases

**Источник:** [официальная статья](https://slint.dev/blog/slint-1.18-released)
и [релиз](https://github.com/slint-ui/slint/releases/tag/v1.18.0), 16 сентября.
Добавлены FlexboxLayout, динамический z-order, spring animations, движение по
path, экспериментальный Vello renderer. Screen readers теперь видят содержимое
и selection text input. Авторы также сообщают о более компактном generated code
и более быстром редактировании больших текстов, без универсального численного
обещания для других frameworks.

**Для FLUI:** проверить совпадение paint order и hit-test order после смены
z-order, доступность selection при редактировании, continuity spring animation
при смене цели и большие multiline buffers. Показательно, что desktop toolkit
выделяет эти наблюдаемые свойства в релизе. Копировать DSL или FlexboxLayout
вместо существующего constraints protocol из этого не следует.

## UI API и проверка приложений

### Практический опыт переноса редактора между GPUI и Xilem

**Источник:** Eli Heuer, [GPUI vs Xilem 2026](https://elih.net/blog/gpui-vs-xilem-2026).
Keenable датирует публикацию 4 сентября; текст описывает состояние конца лета.
Точная дата не подтверждена отдельным release record, поэтому точность ниже,
чем у датированных release notes.

Автор описывает три особенно полезных случая: headless harness с фиксированным
шрифтом показывал другой текст, чем живое окно с system fonts; глубоко
вложенные view types создавали проблемы link/compile; отсутствие удобного
view-level доступа к capabilities заставляло уходить в ручные widgets.
Это отчёт об одном приложении, не сравнительный benchmark всех GPUI/Xilem apps.

**Для FLUI:** две font-конфигурации должны проверять разные контракты:
детерминированные golden tests и native fallback для CJK/RTL/emoji. Для facade
полезен внешний consumer с большим составным экраном, меню, popup и shortcuts:
он проверит ergonomics и время сборки реальной композиции. Возможность внутри
runtime ещё не означает, что пользователь может достичь её через `flui`.

Независимый [README Fenestra](https://github.com/richer-richard/fenestra)
также явно разделяет embedded-font headless coverage и системные шрифты живого
окна. Это подтверждает полезность такого разделения, но не все претензии автора
к конкретным версиям GPUI или Xilem.

### Fenestra и замкнутый цикл проверки UI

**Источник:** [репозиторий](https://github.com/richer-richard/fenestra),
[demo и книга](https://richer-richard.github.io/fenestra/),
[registry index](https://lib.rs/crates/fenestra).
Index показывает 0.40.0 от 9 июля. README прочитан в текущем состоянии;
описанные возможности нельзя автоматически приписывать именно июльской версии.

README показывает headless render, synthetic input и проверку результата,
а также tokens для темы/spacing. Он прямо описывает отличия headless от live:
шрифты, reduced motion и эффекты стекла; на Web остаются ограничения accessibility.
Текущая версия README отличается от индексированной копии, поэтому здесь
не фиксируются число MCP tools и точное покрытие старого релиза.

**Для FLUI:** полезен образец пользовательского сценария
«построить экран → отправить событие → дождаться кадра → проверить семантику
и пиксели». В документации flui-testing стоит описывать, что действительно
проверяет виртуальный host, а что требует native run. Typed design tokens
можно рассмотреть на уровне официальных пакетов, без ограничения фундаментальной
геометрии framework. Заявленные в README скорости в FLUI не измерялись.

## Рендеринг и текст

### wgpu 30.0.1 и ошибки на платформенных границах

**Источник:** [release commit с changelog](https://github.com/gfx-rs/wgpu/commit/40f4a34ebaf56f9a046231f54125ad046239d3f3).
Changelog датирован 21 августа; registry index показывает 22 августа.
Это различие даты release notes и публикации, не два разных релиза.

Исправлены Vulkan validation errors от unwaited fence при
`vkAcquireNextImageKHR` на non-Windows; динамическое разрешение Metal color-space
constants; panic WebGPU bindings при отказе `requestAdapter()`.

**Для FLUI:** 30.0.1 уже в lockfile. Полезны Linux Vulkan live smoke с validation,
проверка отсутствующего WebGPU adapter и native Metal startup. Они проверят
достижимость исправлений через flui-engine и корректный recovery пользователя.
wgpu 30.0.0 вышел 1 июля и сам не входит в исследуемый период.

### Parley 0.11.1 и согласованность font stack

**Источник:** [release notes](https://github.com/linebender/parley/releases/tag/v0.11.1),
16 августа; [registry index](https://lib.rs/crates/parley) подтверждает дату.
Обновлены read-fonts до 0.41, skrifa до 0.44 и harfrust до 0.12; MSRV — 1.88.
Release notes не заявляют отдельную новую возможность редактирования.

**Для FLUI:** Parley 0.11.1 и harfrust 0.12.0 уже разрешены. При изменении font
stack проверять shaping, fallback, variation axes, baseline и glyph rasterization
вместе: версия layout crate сама по себе не доказывает правильность пикселей.
Для тестов полезны смешанный RTL/LTR, CJK fallback и variable font под fractional DPR.

### netrender как архитектурный ориентир

**Источник:** [текущий репозиторий](https://github.com/merely-made/netrender)
и [registry index](https://lib.rs/crates/netrender), где первая публикация 0.1.0
датирована 11 июля. Текущий README описывает более позднее состояние проекта:
display list переводится в Scene, Parley glyph runs передаются через отдельный
adapter, имеются capture/replay и экспериментальные CPU/Hybrid backends.

**Для FLUI:** изучить границу producer/display list/renderer и передачу уже
shaped glyphs без второго shaper. Для диагностики интересны replay минимальной
сцены и bounded glyph-cache epochs. Это ориентир для собственного engine
contract, не готовая замена flui-engine.

Ограничения: проект ранний, MPL-2.0; CPU/Hybrid возможности ограничены, в том числе
для color/bitmap/SVG fonts. Индексированная копия README и текущий репозиторий
содержат разные зависимости и статус. Старый вложенный README URL оказался 404;
использован рабочий корневой URL, а старые claims о покрытии не приняты как факт.

## Численные библиотеки и геометрия

### Fearless SIMD 1.0 на stable Rust

**Источник:** [статья Shnatsel](https://linebender.org/blog/fearless-simd-1-0/),
22 сентября; [GitHub release](https://github.com/linebender/fearless_simd/releases/tag/fearless_simd-v1.0.0)
опубликован 21 сентября. Библиотека предлагает portable SIMD и safe доступ к
intrinsics; вместе с ней выпущен fearless_simd_macros 0.1 с `#[simd]`.
Для части операций различаются precise и fast варианты, у fast результат
на edge cases может зависеть от платформы.

**Для FLUI:** кандидат для измеренного CPU hotspot — обработки пикселей,
цветовых преобразований или подготовки geometry. Начать с профиля и независимого
scalar oracle; включить NaN, infinity, signed zero и tail неполного вектора,
когда они допустимы контрактом. Для golden tests нельзя выбирать fast-вариант
только по имени. Не добавлять новую зависимость без показанной экономии на workload.

### AVX-512 и управление dispatch

**Источник:** [Fearless SIMD 0.6](https://linebender.org/blog/fearless-simd-0-6/),
11 июля. Статья объясняет выбор Ice Lake baseline на Intel, возможность отключать
наборы инструкций и почему это настройки конечного binary через cfg, а не
обычные additive Cargo features. Упомянут выигрыш Vello CPU на AMD Zen 4 —
это измерение конкретной библиотеки и hardware, не прогноз скорости FLUI.

**Для FLUI:** если появится SIMD CPU path, сравнивать AVX2/AVX-512 и portable
fallback по времени и размеру binary. Выбор dispatch принадлежит приложению;
безусловное `target-cpu=native` в распространяемой сборке ограничивает hardware.
Статья полезна прежде всего как объяснение контракта конфигурации.

### Hyperbezier curves для гладких контуров

**Источник:** Raph Levien, [The mathematical beauty of hyperbezier curves](https://linebender.org/blog/hyperbezier/),
8 августа. Предложено семейство кривых с параметризацией по длине дуги,
включающее круговые дуги и Euler spirals; показаны применения к squircles
и плавному изменению кривизны. Автор отмечает предварительность mapping
control points и необходимость дальнейшей работы для практического инструмента.

**Для FLUI:** материал для будущего path editor или эксперимента с формой углов.
Прототип должен отдавать существующий Path, затем проверять flattening error,
stroke, hit-test и экстремальные параметры. Это исследование, а не основание
заменять текущий kurbo/lyon pipeline перед beta.

## Инструменты и язык

### nextest 0.9.145 и ложные утечки handles

**Источник:** [release notes](https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.145),
16 сентября, сверены с [официальным changelog](https://nexte.st/changelog/).
На Unix, особенно Apple, параллельно запускаемый test мог унаследовать capture
pipe соседа и вызвать ложный leaked-handles report. Исправлены также overrides
и setup/wrapper scripts в промежуточных наследуемых profiles.

**Для FLUI:** при разборе intermittent macOS leak report сначала фиксировать
версию runner, затем проверять реальные ресурсы теста. В проекте есть leak timeout
и группы тяжёлых nested-cargo tests, поэтому диагноз имеет практическую ценность.
На исследовательском Windows-хосте уже 0.9.146; состояние Mac/CI не проверено.

### nextest 0.9.143 и размещение build artifacts

**Источник:** [changelog](https://nexte.st/changelog/), 4 августа.
Исправлены dynamic-library search paths для build directory layout v2,
настроенного `build.build-dir` и tests из examples. Архив теперь сохраняет
dynamic libraries, даже если tests их пакета исключены filterset.

**Для FLUI:** полезно при диагностике consumer/template tests и воспроизведении
CI через архивы. Не лечить отсутствующий dylib изменением API framework,
пока не проверены runner и layout. Наличие исправления не означает, что FLUI
уже использует layout v2 или фильтрованные архивы.

### Rust 1.98 и 1.99

**Источники:** [Rust 1.98](https://blog.rust-lang.org/2026/08/20/Rust-1.98.0/),
20 августа; [Rust 1.99](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/),
1 октября. В 1.98 появились algebraic float methods и buffered integer
formatting через NumBuffer; algebraic operations допускают перестановку
вычислений и недетерминированные оптимизации. В 1.99 стабилизированы определения
C variadic functions и ряд raw layout APIs.

**Для FLUI:** Rust 1.99 уже закреплён. Algebraic float methods стоит рассматривать
только внутри измеренного численного kernel с явно выбранной погрешностью;
геометрия, reconciliation и golden assertions не должны случайно менять контракт.
NumBuffer может пригодиться в диагностике, если allocation там измерен.
Новые FFI API нужны лишь при реальном platform use case, не для расширения unsafe
surface ради использования новой версии языка.

## Язык, ownership и evolution типов

### Rust 1.97: символы, linker diagnostics и битовые операции

**Источник:** [официальный релиз](https://blog.rust-lang.org/2026/07/09/Rust-1.97.0/),
9 июля. Symbol mangling v0 стал форматом по умолчанию; появились новые битовые
операции, включая `bit_width`, `highest_one`, `lowest_one` и методы изоляции бита.
Linker output теперь представлен предупреждением `linker_messages`, которое
не входит в группу `warnings`.

**Для FLUI:** v0 полезен при чтении flamegraphs и crash traces с generic-функциями.
Профиль release уже сохраняет символы, удаляя debuginfo: это существующий выбор,
а не предложение включить всё debug в shipping build. Битовые API — повод
проверить понятность упаковки generational IDs, сохранив slot/generation contract.
Platform linker warnings следует разбирать отдельно: общий запрет `warnings`
не доказывает отсутствие этих сообщений.

### Rust 1.98: строки и явные границы unsafe

**Источник:** [release notes](https://blog.rust-lang.org/2026/08/20/Rust-1.98.0/),
20 августа. Дополнительно к численным API интересны `substr_range`/`subslice_range`
и UTF-16LE/BE decoding APIs. Уточнён контракт перемещения уже dropped
`ManuallyDrop<Box<T>>`.

**Для FLUI:** range APIs могут упростить адаптеры, которым нужен диапазон
подстроки в исходном буфере. Они не превращают byte offsets в grapheme offsets:
TextStore, shaping и selection обязаны сохранять собственные единицы индексации.
UTF-16 byte decoding тоже не заменяет контракт native text store с `u16` units.
Изменение `ManuallyDrop` нужно учитывать при unsafe review; оно не доказывает
безопасность произвольного generic контейнера после ручного уничтожения поля.

### Rust 1.99: обратимое raw ownership вместо reclaim после leak

**Источник:** [release notes](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/),
1 октября. Появились `Vec::into_parts`/`from_parts`, `Box::into_non_null`/
`from_non_null` и raw layout APIs. Документация `Box::leak` теперь рекомендует
не строить reclaim через возвращённую ссылку; для обратимой передачи ownership
предпочтительны `into_raw` или `into_non_null`.

**Для FLUI:** это материал для platform callbacks и hot reload: ровно один владелец
должен восстановить allocation, после последнего callback, включая error/unwind
и teardown. В checkout уже есть `into_raw` на этих границах. Новые функции
не являются причиной переписывать рабочий ownership protocol; важнее проверить
allocation provenance, allocator/layout и прекращение доступа до освобождения.

### Polonius Alpha: меньше borrow-checker workarounds, пока nightly

**Источник:** [официальный анонс](https://blog.rust-lang.org/2026/08/04/enabling-polonius-alpha-on-nightly/),
4 августа. Flow-sensitive lifetime analysis принимает некоторые корректные
ветвления, которые NLL отвергает: например, вернуть mutable borrow в одной
ветке и изменить коллекцию в другой. Это Alpha, а не вся исходная модель Polonius;
анонс включает известные compile-time regressions.

**Для FLUI:** полезен для arena/reconciliation API, где workaround ради borrow
checker способен привести к дублированию lookup или лишнему interior mutability.
Проверить существующие сложные consumer cases на nightly можно как эксперимент;
публичный API и stable gate остаются проверяемыми на закреплённом 1.99.
Анонс не позволяет считать новые borrow patterns доступными на stable.

### Новый trait solver: проверить arity, GAT и compile-fail контракты

**Источник:** [официальный анонс](https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/),
21 августа. Новый solver включён на nightly; меняются inference, нормализация
associated types, higher-ranked lifetime cases и обработка opaque types.
Есть как исправленные ошибки, так и намеренные несовместимости.

**Для FLUI:** особенно полезен для typed child arity, derive macros, sealed traits
и SDK consumer fixtures. Эксперимент должен сравнивать успешные программы,
ожидаемые compile failures и время сборки. То, что отрицательная fixture теперь
компилируется, требует проверки: это может быть улучшение solver или потерянная
граница API. TAIT/RTN упомянуты как будущие возможности, не stable API 1.99.

## Cargo и конфигурация сборок

### Cargo 1.97: запрет warnings без нового compiler fingerprint

**Источники:** [Rust 1.97](https://blog.rust-lang.org/2026/07/09/Rust-1.97.0/),
9 июля; [Cargo changelog](https://github.com/rust-lang/cargo/blob/master/doc/book/src/CHANGELOG.md).
Стабильны `build.warnings` / `CARGO_BUILD_WARNINGS` со значениями
`allow`, `warn`, `deny`; смена режима не инвалидирует build cache как изменение
compiler flags через `RUSTFLAGS`. Также стабилен `resolver.lockfile-path`.

**Для FLUI:** это практичный способ разделять локальное отображение diagnostics
и строгий gate, не заставляя общий target повторно компилировать весь workspace
из-за одной warning policy. В tooling уже найдено использование
`CARGO_BUILD_WARNINGS=warn`: сначала учитывать существующую policy. Отдельный
lockfile полезен для изолированных consumer/tooling probes, если это соответствует
контракту запуска; production lockfile и его обновления должны оставаться явными.

### Cargo 1.99: profiles, incremental и workspace features

**Источник:** [Cargo 1.99 changelog](https://github.com/rust-lang/cargo/blob/master/doc/book/src/CHANGELOG.md#cargo-199-2026-10-01),
1 октября. Добавлен встроенный профиль `debug`, сейчас эквивалентный `dev`.
Incremental по умолчанию выключается, когда установлен `CI`. Для edition 2024
member может отключить inherited dependency `default-features`, даже когда
workspace включает их. Имена lints с дефисами deprecated: нужен underscore.

**Для FLUI:** пользовательский `dbg` с full debug имеет собственный смысл и
не становится ненужным из-за нового встроенного `debug`. У FLUI явно заданы
dev incremental и CI override: новое значение по умолчанию не означает, что
можно удалить override без проверки effective configuration. Возможность
переопределить features полезна для лёгких слоёв и wasm, но требует просмотра
resolved feature graph: другой member всё ещё может включить feature обратно.
Это не способ обойти tier/reach restrictions.

### Dependency cooldown и Cargo lints: merged не означает stable

**Источники:** [stabilization PR min-publish-age](https://github.com/rust-lang/cargo/pull/17335),
merged 28 августа, milestone **1.100.0**;
[Cargo changelog](https://github.com/rust-lang/cargo/blob/master/doc/book/src/CHANGELOG.md).
В changelog 1.99 `min-publish-age` и `cargo-lints` находятся в **Nightly only**;
стабилизация перечислена в будущем разделе 1.100 с датой 12 ноября.
Это направление развития за изучаемый период, а не доступная stable настройка.

**Для FLUI:** cooldown позволяет не выбирать совсем свежие версии зависимостей;
новый Cargo lint system включает проверку unused dependencies. Следить за ними
стоит для будущего обновления toolchain. Пока нельзя предлагать эти настройки
для stable 1.99 или заменять ими `cargo xtask deps` и cargo-shear. После выхода
нужны исключения для срочных fixes, проверка locked versions и feature coverage.

### Уменьшение target directory: metadata отдельно от rlib

**Источники:** [Inside Rust](https://blog.rust-lang.org/inside-rust/2026/08/18/reducing-target-dir-size-on-nightly/),
18 августа; [отчёт Jakub Beránek](https://kobzol.github.io/rust/2026/09/30/stf-august-september-2026.html),
30 сентября. Nightly эксперимент `-Zembed-metadata=no` убирает дублирование metadata
в rlib, сохраняя отдельные rmeta. Во внешних измерениях снижение размера target
составляло 5–35%; это не измерение FLUI и не стабильная опция 1.99.

**Для FLUI:** особенно интересно на memory/disk-limited dev host. Tooling,
hot reload и consumer tests, которые вручную ищут rlib, должны учитывать
соответствующий rmeta. Пути артефактов лучше получать из Cargo JSON messages,
а не выводить из предполагаемой структуры target. Эксперимент нужно проводить
на отдельном build directory с проверкой link/reload/archives.

## Clippy: новые правила, миграции и качество диагностики

**Первичный источник:** [Clippy changelog](https://github.com/rust-lang/rust-clippy/blob/master/CHANGELOG.md),
разделы Rust **1.97 — 9 июля**, **1.98 — 20 августа**, **1.99 — 1 октября**.
FLUI уже включает `all` и `pedantic`; правило из этих групп может уже действовать.
Таблица — поводы проверить конкретный код, а не список обязательных новых allow/deny.

| Версия | Правило | Зачем смотреть в FLUI |
|---|---|---|
| 1.97 | `manual_assert_eq` (pedantic) | Assertion diagnostics должны показывать обе величины; полезно в таблицах behavior cases |
| 1.97 | `manual_clear` (perf) | Проверить очистку frame-local collections без обхода элементов через remove/pop |
| 1.97 | `useless_borrows_in_formatting` (perf) | Упростить diagnostic formatting; не обещать измеримый frame speedup |
| 1.98 | `chunks_exact_to_as_chunks` (style) | Выразить fixed-size data grouping через arrays, когда нужен этот контракт |
| 1.98 | `unnecessary_unwrap_unchecked` (complexity) | Удалить ненужную unsafe операцию, сохранив failure behavior |
| 1.98 | `for_unbounded_range` (suspicious) | Пересмотреть циклы со счётчиком без естественного finite bound |
| 1.98 | `unused_async_trait_impl` (pedantic) | Уже явно разрешён в FLUI ради async trait families/stub signatures; не добавлять дублирующее исключение |
| 1.99 | `assert_is_empty` (pedantic) | Улучшить смысл проверок пустоты без тестов private structure |
| 1.99 | `nonnull_unchecked_on_box_ptr` (complexity) | Проверить, нельзя ли получить NonNull из ownership-aware API вместо unchecked conversion |
| 1.99 | `manual_bit_width` (pedantic), `mismatched_bit_width_type` (suspicious) | Проверить ID/bitset arithmetic и согласованность ширины типа |

### Меняются не только новые lints

В 1.97 `nonminimal_bool` и `overly_complex_bool_expr` переведены в pedantic;
исправлены ошибочные предложения `collapsible_match` и coherence-проблемы
`from_over_into`. В 1.98 `empty_enums` перемещён в nursery,
`from_iter_instead_of_collect` deprecated, а `result_large_err`/`result_unit_err`
начали проверять async functions. В 1.99 `clone_on_copy` учитывает UFCS, а
`manual_div_ceil` избегает предложений, меняющих число вычислений divisor
с побочными эффектами.

**Практика:** при toolchain bump перечитывать и additions, и moved/deprecated/fixed.
Не принимать массовый `cargo clippy --fix` как доказательство семантической
эквивалентности. Изменения suggestions важны именно для guard lifetimes,
side effects, Drop и arithmetic edge cases.

### Стиль модулей: restriction lints требуют отдельного решения

В период появились `inline_trait_bounds`/`inline_modules` (1.97),
`definition_in_module_root`, `rest_pattern_accessible_field` и
`unnecessary_rest_pattern` (1.99) из restriction. Они предлагают конкретный
стиль записи/размещения, который не равен архитектурной правильности.
У FLUI уже есть module DAG и crate layers; включать всю restriction группу
ради порядка в файлах не следует. Выбирать правило стоит по реальной ошибке,
которую оно предотвращает, и совместимости с существующей композицией тестов.

## Дизайн API, стиль и инженерные практики

### Practical Rust API Design: locality, generic relationships, capabilities

**Источник:** [Matthias Endler / Corrode](https://corrode.dev/blog/practical-rust-api-design/),
2 октября по metadata страницы. Рекомендации: делать reasoning локальным,
выражать связи типов явно, применять `impl Trait` к независимому unnamed generic,
а именованный параметр — когда одна и та же связь нужна в нескольких местах.
`Deref` подходит прозрачным pointer-like wrappers; доменный wrapper не должен
автоматически наследовать чужую API поверхность. Ownership охватывает ресурсы
и права доступа, как у `BorrowedFd`/`OwnedFd`.

**Для FLUI:** хороший review checklist для LifecycleContext handles, realm IDs
и SDK traits. Проверять, можно ли смешать handles разных realms, сохранить
capability после teardown или обойти lifecycle через wrapper. Возможность
выразить типовую связь полезнее getter/setter chains. При этом типы не доказывают
семантику, отсутствие panic или скорость: такие обещания всё равно требуют
docs и behavior tests. Изменение public trait/evolution policy — отдельный ADR.

### Rustdoc performance: фильтровать до дорогой материализации

**Источник:** [Noah Lev, How I made Rustdoc 33% faster in one week](https://noahlev.org/blog/2026/08/27/making-rustdoc-faster),
27 августа. Серия оптимизаций сокращает среднее wall time в опубликованной
benchmark suite на 25% (что соответствует speedup 33%); статья указывает
попадание в stable 1.99. Основная идея: не строить полное представление impl,
которое затем будет выброшено фильтром. Проверки GUI обнаружили hidden dependency
у notable-trait tooltip, которую обычная suite почти не показывала.

**Для FLUI:** применимо к diagnostics, semantics и rendering: сначала дешёвый
критерий необходимости, затем expensive work, если это сохраняет observable
behavior. Culling нельзя автоматически переносить на semantics или hit testing.
Практика удаления подозрительного пути с запуском tests помогает найти реальные
контракты; один зелёный unit gate не заменяет live/UI coverage. Процент из rustdoc
не является прогнозом ускорения FLUI.

### Compiler performance: измерять outliers и полный workload

**Источник:** [Nicholas Nethercote, September 2026](https://nnethercote.github.io/2026/09/30/how-to-speed-up-the-rust-compiler-in-september-2026.html),
30 сентября. В rustc benchmark suite за 29 июля — 28 сентября mean wall time
снизился на 4.57%; отдельные регрессии сохранились. Разобраны PGO для Clippy,
allocation avoidance, static dispatch, dataflow traversal и затраты новых solvers.

**Для FLUI:** measuring practice важнее одного среднего числа: отдельно
clean/incremental build, consumer с deeply nested View composition, derives,
clippy и rustdoc. Новый solver может улучшить один generic workload и ухудшить
другой. Статья описывает compiler development: не все перечисленные PR уже
в stable 1.99. Оснований включать PGO или `#[inline]` повсюду в FLUI из неё нет.

## Дополнительные библиотеки: layout, rendering, текст и animation

### Taffy 0.14: measure contract и scrollable overflow

**Источник:** [release 0.14.0](https://github.com/DioxusLabs/taffy/releases/tag/v0.14.0),
24 августа, дата package также проверена через crates.io API.
Расширена поддержка CSS sizing keywords; measurement переходит от одиночных
baselines к `Baselines`, используется `LayoutInput` → `LayoutOutput`;
`scrollable_overflow_rect` заменяет прежний `content_size`.

**Для FLUI:** полезен как библиотека flex/grid алгоритмов и reference edge cases.
Adapter должен явно переводить constraints, intrinsic measurement, baselines
и overflow; измеренный size, paint bounds и scroll extent не одна величина.
Поддержка sizing keywords различается между обычными и min/max размерами.
Не переносить CSS tree поверх View/Element ради подключения алгоритма.

### Vello CPU/GPU 0.3: compositing correctness и backend capabilities

**Источник:** [sparse-strips 0.3.0 release](https://github.com/linebender/vello/releases/tag/sparse-strips-v0.3.0),
2 октября. `vello_hybrid` переименован в **`vello_gpu`**. Release notes включают
root blending isolation, cache collision между premultiplied/unpremultiplied
gradients, пропуск non-invertible paint transforms, обработку прозрачных draws
при filters, configurable target initialization и external texture paints.
Есть fallible WebGL errors и nonblocking begin APIs. Stability guarantees нет;
GPU ещё не имеет полного feature parity. SIMD dependency этой линии — 0.7,
не следует приписывать ей автоматически release Fearless SIMD 1.0.

**Для FLUI:** сильный checklist для engine readbacks: destination background,
градиенты с совпадающими ключами, singular transform, flood filter на transparent
input, clear/load target и caller-owned texture lifetime. Feature probes должны
возвращать конкретные capabilities. CPU backend интересен для воспроизводимых
сравнений, но pixel equality разных растеризаторов требует выбранной tolerance.

### Vello 0.11: отдельная research-renderer линия

**Источник:** [release 0.11.0](https://github.com/linebender/vello/releases/tag/v0.11.0),
2 октября. Обновлены wgpu/naga до 30; исправлены sbix glyph bearings.
Это другая renderer API, чем `vello_gpu` 0.3.

**Для FLUI:** читать для glyph placement и GPU integration. Совпадение версии
wgpu облегчает dependency compatibility, но не доказывает совпадение scene,
surface и compositing contracts. Pilot должен проверять одну выбранную линию.

### Glifo 0.4: fallible glyph rendering и bitmap placement

**Источники:** [release](https://github.com/linebender/vello/releases/tag/glifo-v0.4.0),
2 октября; [versioned API](https://docs.rs/glifo/0.4.0/glifo/).
Glyph fill/stroke возвращают `Result` с `GlyphRenderError`;
atlas command replay допускает fallible callback, исправлены sbix offsets,
добавлено bilinear sampling. Библиотека экспериментальная.

**Для FLUI:** интересна как готовый glyph cache/rendering компонент после shaping,
а также как набор failure cases: ошибка загрузки glyph, atlas replay failure,
placement цветного bitmap glyph и последующая успешная отрисовка. Она не заменяет
Parley/harfrust и контракт text layout. Не скрывать fallible работу дефолтным
пустым glyph, если это нарушит paint/selection geometry.

### AnyRender 0.14: границы renderer adapter

**Источники:** [репозиторий и README](https://github.com/DioxusLabs/anyrender);
[package versions](https://crates.io/crates/anyrender/versions), 0.14.0 — 3 октября,
0.13.0 — 16 августа. Текущая документация разделяет `PaintScene`,
`WindowRenderer`, `ImageRenderer`, показывает CPU/Hybrid/Vello/Skia backends.
README прочитан отдельно; все его возможности не приписываются каждой версии.

**Для FLUI:** полезен для выбора engine adapter boundary: recording commands,
offscreen output и presentation — разные обязанности. Проверить, можно ли
согласовать его поверхность с DisplayList и Layer без потери capabilities.
Само наличие backend-neutral trait не оправдывает второй параллельный protocol
в FLUI; часть идей может пригодиться без зависимости.

### Animato 1.7.2: exit animation, retained items и FLIP

**Источники:** [репозиторий](https://github.com/AarambhDevHub/animato);
[версии](https://crates.io/crates/animato/versions), 1.7.2 — 20 июля.
Документация описывает springs/tweens/timelines и lifecycle exit animations:
удалённые из списка строки остаются до завершения transition, затем уничтожаются;
FLIP анимирует изменение расположения сохранившихся элементов.

**Для FLUI:** reference для reconciliation/keep-alive/animation contracts.
Acceptance cases: remove → reinsert того же key до завершения, cancellation,
смена realm, reduced motion, hit testing и semantics выходящего элемента.
Animation math можно рассматривать отдельно; renderer/runtime зависимости
библиотеки не должны попасть в нижний animation layer без причины.

## Архитектура инструментов, platform и automation

### Blitz 0.3 beta: слои native web engine

**Источники:** [Blitz](https://github.com/DioxusLabs/blitz);
[версии](https://crates.io/crates/blitz/versions), beta.1 — 10 июля,
beta.2 — 24 августа. Текущий README описывает модульное сочетание Stylo,
Taffy, Parley, AccessKit и platform integration. Простой string frontend и
Dioxus Native frontend с VirtualDom имеют разную интерактивность.

**Для FLUI:** сравнить границы typography/layout/accessibility/runtime, выбрать
зрелый компонент для нижнего слоя. Полезен и как внешний consumer того же
Rust graphics stack. HTML/CSS frontend не является предлагаемой формой FLUI API.

### Subsecond: hot reload как versioned вызов

**Источники:** [Subsecond](https://github.com/DioxusLabs/dioxus/tree/main/packages/subsecond);
[версии](https://crates.io/crates/subsecond/versions), 0.7.10 и 0.8.0-alpha.1 — 30 июля.
Документация показывает `current`/`call` и проверку `changed`/`on_changed`
у hot function. Это источник для API и lifecycle reload, а не подтверждённый
benchmark задержки FLUI.

**Для FLUI:** сопоставить с DevReloadHook: что обновляется атомарно, кто владеет
старым code/data до конца callback, что происходит при неудачной компиляции
и повторном reload. State preservation нуждается в конкретном compatibility
contract; один patchable function pointer его не обеспечивает.

### Crownshell: Linux shell capabilities не равны обычному window API

**Источники:** [репозиторий](https://github.com/Crown-OS/crownshell);
[версии](https://crates.io/crates/crownshell/versions), 0.2.0 — 4 августа.
Текущий README после перемещения в Crown-OS/crownOs описывает layer-shell,
anchors, exclusive zones, keyboard interactivity и calloop multiwindow.
Fractional scale аппроксимируется целочисленным; blur зависит от compositor.
Это состояние текущей документации, не полное описание исторического 0.2.

**Для FLUI:** background/panel/overlay — отдельные platform capabilities,
которые нужно запрашивать и обнаруживать. Полезный reference для Linux backend,
если такие surfaces нужны продукту. Не обещать fractional-DPR correctness
или переносимость compositor extensions по наличию общего window trait.

### eguidev/edev: frame-boundary introspection и сценарии агента

**Источники:** [eguidev](https://github.com/cortesi/eguidev);
[package edev](https://crates.io/crates/edev/versions), 0.1.0 — 5 июля.
README описывает `frame_scope`, tagging controls через dev wrappers,
roles/labels/values/geometry/hierarchy и Luau scripts для click/type/expect/settle.
Script sandbox исключает file/network/import; video support platform-limited.

**Для FLUI:** сравнить wire vocabulary и frame snapshot с flui-protocol и
semantics. Targets должны быть стабильными и scoped к realm; geometry должна
соответствовать опубликованному frame. Дополнительные dev wrappers полезны
только если не создают расходящуюся копию реального interaction/semantics пути.

### ps-qa 0.7.6: проверять эффект команды, а не только acknowledgement

**Источник:** [versioned документация](https://docs.rs/ps-qa/0.7.6/ps_qa/),
28 сентября по crates.io API. QA model разделяет precondition, action и assertion;
рассматривает неоднозначные selectors, reset fixtures и mutation checks.
Paint и presentation различаются; исчезновение и сохранение zero-size элемента
тоже имеют разные значения.

**Для FLUI:** сильный материал для flui-testing/agent protocol: проверять
конкретный subject и observable result после доставки input. Test должен падать,
если production action отключена; успешный ack или просто существующий textbox
этого не доказывает. Разделить frame settled, GPU submission и presented frame.
Полезность — в проверяемом протоколе, не в обещании универсального AI tester.

### Strek: semantic commands и общий путь undo/export

**Источник:** [Introducing Strek](https://helgesver.re/articles/introducing-strek),
индексная дата 2 августа; текст описывает разработку 29 июля — 1 августа.
Это пример редактора с semantic command API, общим документом и undo,
background operation без перехвата focus и backend-neutral display list.
Editor overlays исключаются из export output.

**Для FLUI:** agent action и pointer action должны встречаться в одном
domain operation, иначе undo/validation начинают расходиться. Focus не должен
меняться только из-за обращения automation. Отделение document paint от editor
overlays полезно для export и screenshot tests. Статья не доказывает зрелость
продукта или необходимость встроить document model в сам framework.

### wgpu-profiler 0.28 и кейс Ruffle: обязательства drain на каждом backend

**Источники:** [версии wgpu-profiler](https://crates.io/crates/wgpu-profiler/versions),
0.28.0 — 31 июля; [Ruffle issue](https://github.com/ruffle-rs/ruffle/issues/24605),
4 сентября, на момент чтения closed. Отчёт описывает рост памяти около 10 MB/s
в определённом WebGL2/Electron workload: scopes записывались и при disabled
GPU timer queries, а `end_frame` пропускался на web пути. Это наблюдение автора
issue, не наш локальный benchmark.

**Для FLUI:** версия 0.28 уже в workspace; сочетание gpu-profiler+wasm явно
отвергается при компиляции. Поэтому это не установленная wasm утечка FLUI.
Практический вывод для разрешённых backends: ownership frame obligations должен
охватывать begin/end/resolve/process, failed frame, aborted submission и
несколько flushes. Выключенные queries не обязательно означают нулевую очередь
CPU profiling data; проверить bounded retention отдельно.

## WebGPU и browser-facing практика

### Compose Multiplatform 1.12: visual bounds, settle и semantics

**Источник:** [официальный release](https://github.com/JetBrains/compose-multiplatform/releases/tag/v1.12.0),
25 августа. `LayerOutsets` позволяют offscreen layer выходить за measured bounds;
testing idle учитывает composition, measure/layout и pending draws. Есть fixes
RTL caret при newline, emoji/combining sequences, invalid composing region,
accessibility merged focusable nodes и iOS VoiceOver LiveRegion.

**Для FLUI:** три конкретных архитектурных вопроса. Measured size не должен
неявно обрезать paint/shadow bounds. Settled frame требует явного определения
при остановленном virtual clock: наличие pending draw не равно отсутствию работы.
Merged semantics должны сохранять корректную focus navigation и announcements.
Текстовые fixes полезны как acceptance cases для нашего TextStore. Compose MCP
Hot Reload упомянут в release, но не заменяет native semantics или pixel checks.

### Chrome 151/152: WGSL subgroup size и capability negotiation

**Источник:** [официальные WebGPU notes](https://developer.chrome.com/blog/new-in-webgpu-151-152),
12 августа. Расширение WGSL `subgroup_size` опирается на запрос capability;
adapter сообщает min/max sizes, значение по умолчанию остаётся driver-dependent.
Изменено исключение для некорректных offset/size в `setImmediates` на OperationError.

**Для FLUI:** полезно для будущих kernels: shader specialization нельзя привязать
к одной subgroup width на всех adapters. Feature detection и fallback должны
проверяться независимо. Browser exception naming касается web bridge/diagnostics,
а не доказывает наличие соответствующего API в текущем wgpu abstraction.

### Safari 27: WebGPU feature probes и пределы browser automation

**Источник:** [WebKit features](https://webkit.org/blog/18325/webkit-features-for-safari-27-0/),
17 сентября. Описаны WebGPU `clip_distances` и Safari MCP/automation tooling.
Доступ к новой GPU feature всё равно зависит от adapter capability.

**Для FLUI:** DOM automation не сможет само прочитать внутренние widgets canvas
renderer; нужен semantics/agent protocol snapshot и проверка actual pixels.
Это материал для browser smoke matrix и взаимодействия native/web automation,
а не основание считать браузерную поддержку feature универсальной.

## Предлагаемые следующие задачи

Это предложения для отдельных изменений; исследование не изменяет roadmap.

1. **Проверить обновление AccessKit Unix/AT-SPI.** Согласовать весь набор adapters,
   затем показать enabled/disabled/Image через реальный Linux accessibility client.
2. **Расширить live-матрицу text input.** Добавить корейскую композицию,
   surrounding deletion, autocorrect и selection со screen reader. Web и native
   получают отдельные сценарии и доказательства.
3. **Описать границы headless harness.** Разделить fixed-font golden coverage,
   system fallback, native IME и assistive technology. Для одной сцены сравнить
   headless и live при выбранных fonts/DPR.
4. **Проверить внешний большой consumer.** Через facade собрать экран с popup,
   shortcuts, multiline editor и списком; зафиксировать compile time и поведение.
5. **Измерить CPU hotspot до SIMD pilot.** Если hotspot найден, сравнить scalar
   и precise SIMD на одинаковом workload, включая edge cases и binary size.
6. **Проверить toolchain policy целиком.** Для 1.97–1.99 сверить Clippy additions,
   moved/deprecated rules, Cargo warning policy, effective profiles и feature
   graph; учитывать существующие allow и overrides, не создавать их копии.
7. **Провести отдельный nightly compatibility probe.** Typed arity, GAT,
   derives и consumer compile-fail cases проверить с новыми solvers; измерить
   compile time. Результат не меняет stable MSRV и не вводит нестабильные API.
8. **Проверить engine failure/retention cases.** Profiler drain, fallible glyph
   replay, transparent filters и target initialization — отдельные observable
   cases, не один assertion об успешном frame.
9. **Review SDK capabilities и automation assertions.** Проверить realm/lifecycle
   связи, exact subject selection и test mutation: disabled production action
   обязана сделать сценарий красным.

## Как проверялись источники и где остаются пробелы

Keenable использован для четырёх групп поиска: Rust UI, GPU/text,
accessibility/platform input и testing/toolchain. Затем выполнены уточняющие
поиски Slint, egui, AccessKit, winit и Linebender. Firecrawl использован для
независимого поиска и чтения официальных страниц Slint/Linebender и release notes.
GitHub API подтвердил timestamps egui, Slint, AccessKit, nextest, Fearless SIMD,
Compose и Vello. Crates.io API использован для версий Taffy, AnyRender, Animato,
Blitz, Subsecond, Crownshell, edev, ps-qa и wgpu-profiler. Язык, Cargo и Clippy
сверены с официальными release notes/changelogs; отдельно прочитаны материалы
по API design, compiler/rustdoc performance и новым solvers.
Индексные даты сами по себе не считались датами обновления README.
После ответа Keenable о rate limit повторные вызовы не делались; дополнительные
страницы прочитаны через Firecrawl и GitHub API. Неудачные/пустые fetches
не использованы как подтверждение API или дат.

В выборку не включены как новые релизы периода:

- [Slint 1.17](https://slint.dev/blog/slint-1.17-released): официальный релиз
  **24 июня**, хотя [обзор](https://decebaldobrica.com/blog/2026-07-08-slint-1-17-rust-desktop-ready-evaluation)
  вышел 8 июля. MCP, tray и DnD из этого релиза — полезный фон, но не июльская новинка.
- [Linebender in 2026 Q1](https://linebender.org/blog/tmil-25/): **19 апреля**,
  несмотря на августовскую дату у найденного агрегатора.
- winit forks с именами quickgui-winit/hilen-winit не считались upstream релизом
  winit. Свежесть upstream API требует отдельного подтверждения.

Для Iced не найден достаточно сильный датированный первичный материал для
отдельной рекомендации; это пробел выборки, а не доказательство отсутствия
развития проекта. Для Dioxus экосистемы добавлены Taffy, AnyRender, Blitz и
Subsecond с registry dates и отдельно прочитанной документацией. Полный аудит
GitHub commits каждого framework не проводился. У Fenestra/netrender даты появления
пакетов подтверждены только registry index; текущая документация прочитана
отдельно и не приписана старому релизу.

Скорость, correctness и platform support внешних проектов не проверялись
локальными сборками или устройствами. Все шаги внедрения требуют собственных
поведенческих тестов FLUI; cross-crate contract оформляется ADR по правилам проекта.
