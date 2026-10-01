# Flutter расширения: обязательные решения для FLUI

Проверка источников: 1 октября 2026 года, через Keenable и Firecrawl.
Это исследование первичных API и документации; приложения Flame/Scene/GenUI
не запускались, их производительность и platform parity не измерялись.
Main/latest страницы изменяемы; исторический материал Flutter 3.24 не
выдаётся за текущий список ограничений Flutter 3.47.

## Исправление исходной позиции

Flame и активная AI экосистема Flutter — конкретные доказательства ценности
расширяемого UI framework. Отсутствие доказательства массовости 3D к 2031
не оправдывает откладывание device ownership, custom rendering, scheduling
и package contracts. В то же время расширяемость не требует поместить physics,
scene graph или inference в UI rasterizer. Правильный вопрос: может ли пакет
реализовать эти возможности через поддерживаемые границы, без engine fork?

## Реальные архитектуры

### Flame: отдельная модель игры внутри UI

Официальный GameWidget — мост: Game не является Flutter widget, но может
занять всё окно или часть layout, сосуществовать с navigation/dialogs/overlays;
в приложении могут быть несколько GameWidget [1]. Controlled constructor
создаёт и владеет Game; обычный принимает предоставленный instance. Focus,
hit-test policy, loading/error builders и overlays являются частью интеграции.
Canvas автоматически не клипируется границами GameWidget — документация
предлагает ClipRect при необходимости. Это явный контракт, а не основание
называть Flame «костылём».

FlameGame владеет component tree и update/render loop. Update получает dt,
render — canvas; resize, load, mount/remove, pause и cleanup имеют отдельный
lifecycle. Документация предупреждает о создании Game в build и о необходимости
cleanup; dispose помогает удалить children и очистить caches [2]. Для FLUI
это пример нужной host seam, а не предложение переносить Dart class hierarchy.

### Flutter GPU и Scene: низкий GPU API и пакет 3D

Текущий официальный flutter_gpu API описан как low-level API для rendering
packages: GpuContext, DeviceBuffer, ShaderLibrary, RenderPipeline, CommandBuffer,
RenderPass, RenderTarget, depth/stencil, sampling и presentable surfaces.
GpuImageSurface — pool render targets, который Flutter рисует как ui.Image [3].
Здесь важна поддерживаемая связь custom GPU результата и обычной композиции UI.

Flutter Scene — отдельная экосистема packages, выросшая из реализации внутри
engine. Официальный сайт прямо объясняет, что Flutter GPU позволил вынести
Scene наружу. Scene заявляет retained scene graph и declarative widget API,
materials/assets/physics/post-processing, widgets на 3D surfaces, semantics
components и editor MCP [4]. Это заявления проекта, не независимо проверенные
гарантии. Его текущая API документация указывает Flutter 3.47+, GPU opt-in
на native, web backend WebGL2 и pre-1.0 изменяемость [5].

Историческая официальная статья Flutter 3.24 описывала GPU/Scene preview,
main channel и Impeller ограничения [6]. Нельзя переносить этот список на
2026 без проверки: нынешний API и package требования уже изменились.

### Три разных значения Flutter AI

| Направление | Что реально делает | Место относительно renderer |
|---|---|---|
| Coding agents/MCP/skills | Анализатор, тесты, runtime inspection и помощь изменению исходников [7]. | Development tooling; не end-user app inference. |
| Flutter AI Toolkit | Chat widgets, streaming, voice/media, function calling, serialization и abstract LLM provider; Firebase AI Logic adapter [8]. | UI package плюс IO/provider integration. Permissions microphone/files/network конфигурируются отдельно. |
| GenUI | Runtime orchestration пользователя, widgets и агента; JSON композиция из developer widget catalog и обратная передача state changes [9][10]. | Динамический UI/data binding package. Это реальное AI приложение, не только средство написания кода. |

GenUI README явно называет SDK highly experimental, описывает A2UI v0.9,
backend adapters и переработку в modular packages; streaming UI фигурирует
также в future goals [10]. Не следует делать из стороннего tutorial гарантию
полной streaming реализации. Model-authored JSON — входные данные; SDK не
даёт права выполнять произвольный сгенерированный код.

## Какие контракты FLUI необходимо определить сейчас

«Определить» означает принять проверяемую границу и критерии её реализации;
это не означает немедленно публиковать десятки unwired traits.

