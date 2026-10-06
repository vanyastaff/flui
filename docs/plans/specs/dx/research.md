# dx — реестр болей DX у UI-фреймворков и позиция FLUI (исследование)

- **Статус:** исследование
- **Дата:** 2026-10-05
- **База:** `main` @ `4915054c8`
- **Уровень 0:** [../release/requirements.md](../release/requirements.md) (R13 измерения, R17 docs, R18 три стиля)
- **Цель владельца (2026-10-05):** самый удобный DX среди UI-библиотек, без повторения известных
  болей Flutter, Slint, egui, Leptos (для контраста: Dioxus, Iced, Xilem, GPUI, Compose, SwiftUI).
  Никакой разметки (`rsx!`/`view!`/JSX, отдельный DSL): дерево — обычные Rust-значения в трёх
  стилях (макрос `column![…]`, struct-литерал `Column { … }`, builder `Column::new(…)`).

## Метод и обозначения

Источники: issue с наибольшим числом реакций (`gh search issues --repo <repo> --sort reactions`,
снято 2026-10-05; `r=` — сумма реакций, `c=` — комментарии), официальная документация,
признающая ограничение, посты мейнтейнеров, опросы. Каждая строка реестра имеет ссылку; что
не удалось проверить, помечено «не проверено». Код FLUI прочитан в этой ветке (сборок не было):
ссылки `файл:строка` и имена тестов.

FLUI сейчас: **лучше** — боль снята конструкцией; **частично**; **повторяет**; **неизвестно**.
Спека: владелец среди authoring-styles, focus-keyboard, text-ime, persistence, render-proof,
teardown, send-flip, measurements, docs-community, a11y-native (R12) — или **новая**.

## Сводная таблица

