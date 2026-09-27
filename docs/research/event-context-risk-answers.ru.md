# Ответы на открытые риски событий и состояния

Дата проверки: 2026-09-27. Код: `4a16b7305`; исходное заключение:
[условия принятия](event-context-foundation-verdict.ru.md). Исследование выполнено
через Firecrawl developer-index с последующим чтением первоисточников. Индекс
иногда возвращал сторонние пересказы и неполные passages; они не использованы
как основание контрактов. Документация Tokio при live-fetch: 1.53.1,
tokio-util: 0.7.19, GPUI: 0.2.2, slotmap: 1.1.1.

Это предложения для FLUI, а не принятый ADR и не реализация. Production-код,
dependency features и публичные контракты этим исследованием не менялись.

**Статус последующей реализации.** После исследования отдельным изменением
закрыты две конкретные неоднозначности из разделов 2 и 4: semantic-команды
сохраняют кратность/FIFO (включая хвост после panic), а формы и поля используют
generation-stamped lease, typed diagnostic drain и условное освобождение.
Остальные предложения документа не считаются реализованными этим follow-up.

## Вывод

Не требуется превращать EventCx в универсальный runtime. Нужно разделить доступ
к состоянию, lifetime получателя, revision операции и правила доставки. У этих
задач есть зрелые строительные блоки, но ни один сам по себе не решает все четыре.
Очередные тесты должны проверять интеграцию этих гарантий в FLUI, а не повторять
внутренние тесты выбранной библиотеки.

## 1. Отмена и устаревшие результаты