| Контракт | Решение, которое нельзя оставлять случайным | Проверка минимальным пакетом |
|---|---|---|
| GPU ownership и recovery | Кто владеет device/queue, может ли пакет использовать тот же device, как ресурсы привязаны к generation; порядок drop и реакция на device loss. | Custom render target переживает resize; старый ресурс отвергается после recovery. |
| Custom output composition | Same-device render target либо external texture route: format, alpha/color, usage, readiness, lifetime, clip, damage и sampling. Решение должно избегать обязательного CPU readback/upload round trip. | 3D triangle под обычным UI, clip/opacity/filter и resize, pixel readback и проверка upload traffic. |
| Scheduling | Кто запрашивает кадр; continuous animation, visibility/background pause, fixed-step simulation и presentation timing не должны плодить конкурирующие vsync loops. | Игра+UI overlay; pause/resume без скачка dt, ограниченный catch-up и один presentation owner. |
| Input и focus | Mapping widget coordinates→game/3D target, pointer capture/cancel, overlays, keyboard/IME и focus handoff. | HUD закрывает game target; drag отменяется при removal; текстовый input сохраняет IME. |
| Accessibility | Custom scene не может публиковать только pixels: роли/labels/actions, focus и невизуальная альтернатива. | Игровой/3D объект доступен через тот же action path и AT; semantic geometry следует camera. |
| Package capabilities | Уточнить evolving SDK и lifecycle acquisition для GPU/clock/assets; платформенные типы остаются за platform contracts. Unsupported capability — явный Result, не no-op. | Пакет строится на SDK, запрашивает capability при lifecycle, имеет headless failure path. |
| AI-generated UI | Разрешённый catalog, schema/version negotiation, данные и action binding, depth/node/text budgets, invalid patch containment. | Валидный catalog JSON строит UI; неизвестный widget, цикл, oversize и invalid update отклоняются без потери следующей операции. |
| Resource/security budgets | GPU memory, frames-in-flight, shader/asset work; AI request/token/concurrency и cancellation; permission и redaction policies. | Отмена pending load/inference при закрытии realm; queued response не меняет replacement instance; bounded caches. |

В FLUI уже есть external texture registry, scheduler, SDK, semantics и
типизированный agent protocol ADR-0080/0095. Поэтому работу следует начинать
с production reach и выявления недостающих гарантий этих поверхностей, а не
с дублирования. Debug-only authenticated local agent server не является
production AI authorization: GenUI и model tools потребуют отдельной границы
доверия, schema validation и решения приложения о допустимом действии.

## Конкретный путь

Первый consumer — небольшой 2D game package: owned state, clock request,
sprite atlas, input, pause и обычный UI overlay. Он проверяет Flame-подобную
интеграцию без physics и 3D catalog. Второй — same-device custom 3D target с
depth buffer и UI overlay: он вскрывает настоящие device/composition contracts.
Третий — bounded generated form из catalog с fake async provider, затем
реальный adapter; invalid schema и поздний ответ проверяются до подключения LLM.

Async IO, inference и asset preparation доставляют результат следующей
транзакции. Из этого не следует запрет app AI runtime или постоянной game
simulation: они допустимы на соответствующем слое. Нельзя ни заставлять их
работать в paint, ни использовать запрет async paint как довод против них.

До публичной стабилизации результаты трёх consumers должны определить ADR:
ownership, scheduling и capability guarantees. Renderer остаётся wgpu,
но packages получают реальный путь расширения. Производительность оценивается
по frame latency, bandwidth, memory и recovery, не по числу поддержанных nouns.

## Источники и доступ

1. [Flame GameWidget](https://docs.flame-engine.org/latest/flame/game_widget.html), полный текст Keenable.
2. [FlameGame lifecycle и game loop](https://docs.flame-engine.org/latest/flame/game.html), Keenable, прочитаны разделы до Low-level API; хвост ответа усечён.
3. [Официальный flutter_gpu API](https://api.flutter.dev/flutter/flutter_gpu/), Keenable.
4. [Flutter Scene официальный сайт](https://fscene.dev), Firecrawl; возможности заявлены проектом.
5. [Flutter Scene API/requirements](https://fscene.dev/api/flutter_scene/latest), Keenable search excerpts; сведения о требованиях ограничены этим извлечением.
6. [Историческая Flutter GPU статья, pinned source](https://github.com/flutter/website/blob/ab59c614e780e2d6d44f07ae4a96238028f581a5/sites/www/content/blog/getting-started-with-flutter-gpu/index.md), Firecrawl developer-search excerpts; относится к 3.24.
7. [Flutter AI development tools](https://docs.flutter.dev/ai/get-started), Keenable; перечень core capabilities прочитан, install guide усечён.
8. [Flutter AI Toolkit](https://docs.flutter.dev/ai/ai-toolkit), Firecrawl, page updated 23 сентября 2026.
9. [GenUI official overview](https://docs.flutter.dev/ai/genui), Keenable, page updated 19 августа 2026.
10. [Flutter GenUI README](https://github.com/flutter/genui), Keenable, изменяемый main.
