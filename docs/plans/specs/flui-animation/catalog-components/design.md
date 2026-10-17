# flui-animation / catalog-components — design

- **Статус:** утверждён (см. [requirements.md](requirements.md))
- **Дата:** 2026-10-17
- **База:** `main` 4967778a1.

## Решения

| # | Решение | Почему | Кем |
|---|---|---|---|
| K1 | Rive не делаем | Нет зрелого Rust-рантайма: `rive-rs` не опубликован и требует сборки C++ | владелец |
| K2 | Lottie не в этом раунде | Решение владельца после разбора пробелов отрисовки (requirements «Вне объёма») | владелец |
| K3 | `AnimatedIcon` — ключевые кадры векторных путей плюс словарь эффектов в духе SF Symbols для любой иконки | Набор Flutter заморожен на 7 парах без конвейера авторинга; оси вариативного шрифта требуют поставки шрифта и перерастеризации глифов | владелец |
| K4 | Индикаторы M3 Expressive с портом `RoundedPolygon`/`Morph` (androidx.graphics.shapes, Apache-2.0) | Морф форм нужен `LoadingIndicator` и пригодится формам M3 Expressive вообще | владелец |
| K5 | Частицы: декларативный излучатель, `emit_burst` и пресеты; CPU и один `draw_atlas` | UI-масштаб — тысячи частиц; детерминированно и тестируемо без GPU | владелец |
| K6 | Новых крейтов нет: геометрия в `flui-painting`, общие компоненты — модули `flui-widgets`, Material и Cupertino — в своих пакетах | «Layers, not micro-crates»; решение C2 прошлого раунда; тяжёлых зависимостей в этом наборе нет | делегировано |
| K7 | Перестановка фиксируется один раз при отпускании, `to` уже учитывает удалённый элемент; есть сетка и sliver; живой ход жеста — необязательный `on_reorder_preview` | Ошибка на единицу в `newIndex` была главной ошибкой пользователей Flutter, и Flutter сменил API; живая перестановка Compose-библиотеки требует синхронного обновления модели, иначе мерцание | делегировано |
| K8 | Карусель на модели ключевых линий (Compose) с пресетами весов (Flutter); автопрокрутка opt-in с гарантиями WCAG 2.2.2 и шаблона W3C | Ключевые линии — общий движок для всех раскладок M3; ни Compose, ни Flutter не дают автопрокрутку, а сторонние реализации обычно нарушают 2.2.2 — «батарейки» здесь значат правильную реализацию | делегировано |
| K9 | Конвертер иконок — `cargo xtask icon-import` (AVD XML и пары SVG-путей); данные в исходниках как `const` | Каталог должен расти; Rust-инструменты в xtask, без скриптов | делегировано |
| K10 | ГПСЧ частиц — `fastrand` с сидом (workspace-зависимость) | Зрелый небольшой крейт вместо самописного; сид даёт воспроизводимые кадры | делегировано |
| K11 | `PageIndicator` — общий в `flui-widgets`, `CupertinoPageControl` — стиль поверх | У Material нет отдельного компонента индикатора страниц; поведение и семантика общие | делегировано |
| K12 | Отложенный multi-drag распознаватель — в `flui-interaction`, `LongPressDraggable<T>` — в `flui-widgets` | Это недостающая половина существующего `Draggable` (названа в его модуле), нужна и вне списков | делегировано |

Первичные источники (Compose `Carousel.kt`, `Keylines.kt`, `LoadingIndicator.kt`,
`ProgressIndicator.kt`; Flutter `reorderable_list.dart`, `carousel.dart`,
`animated_icons.dart`; W3C APG carousel; dnd-kit; SF Symbols) перечислены в issue спека;
значения ниже закрепляются тестами, а не ссылкой.

## Размещение

