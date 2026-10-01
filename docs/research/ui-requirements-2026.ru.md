# Требования к UI в 2026 году и границы ответственности FLUI

Дата исследования: 30 сентября 2026 года. Это исследовательская записка, а не
принятый архитектурный контракт и не утверждение о готовности FLUI.
Keenable использован для поиска и чтения, Firecrawl — для независимого получения
страниц Apple и Android. Приоритет отдан первичным спецификациям и документации
платформ. Старый год публикации не делает действующий платформенный контракт
устаревшим; наличие статьи 2026 года само по себе не делает её надёжной.

## Вопросы исследования

1. Как совместить плавность, переменную частоту обновления и расход энергии?
2. Что реально требуется для wide gamut, HDR и внешних GPU-текстур?
3. Какие обязательства создают доступность, клавиатура и reduced motion?
4. Что требуется от пользовательского редактора для IME и разных языков?
5. Как распределить эти требования между engine, host и widgets и проверить их?

## Подтверждённые требования

### Частота кадров и энергия

Android ARR, представленный в Android 15, отделяет частоту VSync от частоты
обновления панели. Документация приводит пример VSync 240 Гц при максимальном
обновлении 120 Гц и связывает меньшую частоту с экономией энергии. Это описание
системного механизма, а не обещание, что любое приложение получит 240 FPS [1].
Apple независимо подтверждает связь ProMotion, частоты анимации и энергопотребления:
второстепенная анимация с высокой частотой может поднимать частоту всего экрана.
`CADisplayLink.preferredFrameRateRange` задаёт диапазон и предпочтение, а система
выбирает доступный режим [2].

**Вывод для FLUI:** host/platform должен поставлять реальный временной сигнал и
возможности конкретного окна; scheduler/runtime выбирает необходимость и темп
кадра; engine сообщает длительность CPU/GPU и результат представления. Постоянный
таймер 60 Гц не является достаточным контрактом. Максимальная частота также не
должна быть целью каждого статического экрана. Это проектная рекомендация,
поддержанная двумя независимыми платформенными источниками, а не требование
реализовать одинаковый алгоритм Apple и Android.

### Цвет, HDR и взаимодействие с платформой

Android различает дисплей с широким gamut и поддержку управляемого цветом вывода.
ICC-профиль изображения, формат буфера и цветовое пространство поверхности
участвуют в результате; включение wide gamut увеличивает память и стоимость
композиции [3]. Apple EDR допускает RGB выше 1 при SDR-white = 1, учитывает текущий
headroom дисплея и настраивает Metal layer, float-формат и цветовое пространство
совместно. Alpha при этом остаётся в диапазоне 0–1 [4].

**Вывод для FLUI:** одной замены поверхности на FP16 недостаточно. Нужен сквозной
контракт: пространство исходного цвета/изображения → рабочее пространство
blend/filter → формат промежуточных текстур → transfer function и пространство
вывода. Host владеет возможностями монитора/окна; painting/assets — смыслом
исходных данных; engine — преобразованиями, композицией и точностью. Wide gamut
и HDR — разные возможности; их нельзя объединять в один флаг по имени GPU backend.
Apple и Android подтверждают необходимость согласованной цепочки, но не задают
единственное правильное рабочее пространство для FLUI [3][4].

WebGPU валидирует команды и использование ресурсов, определяет отдельные
возможности/лимиты adapter и device, а также обработку GPU-ошибок [5].
**Рекомендация:** engine должен запрашивать только поддерживаемые дополнительные
возможности, соблюдать лимиты устройства и различать успешный submit, успешный
present и ошибку GPU. Внешняя текстура должна иметь явное описание размера,
sampling, alpha, цветового пространства и срока доступности. Импорт видеокадра,
синхронизация с декодером и native view принадлежат platform/host; корректное
сэмплирование и композитинг принадлежат engine. Это рекомендуемый контракт,
а не заявление о существующей поддержке всех видов native interop в wgpu.

### Доступность и управление движением

WCAG 2.2 определяет проверяемые критерии для web-контента [6]. Для native FLUI
они служат полезной проверочной базой; эта записка не утверждает автоматическое
юридическое применение web-критериев к каждому native-приложению.

- Keyboard 2.1.1 — уровень A: функциональность доступна через клавиатуру, кроме
  функций, где важна траектория движения.
- Focus Not Obscured (Minimum) 2.4.11 — AA: компонент с клавиатурным фокусом не
  должен быть целиком скрыт авторским контентом. Focus Appearance 2.4.13 — AAA;
  их нельзя смешивать.
- Target Size (Minimum) 2.5.8 — AA: 24 × 24 CSS pixels с указанными исключениями,
  включая spacing и inline. Это не 24 физических пикселя native-дисплея.
- Animation from Interactions 2.3.3 — AAA: несущественную анимацию от взаимодействия
  можно отключить [6][7].

Apple предоставляет Reduce Motion как системную настройку; zoom/slide может
заменяться dissolve, parallax отключается [8]. Это независимое подтверждение
практической необходимости предпочтения движения, а не утверждение, что любой
fade всегда допустим или что AAA автоматически обязателен [7][8].

**Вывод для FLUI:** platform доставляет настройки и события assistive technology;
semantics задаёт роли/действия; widgets/focus/layout обеспечивают порядок фокуса,
доступный размер цели, видимость и альтернативы drag; animation/runtime учитывает
reduced motion. Engine должен точно рисовать focus indicator, сохранять контраст
и clipping и не терять тонкие элементы при масштабировании. GPU-readback не
доказывает, что screen reader видит роль, а semantics-тест не доказывает видимость
фокуса. Нужны обе проверки и live smoke с assistive technology.