**Первоисточники.** Tokio: “just schedule the task for cancellation”; вызов abort
не подтверждает завершённую отмену, а уже начатый spawn_blocking не прерывается
этим механизмом. [Контракт отмены](https://docs.rs/tokio/1.53.1/tokio/task/index.html#cancellation).
У CancellationToken отмена родителя распространяется на child token, но не
наоборот; это уведомление для сотрудничающей задачи, не принудительное завершение.
[Контракт token](https://docs.rs/tokio-util/0.7.19/tokio_util/sync/struct.CancellationToken.html).
React отдельно показывает cleanup, игнорирующий результат предыдущего запроса,
поскольку ответы могут приходить не по порядку.
[useEffect](https://react.dev/reference/react/useEffect#fetching-data-with-effects).

**Решение для FLUI.** Mount владеет scope задач и отзывает право доставки при
dispose. Принимающий UI проверяет identity экземпляра и, для выбранной политики
latest-wins, revision запроса **до** пользовательского effect. Отмена producer
экономит ресурсы; проверка доставки обеспечивает корректность, даже если producer
уже завершился или не умеет останавливаться. Автосохранение и независимые команды
не должны автоматически наследовать latest-wins поиска.

Существующие AsyncDriver/TaskToken оставить базой owner-local пути. На IO-границе
оценить существующий tokio-util, не вводить второй executor. В workspace его
features намеренно ограничены: нельзя просто включить `rt` во всех слоях.
Внешние эффекты — запись файла, отправка запроса — не откатываются токеном;
если нужен rollback, его проектирует доменная операция.

**Закрывающий тест.** A → B → завершение B → завершение A, unmount/remount с тем же
доменным ключом, completion уже в inbox перед dispose. Старый пользовательский
effect не вызывается. Отдельно: блокирующая работа может продолжаться, но её
результат не доставляется закрытому UI. Тесты FutureBuilder уже покрывают только
принадлежащую ему future, не этот полный IO-контракт.

## 2. Очереди, перегрузка и справедливость

**Первоисточники.** Bounded mpsc ограничивает число сообщений и обеспечивает
backpressure; drop receiver удаляет непрочитанные сообщения.
[Tokio mpsc](https://docs.rs/tokio/1.53.1/tokio/sync/mpsc/index.html).
Watch хранит только последнее значение: “no guarantee that consumers will see all values”.
[Tokio sync](https://docs.rs/tokio/1.53.1/tokio/sync/index.html#watch-channel).
Гарантия fairness Tokio требует ограниченного числа задач и ограниченного времени
одного poll; это не обещание одинакового обслуживания или бюджета UI-кадра.
[Runtime](https://docs.rs/tokio/1.53.1/tokio/runtime/index.html#detailed-runtime-behavior).

**Предлагаемые политики, не текущие гарантии FLUI:**

| Семейство | Доставка при перегрузке |
|---|---|
| Progress / заменяемый snapshot | Последнее значение на ключ операции; объединение допустимо явно |
| Команда / click / текстовая операция | Порядок принятия; явный отказ при невозможности принять, не молчаливое объединение |
| Completion / error / cancellation | Отдельный terminal result на принятую операцию; ограничение числа операций при admission |

Ограничение числа записей не ограничивает байты произвольных captures. Нужны
лимит активных producers, учёт размера payload там, где он известен, и метрики
high-water mark, возраста события, длительности callback и stale/drop reason.
Frame-path не ждёт `send().await` и не блокируется на свободном месте: bounded
async channel применим на IO-границе, локальная очередь остаётся синхронной.
Конкретную ёмкость, число обработок за turn и политику обслуживания owners
выбрать измерением. Нельзя универсально обрезать post-frame drain, не проверив
порядок animation/lifecycle: перенос работы меняет наблюдаемый контракт.

**Обнаруженное спорное допущение.** В
`crates/flui-widgets/src/interaction/gesture_detector.rs:532` комментарий объявляет
два одинаковых запроса до кадра одним нажатием; AtomicBool действительно теряет
кратность. Это подтверждено чтением кода, но новый исполняемый тест здесь не запускался.
Наш Notes-тест проверял по одному действию и не разрешает этот вопрос. Нужен
сценарий двух принятых semantic Click на counter до кадра и сравнение с двумя
обычными нажатиями. Дедупликация повторной доставки по request ID и объединение
двух независимых команд по времени — разные вещи. В текущем flag нет такого ID.

**Закрывающий опыт.** Приостановить кадры, нагрузить progress и terminal events,
параллельно обслуживать второго owner; проверить отказ/порядок, отсутствие потери
terminal outcome, peak memory и p99. Нынешние 10 000 записей в одном callback
доказывают лишь coalescing rebuild, не ограничение входной очереди.

## 3. Lifetime, Weak и адресация

**Первоисточники.** Weak не удерживает значение, но “does prevent the allocation
itself ... from being deallocated”.
[Rust Weak](https://doc.rust-lang.org/std/rc/struct.Weak.html).
GPUI WeakEntity::update возвращает ошибку, если entity освобождена.
[GPUI WeakEntity](https://docs.rs/gpui/0.2.2/gpui/struct.WeakEntity.html#method.update).
Slotmap даёт versioned keys, но ключ одной карты нельзя использовать в другой:
это не определяется автоматически. Версия также имеет конечный диапазон.
[Slotmap](https://docs.rs/slotmap/1.1.1/slotmap/#custom-key-types).

**Решение для FLUI.** Сначала сравнить слабую ссылку на небольшой target-state,
generation attachment и очистку callback при dispose с registry. Не захватывать
сильный Rc callback в queued closure, если требуется раннее освобождение его
ресурсов. Weak на объект с большим inline payload всё ещё может удерживать
backing allocation. Пользовательские destructors выполняются вне framework
borrow/lock. Даже успешный upgrade не доказывает mounted: объект может иметь
других владельцев. Generation привязки проверяется отдельно.

Адрес доставки должен различать presentation, экземпляр attachment и ключ
операции; это логические компоненты, не обязательное предложение нового public
struct. Slotmap стоит использовать при обоснованной потребности в arena, но не
как замену provenance или lifetime-контракту. Политику переполнения generation
также нельзя оставлять неявной.

**Закрывающий тест.** Поставить реальный callback в post-frame queue, dispose без
следующего кадра, проверить Drop-счётчик большого capture; затем drain и отсутствие
вызова. Повторить remount и повторное использование arena slot. Существующий
RawButton teardown-тест до стадии enqueue этого не доказывает.

## 4. Двойное подключение FormHandle

**Первоисточник.** Flutter запрещает одновременно два widgets с одним GlobalKey
и диагностирует это runtime assertion.
[GlobalKey](https://api.flutter.dev/flutter/widgets/GlobalKey-class.html).
Это прецедент эксклюзивной привязки, не основание копировать assertion в FLUI.

**Решение для FLUI.** Cloneable пользовательский handle может остаться, но право
его attachment должно быть эксклюзивным: fallible admission и RAII lease с
identity/generation. При втором mount первая форма не меняется. Dispose старой
привязки не очищает новую. Тип attachment не клонируется; handle не является им.

Проверять только init_state поздно: `Form::create_state` уже вызывает configure,
а FormField также конфигурирует общий storage. Отказ должен происходить до этих
мутаций либо mutable configuration должна принадлежать mounted state, а handle
лишь адресовать его. Выбор потребует ADR для lifecycle failure path; это не
маленький `if already_attached { panic!() }`. Политика FLUI требует typed Result
для caller-triggerable отказа. Изменение такой формы сейчас предпочтительнее
сохранения неявного запрета до появления внешних потребителей.

**Закрывающий тест.** Два mount одного handle в одном и разных owners: второй
получает определённый отказ, первый сохраняет fields/callback/source; снятие
старого attachment после законного rebind не ломает новый.

## 5. Panic не равна rollback

**Первоисточник.** Rust UnwindSafe предупреждает о нарушении логических инвариантов;
это “speed bump”, а не транзакционная гарантия.
[UnwindSafe](https://doc.rust-lang.org/std/panic/trait.UnwindSafe.html).

**Решение для FLUI.** Не объявлять восстановление хвоста scheduler восстановлением
целостности приложения. Произвольный callback может успеть изменить несколько
сигналов или внешний мир. Dirty-mark на unwind обеспечил бы лишь видимость
частичного состояния, а не корректность. Для доменной edit-операции рассмотреть
prepare/validate/commit или собственный undo; не добавлять Clone-rollback ко всем
signals и не расширять EventCx обещанием atomicity.

Продолжение после panic допустимо только для границы с доказанным восстановлением.
Для остальных границ требуется явный host policy: прекращение операции/изоляция
повреждённого владельца/остановка, согласованные с существующими catch boundaries.
Исследование не выбирает автоматически новый process-wide abort и не меняет
действующую [panic policy](../PANIC-POLICY.md).

**Закрывающий тест.** Паника между двумя доменными мутациями: проверить состояние,
invalidation и то, какие дальнейшие действия host допускает. Нынешний тест
частичного signal update без invalidation должен быть явно пересмотрен, если
будет выбрана другая политика, а не тихо ослаблен.

## 6. Общий документ и несколько окон

**Первоисточник.** GPUI описывает app-owned entities, наблюдения и очередь effects,
которая отделяет уведомление от реентрантного исполнения listeners. Этот подход
не обещает ограниченного времени drain.
[Ownership and data flow](https://zed.dev/blog/gpui-ownership) (авторский материал,
обновлённый для API на 2025-12-12).

**Решение для FLUI.** Авторитетный документ/undo/revision живёт дольше конкретного
окна; presentation хранит focus, selection и UI-state. Команда меняет документ,
его revision уведомляет подписанные окна, каждое обновляет свою проекцию законным
путём. Не переносить Signal одного графа в другой и не вводить process singleton.
Принадлежность документа realm или более высокому application service определить
по действительным правилам совместного доступа; GPUI App не является готовой
топологией FLUI. CRDT не нужен только ради двух локальных окон.

**Закрывающий тест.** Одно изменение видно в двух окнах, selection независимы,
закрытие одного не удаляет документ и не оставляет его subscription. Это проверка
расширяемости, не самовольно добавленный H0 release gate.

## 7. Независимая перепроверка через Keenable

Keenable использован после Firecrawl как независимый путь поиска и чтения. Для
решений оставлены только первичные или авторские источники; найденные сторонние
пересказы не использованы. Десятый одновременный fetch упёрся в лимит организации
10 RPS, поэтому запросы не повторялись циклом. Полностью прочитаны страницы React,
Tokio, Rust, GPUI и Zed. Страница Apple была найдена и дала нужную карточку результата,
но полный fetch оказался именно ограниченным запросом; это частичное, а не полное
прочтение источника.

- React отдельно подавляет устаревший async-result через cleanup/identity допуска,
  потому что ответы могут приходить не по порядку; Tokio отдельно предупреждает,
  что abort не даёт права ожидать завершение и не прерывает уже начатый
  `spawn_blocking`. Это подтверждает разделение cancellation и freshness, уже
  записанное в ADR-0027. Источники: [React useEffect](https://react.dev/reference/react/useEffect),
  [Tokio AbortHandle](https://docs.rs/tokio/latest/tokio/task/struct.AbortHandle.html),
  [CancellationToken](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html).
- Bounded Tokio mpsc ждёт свободное место и сохраняет порядок; `watch` хранит
  только последнее значение. Следовательно, latest-value state и lossless command
  остаются разными классами, а синхронный frame path не должен делать `send().await`.
  Источники: [mpsc::channel](https://docs.rs/tokio/latest/tokio/sync/mpsc/fn.channel.html),
  [watch](https://docs.rs/tokio/latest/tokio/sync/watch/index.html).
- Стандартная библиотека подтверждает: `Weak` не удерживает значение, но удерживает
  backing allocation. GPUI `WeakEntity` отказывает в update освобождённой entity.
  Для queued UI delivery слабой должна быть ссылка на весь delivery target, а не
  только проверка `mounted` после сильного захвата callback slots и writer.
  Источники: [std::rc](https://doc.rust-lang.org/std/rc/index.html),
  [GPUI WeakEntity](https://docs.rs/gpui/latest/gpui/struct.WeakEntity.html).
- `UnwindSafe` является лишь speed bump для наблюдения нарушенных логических
  инвариантов, а `catch_unwind` не рекомендуется как общий try/catch. Восстановление
  scheduler queue tail поэтому не является rollback пользовательских эффектов.
  Источники: [UnwindSafe](https://doc.rust-lang.org/core/panic/trait.UnwindSafe.html),
  [catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html).
- Zed описывает app-owned entities и deferred effect queue; найденная документация
  Apple разделяет один document и отдельный window controller для каждого его окна.
  Это согласуется с ADR-0027: документ/revision/undo выше realms, а focus, selection
  и UI projection принадлежат окну. Источники: [GPUI ownership](https://zed.dev/blog/gpui-ownership),
  [Apple document architecture](https://developer.apple.com/library/archive/documentation/DataManagement/Conceptual/DocBasedAppProgrammingGuideForOSX/KeyObjects/KeyObjects.html).

Перепроверка не обосновывает универсальный новый runtime. `FutureBuilder`,
`StreamBuilder` и image resolution уже используют consumer generation поверх
`TaskToken`; realm command inbox уже bounded и возвращает typed `ChannelFull`.
Найдены три локальных разрыва: post-frame semantic delivery сильно удерживал
delivery state после dispose; `FormField::reset` мог изменить состояние и упасть
до dirty-mark; глубина общей shared/local post-frame партии не наблюдалась. В этом
follow-up delivery closure переведён на слабый mounted target, dirty-mark перенесён
до controller/user callbacks, а scheduler публикует точные work-item counts без
изменения доставки. Общий signal-panic rollback и reusable cross-realm document
subscription по-прежнему требуют отдельного ADR/prototype с реальным consumer.

## Порядок следующей реализации

1. Зафиксировать кратность semantic-команд и эксклюзивность FormHandle: конкретные
   неоднозначности текущего кода, red tests до production-изменений.
2. Проектировать lifetime доставки поверх существующих owner/async механизмов;
   покрыть completion-in-inbox и request revision, затем закрывать transitional
   Reactive-access из ADR-0086.
3. Измерить реальные queue retention и overload. Только после этого выбирать
   ёмкость, квоты и необходимость registry; не обещать p99 по чужому benchmark.
4. Проверить публикационный consumer, native input/MCP и платформенные CI lanes.
   Интернет не заменяет эти доказательства.

## Материалы и границы исследования

Локальные raw-снимки Firecrawl лежат в gitignored `.firecrawl/`: `search-*.json`,
`tokio-task.json`, `tokio-channels.json`, `tokio-mpsc.json`, `tokio-runtime.json`,
`cancellation-token.json`, `rust-unwind.json`, `weak.json`, `gpui-weak.json`,
`gpui-ownership.json`, `flutter-key.json`, `react-effect.json`,
`slotmap-verified.json`. Исходные URL приведены рядом с выводами выше.
Один scrape slotmap получил rate-limit; повтор после окна ограничения успешен.
Ни найденные issues, ни мнения сторонних блогов не трактовались как доказательство
merged fix. Нагрузочные, платформенные и новые семантические тесты в этом
исследовании не запускались; прежний зелёный gate не доказывает новые гипотезы.