| Что | Где | Слой `flui-widgets` |
|---|---|---|
| `Path::{length, trim, transform, lerp}`, `RoundedPolygon`, `Morph`, `MaterialShapes` | `flui-painting` `paint::shapes` | — |
| Отложенный multi-drag распознаватель | `flui-interaction` `recognizers` | — |
| `AnimatedIconData`, `AnimatedIcon`, `SymbolEffect` | `flui-widgets` `icon` | верхний (`icon`) |
| `LongPressDraggable<T>`, `EdgeAutoScroller` | `flui-widgets` `interaction`, `scroll` | `interaction`, `scroll` |
| `ReorderableList`, `ReorderableGrid`, `SliverReorderableList`, `ReorderHandle` | `flui-widgets` `scroll::reorderable` | `scroll` |
| `Carousel`, `CarouselController`, `CarouselLayout`, `CarouselItemScope`, `CarouselFling`, `CarouselAutoplay` | `flui-widgets` `scroll::carousel` (+ `RenderSliverCarousel` в `flui-objects`) | `scroll` |
| `PageIndicator` | `flui-widgets` `controls` | `controls` |
| `ParticleEmitter`, `ParticleController`, пресеты | `flui-widgets` `motion::particles` | `motion` |
| `CircularProgressIndicator`, волнистые варианты, `LoadingIndicator`, `ContainedLoadingIndicator`, `CarouselView`, Material-пары иконок | `flui-material` | пакет |
| `CupertinoPageControl` | `flui-cupertino` | пакет |
| `icon-import` | `tools/xtask` | — |

## Публичный API (эскиз)

```rust
// flui-painting
impl Path {
    pub fn length(&self) -> f64;
    pub fn trim(&self, start: f64, end: f64) -> Path;      // доли длины; start > end — перенос через 0
    pub fn transform(&self, m: &Matrix4) -> Path;
    pub fn lerp(a: &Path, b: &Path, t: f64) -> Option<Path>; // None для несовместимых
}
pub struct RoundedPolygon { .. }   // new(vertices, rounding), star(..), circle(..), rectangle(..)
pub struct Morph { .. }            // new(&a, &b), path(t) -> Path
pub mod material_shapes { pub fn soft_burst() -> RoundedPolygon; /* cookie_9, pentagon, pill, sunny, cookie_4, oval, circle */ }

// flui-widgets::icon
pub struct AnimatedIconData { .. }         // const-конструируемые ключевые кадры путей
AnimatedIcon::new(&PLAY_PAUSE, progress).size(24.0).semantic_label("Play");
Icon::new(Icons::FAVORITE).symbol_effect(SymbolEffect::bounce().on_change(count));
Icon::new(Icons::WIFI).symbol_effect(SymbolEffect::pulse().active(searching));
SymbolEffect::replace(other_icon);   // кроссфейд-переход между иконками

// flui-widgets::scroll
ReorderableList::builder(items.len(), |ctx, i| row(i).key(items[i].id))
    .on_reorder(|from, to| items.move_item(from, to))   // to — после удаления
    .on_reorder_preview(|from, over| ..)
    .drag_start(DragStart::Platform);                    // Handle | LongPress | Platform
Carousel::builder(n, |ctx, scope: &CarouselItemScope, i| ..)
    .layout(CarouselLayout::MultiBrowse { min_small: 40.0, max_small: 56.0 })
    .fling(CarouselFling::SingleAdvance)
    .autoplay(CarouselAutoplay::every(Duration::from_secs(6)))   // opt-in, ≥ 5 с
    .controller(controller);

// flui-widgets::controls
PageIndicator::new(page_controller.position()).count(5);

// flui-widgets::motion
ParticleEmitter::new(ParticleConfig::confetti()).controller(confetti.clone());
confetti.emit_burst_at(tap_point, 120);
```

## Значения по умолчанию

| Компонент | Значение |
|---|---|
| Поднятие элемента списка | высота 0→6, масштаб 1→1.02, 150 мс |
| Сдвиг соседей / посадка броска | 250 мс, `EASE_IN_OUT` |
| Long-press запуска перетаскивания | 500 мс (тайм-аут long-press платформы, где он известен) |
| Карусель `MultiBrowse` | малый элемент 40–56 dp, средний выводится из ключевых линий |
| Карусель `SingleAdvance` | пружина medium-low, один элемент за бросок |
| Автопрокрутка карусели | период ≥ 5 с (меньший отклоняется), по умолчанию выключена |
| Линейный индикатор | толщина 4 dp, зазор трека 4 dp, stop-точка 4 dp, ширина 240 dp, неопределённый цикл 1750 мс |
| Круговой индикатор | 40 dp (волнистый 48 dp), толщина 4 dp, цикл 6000 мс, дуга 0.1–0.87 |
| Волна | линейная: длина 40/20 dp, амплитуда 3 dp; круговая: длина 15 dp, амплитуда 1.6 dp; скорость — длина волны в секунду; амплитуда 0 при ≤10 % и ≥95 %, переход 500 мс |
| `LoadingIndicator` | контейнер 48 dp, форма 38 dp, морф каждые 650 мс, пружина 0.6/200, +90° за морф, оборот за 4666 мс |
| `PageIndicator` | точка 8 dp, промежуток 8 dp, окно 5 точек при многих страницах |
| Эффекты иконок | bounce 0.5 с, pulse период 1 с, replace 0.25 с кроссфейд, draw_on 0.5 с |
| Частицы | ёмкость по умолчанию 512, верхняя граница 4096 |