### IME и локализация

Android `InputConnection` — канал редактора и IME; источником текста может быть
клавиатура, handwriting, speech или emoji. Документация требует отдельно учитывать
composing region и selection, рекомендует испытания нескольких IME, CJK и RTL [9].
Apple custom text view также должен поддерживать marked text и связь с input
context; при прокрутке нужно обновлять координаты символов для UI ввода [10].

Важное различие: Android допускает независимые composition и selection; архивное
руководство Apple описывает selection внутри marked text. Следовательно, нельзя
транслитерировать ограничения одной платформы в универсальный TextStore [9][10].

**Вывод для FLUI:** editor/widgets владеет текстом, диапазонами и транзакциями;
platform адаптирует протокол конкретной ОС; shaping/layout вычисляет bidi,
fallback, позиции glyph и caret; engine рисует уже shaped glyph, selection и
composition decoration. Локализация не сводится к наличию glyph atlas. Engine
не должен становиться владельцем IME или заново формировать абзац при paint.

## Предлагаемые проверки и приоритеты

Это инженерная оценка для плана FLUI, не результат замеров конкурентов.

| Приоритет | Контракт | Где проверять | Доказательство готовности |
|---|---|---|---|
| Сначала | Возможности/лимиты GPU, ошибочные ресурсы и recovery | engine + host | Таблица adapter features; device loss; следующий кадр после ошибки; восстановление ресурсов |
| Сначала | Alpha, sampling, atlas edge, порядок композиции | engine | Pixel readback с независимым CPU-оракулом и различающими sample points |
| Сначала | Клавиатура, видимый focus, реальные IME | widgets/platform/semantics | Live smoke и поведенческие тесты; CJK composition, RTL, emoji, scroll при активном IME |
| Далее | Реальный cadence и отсутствие лишних кадров | host/scheduler/engine | 60/90/120 Гц и смена режима; idle/occlusion; CPU/GPU time, missed deadlines и энергия |
| Далее | Reduced motion и масштаб текста | platform/widgets/animation | Переключение preference во время работы; сохранение функций и видимости focus |
| По продуктовой необходимости | Wide gamut/HDR | painting/assets/platform/engine | ADR всего pipeline; SDR fallback; монитор с поддержкой и без неё; профильные изображения |
| По продуктовой необходимости | Видео/native interop | platform/engine | Обновление кадра, fence/lifetime, alpha/color, resize/recovery и отсутствие устаревшего кадра |

Golden suite следует строить как матрицу возможностей и составных сценариев:
fractional DPR + transformed clip + translucent image; blur + partial damage;
text fallback + selection + IME decoration. Полное совпадение каждого пикселя между
GPU разных производителей нельзя объявлять заранее: WebGPU отдельно обсуждает
machine-specific rasterization/precision artifacts [5]. Допуски следует выбирать
по конкретному эффекту, а семантические ошибки порядка, clipping и цвета не прятать
в большой общей погрешности.

Для 2026+ перспективны сквозной color pipeline, платформенный cadence и
проверяемые бюджет/ошибки ресурсов. Универсальная обязательность HDR, конкретной
частоты выше 120 Гц, ray tracing или нового rasterizer источниками не установлена.
Решение о таком расширении требует сценария потребителя и измерений. Сравнение
со Skia/Impeller должно оценивать поведение и стоимость на одинаковой сцене;
само имя конкурента не является архитектурным требованием.

## Источники и ограничения

1. [Android Open Source Project — Adaptive refresh rate](https://source.android.com/docs/core/graphics/arr).
2. [Apple — Power down: Improve battery consumption, WWDC22](https://developer.apple.com/videos/play/wwdc2022/10083/), transcript 5:11–9:00.
3. [Android — Enhance graphics with wide color content](https://developer.android.com/training/wide-color-gamut?hl=en).
4. [Apple — Display EDR content with Core Image, Metal, and SwiftUI](https://developer.apple.com/videos/play/wwdc2022/10114/), transcript и example code.
5. [W3C — WebGPU](https://www.w3.org/TR/webgpu/), введение, security и privacy considerations; поздние разделы полного документа не удалось извлечь целиком.
6. [W3C — WCAG 2.2](https://www.w3.org/TR/WCAG22/), критерии 2.1.1, 2.4.11, 2.4.13, 2.5.8.
7. [W3C — Understanding Animation from Interactions](https://www.w3.org/WAI/WCAG22/Understanding/animation-from-interactions.html).
8. [Apple Support — Reduce screen motion](https://support.apple.com/en-us/111781).
9. [Android — InputConnection](https://developer.android.com/reference/android/view/inputmethod/InputConnection?hl=en), Implementing an IME or a text editor; Cursors, selections and compositions.
10. [Apple — Creating Custom Views](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/TextEditing/Tasks/TextViewTask.html), архивное руководство; использовано для marked text/input context, не для оценки актуальности всех API.

Часть динамических Apple documentation pages вернула лишь JavaScript placeholder;
поэтому выводы о EDR и pacing опираются на прочитанные официальные transcripts,
а reduced motion — на Apple Support. Firecrawl query extraction некоторых Android
страниц было пустым; содержимое этих страниц прочитано через Keenable с `hl=en`.
Firecrawl предупредил об усечении большого документа WebGPU и не вернул
целевые поздние разделы; рекомендации по external texture descriptor и recovery
являются проектной оценкой, не пересказом полностью прочитанного нормативного
алгоритма WebGPU. Исследование не включает испытаний устройств, сертификации доступности или замеров
энергии. Прогнозы после даты исследования обозначены как рекомендации.