| # | Боль | У кого | Доказательство | FLUI сейчас | Предлагаемое требование | Спека |
|---|------|--------|----------------|-------------|--------------------------|-------|
| P1 | Вложенность и шум в дереве: имя контейнера повторяется, дерево трудно читать | Flutter | [flutter#15922](https://github.com/flutter/flutter/issues/15922) «JSX-like» r=292 | **повторяет**: `Column::new(column![…])` — два имени на один узел (`examples/counter.rs:53`, `examples/two_screens/tree.rs:191`) | КОГДА автор пишет контейнер с детьми макросом, СИСТЕМА ДОЛЖНА возвращать сам виджет (`column![a, b].main_axis_alignment(…)`) без внешнего `Column::new`; тест: дерево из `column![…]` равно дереву из `Column::new(…)` | authoring-styles |
| P2 | Обещанный стиль не компилируется (документация расходится с API) | FLUI | — | **повторяет**: doc `crates/flui-view/src/macros/mod.rs:9-11` показывает `Column { children: … }`, но поля приватны (`crates/flui-widgets/src/flex/flex.rs:246-249`) | КОГДА виджет facade описан struct-литералом с `..Default::default()`, СИСТЕМА ДОЛЖНА его компилировать; доктест на каждый виджет facade | authoring-styles |
| P3 | Обрыв по числу детей | SwiftUI (10 детей до Swift 5.9) | [SE-0393](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0393-parameter-packs.md); [описание ошибки «Extra argument in call»](https://www.hackingwithswift.com/swift/5.9/variadic-generics) | **частично**: обрыв на 17 детях, но с понятной ошибкой (`macros/mod.rs:143`, fixture `crates/flui-view/tests/ui/column_17_compile_error.rs`) | КОГДА `column!`/`row!` получает больше 16 детей, СИСТЕМА ДОЛЖНА компилировать дерево без правок автора (макрос сам вкладывает кортежи); тест на 40 детей | authoring-styles |
| P4 | Условные и разнотипные дети требуют стирания типа | Leptos (`Either`, `into_any()`) | [leptos#4320](https://github.com/leptos-rs/leptos/issues/4320); `into_any()` как обход времени сборки в [leptos#3489](https://github.com/leptos-rs/leptos/issues/3489) | **повторяет**: ветки `if` требуют `.boxed()` (`examples/two_screens/tree.rs:203`, `:208`); `impl View for Option<V>` нет (rg по `crates/` пуст) | КОГДА ребёнок условный (`Option<V>`) или ветки `if/else`/`match` разных типов, СИСТЕМА ДОЛЖНА принимать его в `column![…]` без `.boxed()`; компилируемый тест | authoring-styles |
| P5 | Бойлерплейт состояния и невозможность переиспользовать логику жизненного цикла | Flutter | [flutter#51752](https://github.com/flutter/flutter/issues/51752) r=313, c=555 | **повторяет**: счётчик — 68 строк, 3 типа и 4 `impl` (`examples/counter.rs`): `StatefulView` + `create_state` + `ViewState` + `init_state` | КОГДА `flui create --template counter` создаёт проект, СИСТЕМА ДОЛЖНА уложить счётчик в один тип состояния и один `impl` (замер: ≤ 30 значащих строк); логика «сигнал + контроллер + задача» выносится в функцию, вызываемую из `init_state`, и тест переиспользует её в двух виджетах | новая |
| P6 | Фрагментация моделей состояния | Flutter; Iced (Message-enum) | [Flutter: варианты state management](https://docs.flutter.dev/data-and-backend/state-mgmt/options); [iced#662](https://github.com/iced-rs/iced/issues/662) «Why messages over callbacks?»; [Levien 2022](https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html): Elm-сообщения «verbose» | **повторяет**: `Signal` рядом с `ChangeNotifier`-контроллерами на `Arc<Mutex<…>>` (`crates/flui-widgets/src/text/controller.rs:256-262`), `AsyncSnapshot` в `FutureBuilder` | КОГДА виджет каталога выставляет изменяемое состояние (текст, страница, прокрутка), СИСТЕМА ДОЛЖНА давать его читать как `Signal` в `build` с перестройкой при изменении; тест на `TextEditingController` | новая |
| P7 | `let x = x.clone(); move || …` перед каждым колбэком | Rust GUI вообще (Dioxus, Sycamore) | [проектная цель Rust «ergonomic ref-counting»](https://rust-lang.github.io/rust-project-goals/2025h1/ergonomic-rc.html); [RFC 3680](https://github.com/rust-lang/rfcs/pull/3680) | **частично**: `Signal` — `Copy` (`crates/flui-foundation/src/read_scope.rs:301-313`), но роутер, контроллеры и общие структуры клонируются: 11 клонов в `examples/two_screens/tree.rs` (`:134`…`:283`), 10 в `examples/material_demo/tree.rs` | КОГДА колбэк захватывает дескриптор каталога (signal, router, controller), СИСТЕМА ДОЛЖНА давать `Copy`-дескриптор; замер: 0 строк `let x = x.clone();` в Notes и tutorial | authoring-styles |
| P8 | Колбэк, связанный через `let`, не выводит тип параметра; rustc советует не то | Rust | [rust#58052](https://github.com/rust-lang/rust/issues/58052) «Annotating higher-ranked lifetimes on closures is arduous» (open) | **повторяет**: fixture `crates/flui-view/tests/ui/let_bound_event_closure_without_helper.stderr` — E0631, подсказка rustc «wrap in a closure», а не `callback(…)` | КОГДА колбэк объявлен через `let` без `callback(…)`, СИСТЕМА ДОЛЖНА либо компилировать его, либо давать ошибку, которая называет `callback`; проверка — `.stderr` фикстуры (гипотеза: свой трейт-бound с `#[diagnostic::on_unimplemented]`) | authoring-styles |
| P9 | Ошибки раскладки только во время выполнения: переполнение flex, неограниченная ось | Flutter | [Flutter common errors](https://docs.flutter.dev/testing/common-errors): «RenderFlex overflowed», «Vertical viewport was given unbounded height», «RenderBox was not laid out»; [flutter#18711](https://github.com/flutter/flutter/issues/18711) | **повторяет, хуже — молча**: flex-дети под неограниченной осью тихо становятся негибкими (`crates/flui-objects/src/layout/flex.rs:529-535`), переполнение тихо обрезает свободное место (`flex.rs:658-660`); `overflow_indicator` без вызовов (`crates/flui-painting/src/paint/effects.rs:719`); «Known gap: no debug overflow indicator» (`crates/flui-objects/src/layout/constraints_transform_box.rs:24`) | КОГДА в debug-сборке flex переполнен или flex-ребёнок попал под неограниченную ось, СИСТЕМА ДОЛЖНА один раз на узел выдать структурированную диагностику с путём виджетов от корня и подсказкой, и рисовать индикатор переполнения; тест через `log_capture` и `has_visual_overflow` | новая |
| P10 | Parent-data виджет не под тем родителем (`Expanded` вне `Flex`) | Flutter | [Flutter common errors](https://docs.flutter.dev/testing/common-errors): «Incorrect use of ParentData widget» | **повторяет**: проверка во время выполнения (`crates/flui-view/src/view/view.rs:686-737`, `parent_data_typical_ancestor_description`), `Expanded` принимает любого родителя (`crates/flui-widgets/src/flex/flexible.rs:103`) | КОГДА `Expanded`/`Flexible` стоит не ребёнком `Row`/`Column`/`Flex`, СИСТЕМА ДОЛЖНА отказывать при компиляции; trybuild-фикстура | новая |
| P11 | `BuildContext` после async-разрыва | Flutter | [lint use_build_context_synchronously](https://dart.dev/tools/linter-rules/use_build_context_synchronously); [dart-lang/sdk#58744](https://github.com/dart-lang/sdk/issues/58744) | **лучше**: `build` получает `&dyn BuildContext` (заём, не уходит в `'static`-future); запись только через `EventCx` — fixtures `signal_write_through_build_context`, `signal_write_without_a_writer`; ёмкости только в `LifecycleContext` (ADR-0078) | КОГДА автор захватывает контекст `build` в future или колбэк, СИСТЕМА ДОЛЖНА отказывать при компиляции; trybuild-фикстура «ctx в spawned future» | teardown |
| P12 | Запись в состояние размонтированного виджета / отмена задач | Flutter, Leptos | [leptos#582](https://github.com/leptos-rs/leptos/issues/582) «signal after it was disposed»; Flutter `setState() called after dispose()` — не проверено ссылкой | **частично**: drop `TaskToken` отменяет задачу (`crates/flui-view/src/context/build_context.rs:410-421`); но `Signal::default()` — несвязанный заполнитель (`read_scope.rs:335`), а `get` паникует на ошибке (`read_scope.rs:631`) | КОГДА state размонтирован, СИСТЕМА ДОЛЖНА отменять его задачи до освобождения и превращать запись в освобождённый сигнал в диагностику, не в панику; КОГДА сигнал прочитан до `init_state`, ошибка должна называть виджет и поле | teardown |
| P13 | Идентичность и ключи: потеря состояния при перестановке | Flutter, egui (ID clash) | [Flutter Key](https://api.flutter.dev/flutter/foundation/Key-class.html): «common mistake… incorrect State object»; [egui#4265](https://github.com/emilk/egui/issues/4265), [egui#4940](https://github.com/emilk/egui/issues/4940) | **повторяет** модель Flutter (`crates/flui-foundation/src/key.rs:91`, `:452`; `crates/flui-view/src/tree/id_reconcile.rs`); с ключом перестановка покрыта (`object_keys_follow_retained_allocations_through_reorder`), без ключа диагностики нет | КОГДА динамический список stateful-детей без ключей меняет порядок, СИСТЕМА ДОЛЖНА в debug предупреждать с путём; ленивый список требует функцию ключа в сигнатуре builder'а | новая |
| P14 | Цена макро-DSL: rustfmt, rust-analyzer, ошибки внутри раскрытия | Leptos, Dioxus, Slint | [leptosfmt](https://github.com/bram209/leptosfmt) (rustfmt не форматирует `view!`); [leptos#1310](https://github.com/leptos-rs/leptos/issues/1310) (автодополнение в макросах); [dioxus#3030](https://github.com/DioxusLabs/dioxus/issues/3030) (`dx fmt` съедает комментарии); [slint#685](https://github.com/slint-ui/slint/issues/685), [slint#12096](https://github.com/slint-ui/slint/issues/12096) | **лучше (по устройству)**: `column!` — `macro_rules!` над `expr`, раскрывается в кортеж; по [Rust Style Guide](https://doc.rust-lang.org/style-guide/expressions.html#macro-uses) такой вызов форматируется как обычная конструкция. Локально не прогонялось | КОГДА дерево записано в `column![…]`/`row![…]`, СИСТЕМА ДОЛЖНА форматироваться штатным `cargo fmt` и давать автодополнение rust-analyzer внутри; тест: неотформатированная фикстура меняется `cargo fmt` | authoring-styles |
| P15 | Два языка: `.slint` или HTML/CSS рядом с Rust | Slint, Dioxus, Leptos, Tauri | [slint#1726](https://github.com/slint-ui/slint/issues/1726) r=28 (импорт Rust-структур в `.slint`); [dioxus#2180](https://github.com/DioxusLabs/dioxus/issues/2180) (Tailwind) | **лучше**: только Rust-значения (R18) | КОГДА пишется приложение Notes, СИСТЕМА НЕ ДОЛЖНА требовать файлов кроме `.rs` и `Cargo.toml`; проверка состава tutorial | authoring-styles |
| P16 | Пределы immediate mode: однопроходная раскладка, ID | egui | [README egui](https://github.com/emilk/egui) (раздел о недостатках immediate mode); [egui#843](https://github.com/emilk/egui/issues/843) r=30 | **лучше**: retained-дерево, constraints-down/sizes-up | — (контраст, требование не нужно) | — |
| P17 | Время сборки и итерации | Rust вообще, Iced, Leptos | [State of Rust 2024](https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results/): медленная компиляция — главная проблема; [iced#638](https://github.com/iced-rs/iced/issues/638), [iced#849](https://github.com/iced-rs/iced/issues/849); [leptos#3489](https://github.com/leptos-rs/leptos/issues/3489) | **неизвестно для потребителя**: ADR-0096 — правка приложения 1.4–2.9 с, правка `flui-widgets` 3.5–3.9 с, холодная сборка 145.8 с, exe 19.7 MB (Windows, dev); `docs/BETA.md:518` — холодная сборка потребителя 69 с | КОГДА изменена одна строка в Notes, СИСТЕМА ДОЛЖНА пересобирать и линковать в опубликованный бюджет на эталонной машине; отчёт содержит холодную сборку и размер release-бинаря | measurements |
| P18 | Качество hot reload | Flutter, Dioxus, Iced | [flutter#53041](https://github.com/flutter/flutter/issues/53041) r=1314; [Flutter hot reload: ограничения](https://docs.flutter.dev/tools/hot-reload); [dioxus#4160](https://github.com/DioxusLabs/dioxus/issues/4160) r=39; [iced#21](https://github.com/iced-rs/iced/issues/21) r=31 | **частично**: `flui-hot-reload` (ADR-0094, Subsecond), но вне обещаний 0.2.0 (release «Вне scope») | КОГДА README описывает цикл правки, СИСТЕМА ДОЛЖНА честно назвать: hot reload экспериментальный, опубликовано время «правка → окно» без него | docs-community |
| P19 | IME на desktop | Iced, Floem, egui; Flutter Windows | [boringcactus 2025](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html): IME не включается в Iced/Floem, в egui Tab ломает композицию; [flutter#191194](https://github.com/flutter/flutter/issues/191194) | **неизвестно до сертификации**: TSF-путь (ADR-0090) | R7 | text-ime |
| P20 | Доступность (скринридер) | Iced, egui, Xilem | [iced#552](https://github.com/iced-rs/iced/issues/552) r=57 (open); [egui#167](https://github.com/emilk/egui/issues/167); [boringcactus 2025](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html) (Iced «nope», Xilem — неверные позиции); [Xilem 2024](https://linebender.org/blog/xilem-2024/) | **повторяет по умолчанию**: `a11y` выключена в фичах по умолчанию (`Cargo.toml:753`, `:788`); у eframe AccessKit включён по умолчанию (README egui) | КОГДА потребитель собирает `flui` с фичами по умолчанию, СИСТЕМА ДОЛЖНА публиковать дерево доступности; тест на граф фич facade + ручной прогон Narrator | a11y-native |
| P21 | Клавиатурный фокус и Tab | Iced | [iced#489](https://github.com/iced-rs/iced/issues/489) r=31 | **неизвестно до R8** | R8 | focus-keyboard |
| P22 | Темы и стилизация | egui, Slint, Iced | [egui#3284](https://github.com/emilk/egui/issues/3284) r=83 «CSS-like styling»; [slint#45](https://github.com/slint-ui/slint/issues/45); [iced#1022](https://github.com/iced-rs/iced/issues/1022) «System themes» | **неизвестно**: Material `ThemeData` по образцу Flutter (`packages/flui-material/src/theme_data.rs:710`); следование теме ОС не найдено | КОГДА ОС переключает светлую/тёмную тему, СИСТЕМА ДОЛЖНА перестраивать тему без перезапуска; КОГДА автор меняет один токен темы, СИСТЕМА ДОЛЖНА менять его у всех виджетов пакета без форка; тесты на realm | новая |
| P23 | Асинхронная загрузка: future пересоздаётся на каждом build | Flutter | [FutureBuilder](https://api.flutter.dev/flutter/widgets/FutureBuilder-class.html): «must not be created during … build» | **лучше**: `FutureBuilder::keyed`, фабрика вызывается раз на подписку (`crates/flui-view/src/element/future_builder.rs:60-98`), drop токена отменяет | КОГДА ключ не изменился, повторный build НЕ ДОЛЖЕН перезапускать загрузку; при смене ключа старая задача отменяется; тест считает вызовы фабрики (Notes, R6 Retry) | persistence |
| P24 | Тестирование UI: высокоуровневые действия и golden между ОС | Flutter | [matchesGoldenFile](https://api.flutter.dev/flutter/flutter_test/matchesGoldenFile.html): шрифты дают разные golden на разных ОС | **частично**: `pump_widget`, `find_text` есть (`crates/flui-testing/src/widgets.rs:1185`, `:1374`), но нажатие только по координатам `dispatch_pointer_down(x, y)` (`:1495`), `tap(find)`/`enter_text` нет; снимки слоёв `tests/demo_layer_snapshots.rs` | КОГДА тест Notes нажимает кнопку и вводит текст, СИСТЕМА ДОЛЖНА позволять это по finder'у (текст, ключ, семантическая роль) без координат; пиксельные эталоны — с фиксированным шрифтом из фикстур | новая (пиксели — render-proof) |
| P25 | Документация и онбординг | GPUI, Xilem, Leptos, Dioxus, egui | [README GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui): учиться по исходникам Zed; [Xilem 2024](https://linebender.org/blog/xilem-2024/): документация — постоянный отзыв; [leptos#2141](https://github.com/leptos-rs/leptos/issues/2141); [dioxus#4011](https://github.com/DioxusLabs/dioxus/issues/4011) «Death by 1000 papercuts»; [egui#186](https://github.com/emilk/egui/issues/186) | **повторяет**: `docs/getting-started.md` описывает клон репозитория (Python 3.10+, `cargo xtask`), а не путь потребителя; book ещё нет (R17) | R17: tutorial воспроизводит Notes с нуля на опубликованной версии; КОГДА новичок идёт по tutorial, ни один шаг не требует checkout FLUI | docs-community |
| P26 | Сообщения об ошибках и восстановление | Flutter | [flutter#29075](https://github.com/flutter/flutter/issues/29075) (стек RENDERING LIBRARY) | **частично**: `ErrorView` (`crates/flui-view/src/view/error.rs:177`), PANIC-POLICY; путь виджета в диагностике не проверен | КОГДА `build` паникует, СИСТЕМА ДОЛЖНА показать `ErrorView` на месте поддерева, напечатать путь виджетов и строить следующий кадр (R11) | teardown |
| P27 | «Почему перестроилось?» | Dioxus, Compose | [dioxus#475](https://github.com/DioxusLabs/dioxus/issues/475); [Compose stability](https://developer.android.com/develop/ui/compose/performance/stability) | **неизвестно** | КОГДА включена отладка перестроек, СИСТЕМА ДОЛЖНА называть сигнал или зависимость, перестроившую виджет; тест через `SignalProbe` | новая |
| P28 | Лицензия | Slint | [Slint pricing](https://slint.dev/pricing): GPLv3, royalty-free кроме embedded, платные планы | **лучше**: `MIT OR Apache-2.0` (`Cargo.toml:178`), `cargo xtask deps` проверяет лицензии графа | КОГДА выходит релиз, СИСТЕМА ДОЛЖНА проходить cargo-deny по лицензиям всего графа facade | docs-community |
| P29 | Многооконность | Flutter | [flutter#30701](https://github.com/flutter/flutter/issues/30701) r=727 | вне 0.2.0 (ADR-0091 Proposed) | — (честно назвать в release notes) | docs-community |

## Flutter

- **Читаемость дерева (P1).** [flutter#15922](https://github.com/flutter/flutter/issues/15922)
  (r=292) просит JSX, потому что у вложенных конструкторов нет «закрывающего тега». Мы не идём
  в разметку (решение владельца), но берём суть жалобы: меньше имён и скобок на узел.
  `Column::new(column![…])` повторяет имя дважды — ровно тот шум, на который жалуются.
- **Состояние (P5, P6).** [flutter#51752](https://github.com/flutter/flutter/issues/51752)
  (r=313, 555 комментариев, открыт) — переиспользование логики `State` с жизненным циклом
  (контроллер создать в `initState`, обновить в `didUpdateWidget`, освободить в `dispose`)
  либо многословно, либо невозможно. [Официальная страница](https://docs.flutter.dev/data-and-backend/state-mgmt/options)
  перечисляет `setState`, `ValueNotifier`/`InheritedNotifier`, `InheritedWidget`/`InheritedModel`
  и отсылает к пакетам pub.dev — это и есть фрагментация. Отмена макросов Dart (январь 2025,
  [анонс](https://medium.com/dartlang/an-update-on-dart-macros-data-serialization-06d3037d4f12);
  WebFetch получил 403, текст подтверждён по зеркалу dart.cn) оставила data-классы и
  генерацию кода на `build_runner` ([dart-lang/language#314](https://github.com/dart-lang/language/issues/314),
  открыт). У FLUI `#[derive]` закрывает эту часть, но `StatefulView`/`ViewState` — та же форма
  из двух типов, что у Flutter.
- **Ошибки раскладки (P9, P10).** Страница [common errors](https://docs.flutter.dev/testing/common-errors)
  открывается «RenderFlex overflowed», «RenderBox was not laid out», «unbounded height»,
  «Incorrect use of ParentData widget», «setState called during build». Flutter хотя бы громко
  сообщает. FLUI в flex не падает, но и молчит: неограниченная главная ось делает
  flex-детей негибкими (`flex.rs:529-535`), переполнение не рисуется. Для новичка это хуже:
  обрезанный UI без единой строки в логе.
- **Async-разрыв (P11, P23).** Нужен lint [use_build_context_synchronously](https://dart.dev/tools/linter-rules/use_build_context_synchronously)
  и проверка `mounted`, а у `FutureBuilder` есть [правило](https://api.flutter.dev/flutter/widgets/FutureBuilder-class.html),
  что future нельзя создавать в `build`. У FLUI первое закрыто системой типов, второе —
  ключом и фабрикой.
- **Ключи (P13).** В [документации Key](https://api.flutter.dev/flutter/foundation/Key-class.html)
  прямо назван «common mistake»: к unkeyed `StatefulWidget` цепляется чужой `State`.
- **Hot reload (P18).** [flutter#53041](https://github.com/flutter/flutter/issues/53041)
  (r=1314) — самый популярный DX-запрос; в [документации](https://docs.flutter.dev/tools/hot-reload)
  перечислены случаи, где нужен restart (`initState`, `main`, generic-типы, enum ↔ class, static).
- **Golden (P24).** [matchesGoldenFile](https://api.flutter.dev/flutter/flutter_test/matchesGoldenFile.html):
  эталон со шрифтами с Windows отличается от эталона с другой ОС.
- **Опросы.** [Q2 2026](https://flutter.dev/blog/flutter-q2-2026-survey): зрелость платформы и
  совместимость версий 44 %, инструменты и IDE 33 %, баги и стабильность 24 %.
  [Q1 2020](https://flutter.dev/blog/what-are-the-important-difficult-tasks-for-flutter-devs-q1-2020-survey-results):
  самые трудные задачи — платформенные проблемы, память, CPU, jank; UI-задачи — легче.
  Вывод для нас: диагностика и профилирование важнее синтаксиса.

## Slint

- **Два языка (P15).** UI на `.slint`, логика на Rust; типы приходится дублировать
  ([slint#1726](https://github.com/slint-ui/slint/issues/1726), r=28, открыт).
- **Макрос `slint!` и rust-analyzer (P14).** Мейнтейнеры признают, что дефисы в
  идентификаторах ломаются под rust-analyzer ([slint#685](https://github.com/slint-ui/slint/issues/685)),
  а макрос заново разбирает библиотеку виджетов при каждом раскрытии — около 31 % времени
  раскрытия ([slint#12096](https://github.com/slint-ui/slint/issues/12096)).
- **Стилизация (P22).** [slint#45](https://github.com/slint-ui/slint/issues/45) «Styling» открыт.
- **Лицензия (P28).** [Тарифы](https://slint.dev/pricing): GPLv3 или royalty-free (без embedded),
  embedded — роялти за устройство. FLUI — `MIT OR Apache-2.0`.
- **Сильные стороны (для честности).** По [boringcactus 2025](https://www.boringcactus.com/2025/04/13/2025-survey-of-rust-gui-libraries.html)
  Slint проходит Narrator и японский IME. Это планка, которую должен взять R7/R12.

## egui

- **Immediate mode (P16).** [README](https://github.com/emilk/egui) признаёт: размер узнаётся
  только после размещения, поэтому есть мерцание первого кадра или второй проход
  (`request_discard`). Подробный разбор — [egui#843](https://github.com/emilk/egui/issues/843) (r=30).
- **ID (P13).** Одинаковые заголовки `collapsing` ломают состояние
  ([egui#4265](https://github.com/emilk/egui/issues/4265)), двойной ID у контекстного меню
  ([egui#4940](https://github.com/emilk/egui/issues/4940)).
- **Стилизация (P22).** [egui#3284](https://github.com/emilk/egui/issues/3284) «CSS-like styling»
  (r=83) — самый популярный открытый запрос.
- **A11y и IME (P19, P20).** AccessKit включён в eframe по умолчанию (README);
  [egui#167](https://github.com/emilk/egui/issues/167). В тесте boringcactus Tab крадёт состояние
  конвертера IME.

## Leptos

- **Макрос `view!` (P14).** rustfmt его не форматирует, нужен отдельный
  [leptosfmt](https://github.com/bram209/leptosfmt). Мейнтейнер советует отключать
  proc-macro в rust-analyzer ради автодополнения ([leptos#1310](https://github.com/leptos-rs/leptos/issues/1310)).
- **Стирание типов и время сборки (P4, P17).** Ветви требуют `Either`/`into_any()`
  ([leptos#4320](https://github.com/leptos-rs/leptos/issues/4320)); в
  [leptos#3489](https://github.com/leptos-rs/leptos/issues/3489) собраны долгие сборки,
  ошибки линкера, лимиты рекурсии и тот же обход — `into_any()`, разбиение крейтов.
- **Copy-сигналы с арены (P12).** Удобны в замыканиях, но дают ошибку «signal after it was
  disposed» во время выполнения ([leptos#582](https://github.com/leptos-rs/leptos/issues/582)).
  У FLUI `Signal` устроен так же (`Copy`-слот, `SignalError::Released`/`Unbound`), поэтому
  сообщения об этих ошибках должны указывать на виджет.
- **Онбординг (P25).** [leptos#2141](https://github.com/leptos-rs/leptos/issues/2141): примеры
  вне workspace (rust-analyzer их не видит), не сказано, что нужен nightly.
- HTML-авторинг — противоположность R18; Leptos здесь только для сравнения.

## Dioxus (контраст)

- [dioxus#4011](https://github.com/DioxusLabs/dioxus/issues/4011) «Death by 1000 papercuts» —
  разбор первого опыта опытного Rust-разработчика.
- `rsx!` и форматтер: `dx fmt` удаляет закомментированные узлы
  ([dioxus#3030](https://github.com/DioxusLabs/dioxus/issues/3030)).
- Hot-patching работает не для всех крейтов workspace
  ([dioxus#4160](https://github.com/DioxusLabs/dioxus/issues/4160), r=39).
- Запрос «почему перерисовался компонент» ([dioxus#475](https://github.com/DioxusLabs/dioxus/issues/475)).
- Клонирование в замыканиях: блог Dioxus Labs запустил цель Rust
  [ergonomic ref-counting](https://rust-lang.github.io/rust-project-goals/2025h1/ergonomic-rc.html)
  ([RFC 3680](https://github.com/rust-lang/rfcs/pull/3680), синтаксис `use ||`). Ждать
  язык мы не можем: в стабильном Rust помогают только `Copy`-дескрипторы.
- Сильная сторона: в тесте boringcactus 2025 прошёл и скринридер, и IME.

## Iced (контраст)

- Elm-архитектура: на каждую кнопку — вариант `Message`, ветка `update` и только потом
  код ([iced#662](https://github.com/iced-rs/iced/issues/662)); Левиен называет это «verbose»
  ([2022](https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html)).
- A11y открыт ([iced#552](https://github.com/iced-rs/iced/issues/552), r=57), выделение текста
  открыто ([iced#36](https://github.com/iced-rs/iced/issues/36), r=55), фокус и Tab не работают
  ([iced#489](https://github.com/iced-rs/iced/issues/489), r=31), IME не включается
  (boringcactus 2025), темы ОС не подхватываются ([iced#1022](https://github.com/iced-rs/iced/issues/1022)).
- Итерация: около 10 с на правку строки в простом примере ([iced#638](https://github.com/iced-rs/iced/issues/638)),
  запрос динамической линковки ([iced#849](https://github.com/iced-rs/iced/issues/849)).

## Xilem / Masonry, GPUI (контраст)

- Xilem: [пост 2024](https://linebender.org/blog/xilem-2024/) признаёт, что документации
  не хватает, a11y долго была вторичной, IME требует большой работы; по boringcactus 2025
  Masonry/Xilem отдают скринридеру неверные позиции. [Levien 2022](https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html):
  рабочие UI-архитектуры опираются на общее изменяемое состояние, а это противоречит модели
  владения Rust.
- GPUI: [README](https://github.com/zed-industries/zed/tree/main/crates/gpui) — pre-1.0,
  частые breaking changes, учиться предлагается по исходникам Zed и в Discord.
  [Zed 2023](https://zed.dev/blog/videogame): свой рендерер ради 120 FPS — производительность
  ценой отсутствия документации.

## Compose, SwiftUI (контраст)

- Compose: [стабильность параметров](https://developer.android.com/develop/ui/compose/performance/stability)
  — нестабильный тип всегда перекомпонуется; разработчик обязан понимать модель компилятора.
  Отсюда P27: объяснять перестройку должен инструмент, а не знание внутренностей.
- SwiftUI: до Swift 5.9 `@ViewBuilder` принимал не больше 10 детей с ошибкой «Extra argument
  in call»; лимит сняли variadic generics ([SE-0393](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0393-parameter-packs.md)).
  В Rust variadic generics нет, поэтому P3 решается внутри макроса (вложенные кортежи или
  `Vec<BoxedView>`), а не вынесением выбора к автору.

## Топ-10 для релиза 0.2.0

Порядок: (1) насколько вероятно новичок встретит боль в первый час с tutorial/Notes;
(2) повторяет ли её FLUI сейчас. Все десять — «повторяет» или «частично».

| Ранг | Боль | Почему в первый час | FLUI сейчас | Спека |
|------|------|---------------------|-------------|-------|
| 1 | P9+P10 молчаливые ошибки раскладки | Первая `Column` в `ScrollView` или `Expanded` не там — UI обрезан, в логе пусто | повторяет, хуже Flutter | новая (layout-diagnostics) |
| 2 | P1+P2 двойное имя и недоступный struct-литерал | Первые строки tutorial; R18 обещает три стиля, работает полтора | повторяет | authoring-styles |
| 3 | P5 бойлерплейт счётчика | `flui create` даёт 68 строк, 3 типа и 4 `impl` | повторяет Flutter | новая (state-ergonomics) |
| 4 | P7+P6 клоны перед `move` и две модели состояния | Роутер, контроллер текста и общие данные клонируются вручную; `TextEditingController` не сигнал | частично | authoring-styles / новая |
| 5 | P4 `.boxed()` для `if` | Первая кнопка «показать, если…» | повторяет Leptos | authoring-styles |
| 6 | P8 E0631 при `let`-колбэке | Вынести колбэк в переменную — обычный рефакторинг; rustc советует не то | повторяет (зафиксировано фикстурой) | authoring-styles |
| 7 | P24 тест по координатам | Сценарий 5 R: первый widget-тест Notes | частично | новая (testing-api) |
| 8 | P20 a11y выключена по умолчанию | Приложение из tutorial без фичи недоступно скринридеру | повторяет | a11y-native |
| 9 | P17 время сборки без бюджета | 69 с холодной сборки потребителя; инкремент для потребителя не измерен | неизвестно | measurements |
| 10 | P25 онбординг через клон репозитория | `docs/getting-started.md` — путь контрибьютора, а не потребителя | повторяет | docs-community |

Следующие за десяткой: P3 (17-й ребёнок, ошибка уже понятная), P13 (ключи без диагностики),
P12 (несвязанный `Signal::default()` паникует при чтении), P22 (тема ОС).

## Где FLUI уже лучше

- **Один язык (P15).** Нет `.slint`, HTML, CSS или DSL: дерево — Rust-выражения
  (`examples/counter.rs:52-60`).
- **Retained, не immediate (P16).** Раскладка constraints-down/sizes-up без второго прохода и
  без ID-коллизий egui.
- **Контекст build не переживает async-разрыв (P11).** `build(&self, …, ctx: &dyn BuildContext)`
  (`crates/flui-view/src/view/stateful.rs:119`) — заём, его нельзя унести в `'static`-future.
  Ёмкости представления есть только у `LifecycleContext` (ADR-0078). Запись в сигнал через
  контекст build не компилируется: trybuild-фикстуры `signal_write_through_build_context`,
  `signal_write_without_a_writer` (`crates/flui-view/tests/ui/`). Flutter закрывает это
  lint'ом и проверкой `mounted`.
- **Copy-сигналы (P7, частично).** `Signal<T>: Copy` (`crates/flui-foundation/src/read_scope.rs:301-313`):
  `let count = self.count; … move |cx| count.update(cx, …)` без `clone`
  (`examples/counter.rs:51-58`). Ошибки сигналов — значения (`SignalError`), а не UB.
- **Async без перезапуска на каждом build (P23).** `FutureBuilder` с ключом и фабрикой, которая
  вызывается раз на подписку (`crates/flui-view/src/element/future_builder.rs:60-98`); drop
  `TaskToken` отменяет задачу (`crates/flui-view/src/context/build_context.rs:410-421`).
- **Макрос без разметки (P14).** `column!` — `macro_rules!` над `expr`: внутри обычный Rust для
  rustfmt и rust-analyzer. Обрыв по арности даёт свою ошибку с выходом
  (`crates/flui-view/tests/ui/column_17_compile_error.stderr`) вместо SwiftUI'ного «Extra
  argument in call». Форматирование `cargo fmt` локально не проверялось.
- **Лицензия (P28).** `MIT OR Apache-2.0` (`Cargo.toml:178`) без роялти и GPL.
- **Измеряемость (P17).** У ADR-0096 есть цифры инкрементальной сборки
  (1.4–2.9 с на правку приложения); у Iced и Leptos — только жалобы в issue.

## Что не проверено

- Сводные числа реакций сняты 2026-10-05 и будут меняться.
- Анонс отмены макросов Dart: medium вернул 403, текст подтверждён по зеркалу dart.cn.
- Flutter «setState() called after dispose()» как популярная жалоба — без отдельной ссылки
  (P12 опирается на leptos#582).
- Отдельного опроса по GUI в State of Rust 2024 нет: оттуда берётся только «медленная
  компиляция — главная проблема».
- Размер release-бинаря FLUI и инкрементальная сборка потребителя не измерены (только dev, Windows).
- Форматирование `column![…]` штатным rustfmt и автодополнение rust-analyzer внутри макроса
  выведены из стайл-гайда и устройства макроса, а не из прогона.
- Поведение FLUI при перестановке unkeyed stateful-детей не проверено тестом; вывод «повторяет»
  сделан по модели сверки из `id_reconcile.rs`.