## Reduce motion

Под `MotionOverride::Reduce`; каждая строка — тест (M1).

| Компонент | Поведение |
|---|---|
| `AnimatedIcon` | при смене прогресса — кроссфейд между крайними кадрами вместо морфа |
| `SymbolEffect` дискретные и бесконечные | статичная иконка |
| `SymbolEffect::replace`, `appear`, `disappear` | кроссфейд 0.15 с |
| Поднятие и сдвиг в списке | без масштаба; соседи и посадка — мгновенно; автопрокрутка у краёв работает (функциональна) |
| Карусель | автопрокрутка выключена; привязка после броска — мгновенно |
| Волнистые индикаторы | амплитуда 0 (плоский Expressive) |
| Неопределённые индикаторы | статичный видимый кадр (ADR-0192) |
| `LoadingIndicator` | одна статичная форма |
| Определённый прогресс | значение прыгает, без твина |
| `PageIndicator` | точки прыгают за позицией без анимации перехода |
| Частицы | не излучаются; уже живущие исчезают в следующем кадре |

## Тестовые швы

Один стол на группу, через публичный API:

- **G:** чистые таблицы в `flui-painting`: длина окружности и квадрата, `trim` половины,
  перенос через 0, вырожденный путь, `lerp` совместимых и `None` для несовместимых, морф
  `Morph::path(0/1)` совпадает с исходными формами, `path(0.5)` строго между по площади.
- **I:** кадр `AnimatedIcon` на 0, 0.5, 1 читается из display list (контрольные точки пути);
  RTL зеркалит; каждый эффект — строка (масштаб `bounce` на пике, непрозрачность `pulse` на
  полупериоде, `draw_on` — длина нарисованного пути); `icon-import` — фикстура AVD → данные,
  несовместимая пара → ошибка.
- **R:** перестановка мышью (ручка) и касанием (long-press), `on_reorder` один раз с
  правильным `to` в обе стороны, сдвиг соседа строго между на 125 мс, автопрокрутка у края,
  клавиатура (подъём, две стрелки, бросок; Esc), действия семантики и их отсутствие на
  границах, отмена при размонтировании без колбэка, сетка в двух осях, RTL.
- **C:** маска элемента строго между ключевыми линиями на промежуточной прокрутке;
  `SingleAdvance` при большой скорости переходит ровно на один элемент; автопрокрутка
  листает после периода, останавливается на фокусе и не возобновляется, отказывает периоду
  < 5 с; семантика «n из N»; меньше элементов, чем слотов.
- **P:** записанные кадры индикаторов: зазор и stop-точка; волна на 50 % прогресса и плоская
  на 5 %; морф `LoadingIndicator` на 325 мс строго между формами; `PageIndicator` на дробной
  позиции 1.5 — две точки наполовину; действия «регулируемый»; скрытие при одной странице.
- **X:** с сидом два прогона дают одинаковые записанные атласы; всплеск на 120 частиц
  создаёт 120 спрайтов в одном `draw_atlas`; тик останавливается после смерти последней
  частицы; без излучения под reduce motion; частицы вне семантики.

Каждый новый `RenderBox`/`RenderSliver` — строка `RENDER_OBJECT_TYPES` и `harness_*`.

## Риски

- Ленивый sliver карусели и перетаскивание в ленивом списке требуют, чтобы перетаскиваемый
  или маскируемый элемент жил вне построенного диапазона: держим его отдельным ребёнком
  sliver на время жеста (как призраки `AnimatedList`), а не расширяем диапазон.
- Long-press запуска конкурирует с прокруткой и тапом: распознаватель вступает в арену
  жестов после тайм-аута, и прокрутка до тайм-аута его отменяет.
- Маскирование элементов карусели на каждом кадре: скруглённый прямоугольник через clip, без
  offscreen-слоя.
- Порт `RoundedPolygon`/`Morph` — объёмная геометрия; закрепляем таблицей против значений
  исходника androidx, а не визуально.
