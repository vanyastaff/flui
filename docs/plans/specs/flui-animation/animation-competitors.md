# Animation у других UI frameworks: проверка источников и выводы для FLUI

**Дата проверки:** 2026-10-08. **Назначение:** дополнение к [аудиту](readiness-audit.md) и
[плану готовности](readiness-plan.md), не новая принятая архитектура.

Изучены animation-таблица, раздел 8.14 и утверждения о SwiftUI render server из предоставленного
`ui-frameworks-architecture (2).md`. Сам исходный документ не изменён. Его рекомендации
рассматриваются как предложения автора, а не как инструкции или утверждённые требования FLUI.
Метод Matt research: проверять тезис по документации владельца API или его исходникам;
различать core API, отдельную библиотеку и возможности browser/OS renderer.

Это выборочная проверка значимых контрактов девяти frameworks, дополненная Compose и Motion.
Она не является полным аудитом их исходников, доказательством отсутствия ненайденных функций,
измерением производительности или обещанием полного feature parity. Документация `latest`
меняется; ниже указаны версии там, где удалось привязать вывод к опубликованной версии.
GPUI проверен по исходникам Zed на `b47a4ca595d3a3fba116b9e78ef01a46e2e43f3e`.

## Что нужно исправить в исходном сравнении

| Тезис документа | Результат проверки |
|---|---|
| SwiftUI `withAnimation` интерполируется вне процесса; render server обеспечивает плавность при занятом main thread | Такое обобщение неверно. Apple отдельно описывает SwiftUI animations как вычисляемые на background thread **в процессе приложения**, без backing `CAAnimation`. WWDC25 объясняет перенос встроенной интерполяции и некоторых пользовательских вычислений с main thread; это не гарантия независимости всего кадра от main thread. [Apple: interoperability](https://developer.apple.com/documentation/swiftui/unifying-your-app-s-animations), [WWDC25: concurrency](https://developer.apple.com/videos/play/wwdc2025/266/) |
| В React анимаций «нет» | Устарело для проверенной даты: React 19.3 от 2026-09-09 стабилизировал `ViewTransition`, пока только для DOM, через browser View Transition API. При этом `useTransition` обозначает неблокирующее обновление React, а не универсальный interpolator или spring. [React 19.3](https://react.dev/blog/2026/09/09/react-19-3), [useTransition](https://react.dev/reference/react/useTransition) |
| Iced `Animation<T>` — «lilt» | По существу верно, но важно разделить слои: в Iced 0.14 это публичный core API, внутри которого находится `lilt::Animated<T, Instant>`. Это не только сторонний рецепт для приложения. [Исходник iced_core 0.14](https://docs.rs/iced_core/0.14.0/src/iced_core/animation.rs.html) |
| GPUI — только базовый easing через `with_animation` | Неполно для проверенного SHA: есть `with_spring`, сохранение состояния по element ID, playback states, reduced-motion branch, синхронизированный repeat и ограничение частоты. Это чтение реализации, не наше тестирование её гарантий. [GPUI animation source](https://github.com/zed-industries/zed/blob/b47a4ca595d3a3fba116b9e78ef01a46e2e43f3e/crates/gpui/src/elements/animation.rs) |
| Xilem/Masonry: `request_anim_frame` → `AnimFrame(interval)` | Смысл запроса следующего кадра сохраняется, имена зависят от версии. Masonry 0.4 документирует `request_anim` и `Widget::on_anim_frame`; получение animation frame само по себе не означает paint invalidation. [Masonry Widget](https://docs.rs/masonry/0.4.0/masonry/core/trait.Widget.html#method.on_anim_frame) |
| `TickerMode` останавливает время скрытого subtree | Flutter глушит tick delivery, но elapsed time продолжает идти. Это нельзя автоматически переносить на freeze/pause контракт FLUI. [AnimationController](https://api.flutter.dev/flutter/animation/AnimationController-class.html) |
| «Лучший вариант — SwiftUI модель + Flutter механизм», следовательно FLUI нужен `AnimationContext` | Это оценка и архитектурное предложение, а не следствие источников. У FLUI уже есть согласованные lifetime/time/policy требования; новый transaction API не становится условием выпуска от наличия такого API у конкурента. См. [границы плана](readiness-plan.md). |

## Девять frameworks: какой контракт действительно полезен

| Framework и слой | Проверенное поведение | Что это проверяет в FLUI |
|---|---|---|
| **Flutter — framework core** | Контроллер получает ticker от `TickerProvider`, обновляется по кадрам, требует disposal. Отмена обычного `TickerFuture` не завершает его; `orCancel` сообщает ошибку. `dispose` очищает оба вида listeners. [Controller](https://api.flutter.dev/flutter/animation/AnimationController-class.html), [dispose implementation](https://api.flutter.dev/flutter/animation/AnimationController/dispose.html) | **A1/A2/A3:** определить свои observable completion/cancellation и late-listener правила, доказать last-owner teardown; отдельно проверить mute и freeze. Ручной Dart disposal не является образцом Rust ownership. |
| **SwiftUI — framework + OS** | Transaction связывает animation context с изменением; persistent spring при замене совместимой spring сохраняет velocity. Это не универсальная гарантия C¹ для всякой timing curve. [Explore SwiftUI animation](https://developer.apple.com/videos/play/wwdc2023/10156/), [Animation.spring](https://developer.apple.com/documentation/swiftui/animation/spring) | **A4:** проверять retarget на отображаемом свойстве, включая переход от жеста. **A1/A2:** определить границу синхронного UI state; не делать произвольный user callback многопоточным ради сходства с Apple. |
| **React — core координация + browser** | `ViewTransition` связывает enter/exit/update/share с изменением дерева; custom animation event возвращает cleanup, отменяющий animation при interruption. Visual execution принадлежит browser API, а не scheduler hook `useTransition`. [ViewTransition](https://react.dev/reference/react/ViewTransition) | **A2/A6:** interruption и exit transition должны иметь владельца, cleanup и устойчивую identity. Это аргумент за lifecycle acceptance, не за DOM API или новый FLIP scope в FLUI. |
| **Dioxus — framework + renderer + ecosystem** | Официальный tutorial перечисляет CSS, `dioxus-motion` и интеграцию с системными animation API. CSS-путь в DOM/WebView нельзя превращать в гарантию всех native renderers; `dioxus-motion` — отдельная библиотека, не доказательство встроенного spring core. [Dioxus 0.7 next steps](https://dioxuslabs.com/learn/0.7/tutorial/next_steps/) | **A7/A9:** отделить capability framework от конкретного backend и пакета. В FLUI проверять runtime consumer, а не наличие математического crate. |
| **Leptos — component + browser CSS** | `AnimatedShow` задерживает unmount на `hide_delay`, применяя show/hide CSS classes; это coordination жизненного цикла, не универсальная physics/timeline система. [Leptos 0.8.20 AnimatedShow](https://docs.rs/leptos/0.8.20/leptos/control_flow/fn.AnimatedShow.html) | **A2/A6:** тестировать удаление во время exit, повторный show и отмену; различать окончание эффекта и освобождение дерева. |
| **Iced — core wrapper + runtime subscription** | `Animation<T>` хранит изменения state и проектирует значение в заданный `Instant`; доступны duration/delay/repeat/go. `window::frames()` даёт frame timestamps через Subscription; не следует подменять их произвольным timer. [Iced 0.14 API](https://docs.rs/iced/0.14.0/iced/struct.Animation.html), [core source](https://docs.rs/iced_core/0.14.0/src/iced_core/animation.rs.html), [runtime frames](https://docs.iced.rs/iced/window/fn.frames.html) | **A3:** явное время удобно для deterministic tests. **A8:** отдельно измерять интерполяцию и инвалидируемую фазу; наличие `Animation` не доказывает layer-only update. Страница runtime frames — 0.15.0-dev, не основание приписывать весь dev API версии 0.14. |
| **egui — immediate-mode core** | `animate_bool*` хранит переход по `Id` и запрашивает repaint; `animate_value_with_time` линейно меняет f32 при новом target. API не обещает spring velocity continuity. [egui 0.36.2 Context](https://docs.rs/egui/0.36.2/egui/struct.Context.html#method.animate_bool) | **A2/A3/A8:** identity должна переживать нужные обновления, idle не должен бесконечно заказывать кадры. Immediate-mode repaint модель не означает, что retained FLUI должен rebuild всё дерево. |
| **GPUI — element lifecycle core** | `with_spring` использует state element ID; retarget сохраняет position/velocity. Код reduced-motion выбирает static state и не заказывает animation frame; обычный wrapper применяет animator в `request_layout`, затем запрашивает layout полученного element. [Проверенный source](https://github.com/zed-industries/zed/blob/b47a4ca595d3a3fba116b9e78ef01a46e2e43f3e/crates/gpui/src/elements/animation.rs) | **A2/A4/A5:** полезный пример реального потребителя. **A8:** нельзя считать этот wrapper доказательством «никакого layout/rebuild». Shared phase и FPS throttling — отдельные функции; не добавляем последние в scope FLUI автоматически. |
| **Xilem/Masonry — declarative layer / widget engine** | В Masonry animation callback получает nanosecond interval; первый interval при idle→active равен нулю. Widget отдельно запрашивает следующий animation frame и paint либо render/accessibility invalidation. [Masonry 0.4 Widget](https://docs.rs/masonry/0.4.0/masonry/core/trait.Widget.html#method.on_anim_frame) | **A3/A6:** единица времени и первый tick должны быть явными; frame scheduling, paint и semantics — разные обязанности. Найденный низкоуровневый API не доказывает наличие высокоуровневого implicit retarget во всём Xilem. |

## Два дополнительных ориентира

**Jetpack Compose** особенно полезен для границы между значением и run. `Animatable`
гарантирует непрерывность значения при `animateTo` с interruption; для spring отдельно
обещает непрерывность скорости и обеспечивает взаимное исключение animation runs.
Это более точный контракт, чем «все анимации плавные».
[Официальный Animatable](https://developer.android.com/reference/kotlin/androidx/compose/animation/core/Animatable).

Compose также показывает, почему место чтения animated state важно: `graphicsLayer {}`
применяется для scale/translation/rotation; выбор animation API зависит от того, нужны ли
согласованные свойства, жест или appearance/disappearance. Вывод для **A4/A6/A8** —
проверять правильную фазу выполнения и общий run, а не копировать coroutine API в синхронный
frame path FLUI. [Quick guide](https://developer.android.com/develop/ui/compose/animation/quick-guide),
[Choose an animation API](https://developer.android.com/develop/ui/compose/animation/choose-api).

**Motion** — самостоятельная библиотека, ранее Framer Motion. Её `layout`/`layoutId`
опираются на transform для layout transitions. `MotionConfig.reducedMotion` разделяет
`user`, `always`, `never`; при включённой policy transform/layout отключаются, другие эффекты,
например opacity, могут продолжаться. `useReducedMotion` реагирует на изменение системной
настройки. Вывод для **A5/A6** — policy различает характер эффекта и обновляется вживую;
из этого не следует, что FLUI обязан перенять default Motion (`never`) или всю layout-engine.
[Motion layout](https://motion.dev/docs/react-layout-animations),
[MotionConfig](https://motion.dev/docs/react-motion-config),
[useReducedMotion](https://motion.dev/docs/react-use-reduced-motion).

## Как это меняет проверку готовности, не расширяя scope

Следующие строки — наши выводы из сравнения, а не заявления о реализации конкурентов.
Они уточняют существующие пакеты [плана](readiness-plan.md).

| Пакет | Проверка, которую следует сохранить |
|---|---|
| **A1** delivery/lifecycle | Run и directional status различимы; отмена/завершение имеют точный outcome; reentry не теряет принятую доставку. Конкурентные API не освобождают FLUI от собственных panic/reentry контрактов. |
| **A2** ownership | Изъятие widget/owner отменяет run и подписки; exit не делает объект бессрочно живым; повторный mount не наследует чужую identity. |
| **A3** clock | Первый кадр, mute, pause, resume и target change используют одно определённое время. Проверить два окна и headless adapters; не смешивать «tick не доставлен» с «время заморожено». |
| **A4** retarget | Проверять положение и скорость настоящего implicit-widget при gesture release и повторном target. Не объявлять весь curve path C¹ лишь потому, что spring math имеет такое свойство. |
| **A5** motion policy | Применение до первого кадра, live update, overrides и Preserve; бесконечный decorative effect не продолжает заказывать кадры после принятого reduced-motion решения. |
| **A6** geometry | Paint, hit-test/event localization и semantics согласованы; optimization доказана frame/build counters и GPU readback, а не названием transition API. |
| **A7** composition/surface | Существующий API достигает caller; отмена группы и удаление устаревшего пути проверены. Transaction, general FLIP и весь browser animation API не добавляются только для parity. |
| **A8** performance/docs | Раздельно измерять math, callbacks, rebuild, layout, paint, GPU и idle. Не обещать off-main или compositor-only по косвенным признакам. |
| **A9** release | Проверить native/backend, accessibility и interruption; библиотечные unit tests сами по себе не подтверждают сквозную готовность. |

Сравнение поддерживает последовательность текущего плана: delivery и ownership → единое
время → реальный retarget consumer → policy и geometry → сквозная приёмка. Оно не отменяет
32 воспроизведённых отказа из [локального evidence](readiness-evidence.md) и не подменяет
их дополнительной библиотекой кривых. Перед реализацией конкретного заимствованного
контракта следует закрепить источник на версии/commit и написать свой публичный тест.

## Ограничения проверки

- Не запускались приложения, benchmarks или тесты конкурентов. Выводы о runtime здесь
  основаны на их документации и указанных исходниках.
- Не проверены все renderer/backend комбинации Dioxus, все платформы SwiftUI, все возможности
  Xilem и сторонние animation packages. «Не проверено» не означает «не поддерживается».
- Не оценивались licensing/dependency suitability для заимствования реализации. Никакой
  внешний код или новая зависимость в FLUI не добавлены.
- Текущая английская React документация и release note использованы вместо устаревших
  поисковых выдержек локализованной документации, где `ViewTransition` ещё назывался Canary.
- Новый общий `AnimationContext`/transaction API, FPS throttling и general layout-FLIP остаются
  самостоятельными решениями владельца scope, а не обязательными результатами этого обзора.
