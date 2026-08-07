# Альтернативы wgpu и состояние рендера UI в Rust-экосистеме — 2026-08-06

**Модель:** Qwen (Qwen Code). **Метод:** живые данные crates.io MCP (версии/даты/зависимости),
changelog'и upstream'ов, релизные заметки wgpu 29/30, квартальный отчёт Linebender 2026 Q1.
Всё с пометкой «live» проверено запросом 2026-08-06; остальное — оценка.

Контекст: `flui-engine` пинит wgpu 29 / glyphon 0.11 / naga_oil 0.22 / wgpu-profiler 0.27.
Аудит движка: [`docs/audits/2026-08-06-flui-engine-audit.md`](../audits/2026-08-06-flui-engine-audit.md).

---

## 1. wgpu: статус и что несёт wgpu 30

**Live-факты.** wgpu **30.0.0** вышел 2026-07-01 (crates.io: 30.4M загрузок всего, 9.0M за
последний период). wgpu 29.0.0 — 2026-03-19. FLUI на 29: миграция 25→29 уже сделана, новый
`CurrentSurfaceTexture`-enum полностью разобран в `renderer.rs:1258-1312`.

### Что в wgpu 30 важно именно для UI-движка

1. **Color-space API для поверхностей — закрывает необходимость угадывать HDR-режим поверхности эвристикой.**
   - `SurfaceConfiguration` получил **обязательное** поле `color_space: SurfaceColorSpace`;
     `SurfaceColorSpace::Auto` сохраняет историческое поведение (extended linear scRGB для
     `Rgba16Float` где поддержано, иначе sRGB; никогда wide-gamut/HDR без запроса).
   - `SurfaceCapabilities::format_capabilities` — поддерживаемые цветовые пространства на
     формат (в hal: `Vec<SurfaceFormatCapabilities>` вместо `Vec<TextureFormat>`).
   - `Surface::display_hdr_info()` — **реальные** характеристики дисплея: яркость в нитах,
     EDR headroom, primaries, bit depth, `tone_map_headroom()`. Это ровно то, чего не хватает
     `check_hdr_support` (`renderer.rs:127-135`), который сегодня возвращает `true` просто по
     backend==Metal|Dx12.
   - Поддержка: `Srgb` — все бэкенды; `ExtendedSrgb`/`DisplayP3` — Vulkan/Metal/WebGPU;
     `Bt2100Pq` (HDR10) — Vulkan/DX12/Metal; расширенные пространства зависят от
     драйвера/платформы.
2. **`VertexState::buffers: &[Option<VertexBufferLayout>]`** — breaking для всех pipeline
   descriptor'ов (в FLUI это каждая запись `PipelineBuilder`/`pipelines.rs`).
3. **Integer shader I/O больше не flat по умолчанию** — WGSL-шейдеры с целочисленными
   varying'ами требуют явного `@interpolate(flat)` (пройтись по `shaders/`).
4. **Пустые buffer slices разрешены**, но `BufferBinding`/`BindingResource` теперь
   `TryFrom<BufferSlice>` (fallible); `BufferSlice::size()` → `u64`.
5. **`StagingBelt::finish_and_recall_on_submit`** — убирает ручной `recall()` после submit;
   упрощение для CPU-upload путей.
6. **`Device::create_texture_from_hal`** теперь требует явный `initial_state: TextureUses` —
   важно для будущего импорта внешних текстур (видео/платформенные текстуры, см.
   `ExternalTextureRegistry`).
7. Multi-planar `copy_texture_to_texture` (NV12 plane → R8/RG8) — задел под видео-текстуры.

### Стоимость миграции 29→30 для FLUI

Механическая: `color_space` во всех `SurfaceConfiguration` (одно место — `renderer.rs:475-500`,
плюс `recover`), `Some(&layout)` в vertex states по `pipelines.rs`, ревизия integer I/O в
WGSL. Смысловая работа — одна: HDR-стратегия (§4.1). Ожидаемый бонус: glyphon 0.12 становится
доступен (он уже на wgpu 30 — live).

---

## 2. Альтернативы wgpu как графической абстракции

| Крейт | Версия (live) | Обновлено (live) | Что это | Применимость к FLUI |
|---|---|---|---|---|
| **wgpu** | 30.0.0 | 2026-07-01 | safe cross-API абстракция (Vulkan/Metal/DX12/WebGPU/GL) | текущий выбор; экосистемный стандарт де-факто |
| **blade-graphics** | 0.8.4 | 2026-04-18 | минималистичная абстракция от kvark (автор wgpu) | нишевое adoption; API нестабилен; менять нет причин |
| **ash** (Vulkan) / **metal-rs** / **glow** (GL) | — | — | прямые API без абстракции | полный контроль, полная стоимость портирования ×4 платформы — противоречит стратегии |
| **softbuffer** | — | — | CPU blit на окно (не рендерер) | только как fallback-поверхность |

Вывод: **реальной альтернативы wgpu как абстракции для кроссплатформенного UI-движка в чистом
Rust нет** — blade интересен как мнение, прямые API — только ценой 4× работы. Настоящий выбор
происходит уровнем выше: чем рендерить 2D-контент (см. §3).

---

## 3. Рендерерные альтернативы (уровень «2D-контент → GPU»)

### Vello — главный кандидат «второго бэкенда»

**Live:** vello **0.9.0** (2026-05-15), на wgpu 29; 658.7K загрузок всего / 385.5K за период.
«No longer experimental» с 0.3.0 (2024-10), статус — alpha, «direction proven».

Что это: compute-centric 2D векторный рендерер поверх wgpu (Path/Gradient/Image/Glyphs →
GPU-compute pipeline → один композитный pass). Не заменяет wgpu — строится на нём.

Свежее (changelog 0.6–0.9 + отчёт Linebender 2026 Q1):

- **Vello Hybrid — «roughly beta quality»**; **Vello CPU** — без-GPU растеризация (общий
  релизный трек `sparse strips`); Masonry (виджет-слой Xilem) уже мигрировал на rendering
  abstraction `imaging` и умеет в Vello CPU.
- 0.9.0: **image atlas residency сохраняется между рендерами** (нет повторных rebuild/upload),
  bicubic image sampling (`ImageQuality::High`), skrifa 0.42 (VARC-глифы), `font_embolden`,
  `brush_transform`.
- 0.6.0: `push_clip_layer`, `push_luminance_mask_layer`, `register_texture` (композит
  чужих `wgpu::Texture` в сцену Vello).

Применимость к FLUI: `RasterBackend`-трейт прямо называет Vello точкой подмены
(`src/raster.rs:1-12`). Реалистичный сценарий — не замена wgpu-бэкенда, а (а) второй бэкенд
для сравнения/переносимости и (б) **CPU-эталон для golden-тестов**: FLUI опускает
`DisplayList`/`LayerTree` в `vello::Scene` тем же visitor-паттерном, что `Backend`
(`src/wgpu/backend.rs`), и получает детерминированный CPU-рендер без WARP. Ограничения: у
Vello своя модель (нет saveLayer-семантики Flutter 1:1, фильтры — через blur/clip/luminance
слои, advanced blend покрывается не весь 28-режимный набор FLUI) — паритет частичный, как
оракл для подмножества.

### Skia через FFI — skia-safe

**Live:** skia-safe **0.99.0** (2026-06-19), 3.3M загрузок. Полный Skia (тот самый, что под
Flutter до Impeller): máxima feature completeness, PDF/SVG, весь blend/filter-набор. Цена:
C++ сборка, FFI, размер, время CI. Для FLUI это анти-стратегия («чистый Rust», leapfrog-зоны),
но остаётся **бенчмарком полноты поведения** — если где-то нужен эталон «как делает Flutter»,
Skia ближе всех.

### femtovg и tiny-skia

**Live:** femtovg **0.26.0** (2026-07-20, 2.3M загрузок) — immediate-mode canvas поверх
OpenGL(ES) (ретейнер-подход, рендерер Slint). tiny-skia **0.12.0** (2026-02-02, 40.4M
загрузок) — чистый CPU-растеризатор подмножества Skia (движок resvg).

- femtovg для FLUI бесполезен как бэкенд (GL-only ниша), интересен только как референс
  immediate-mode canvas API.
- **tiny-skia — самый дешёвый способ получить CPU-оракл в CI без GPU**: детерминированный,
  pure Rust, проверенный resvg-проектом. Подмножество меньше Vello (нет blur-фильтров,
  advanced blends ограничены), но для shape/gradient/text-free сцен это рабочий второй
  наблюдатель — прямое подспорье для боли headless-паритета (§3.2.1 аудита).

---

## 4. Новости рендера UI в Rust-среде (2025 → 2026-08)

### 4.1. HDR/wide-gamut стало first-class в wgpu (см. §1)

Экосистемный сдвиг: до wgpu 30 HDR-вывод делался вручную через хаки swapchain-форматов; теперь
есть декларативный `SurfaceColorSpace` + метаданные дисплея. Для FLUI это одновременно и
закрытие текущей эвристики, и новая продуктовая поверхность (tone mapping headroom, EDR).

### 4.2. Экосистема консолидируется вокруг стека Linebender

Отчёт Linebender 2026 Q1 (Raph Levien, 2026-04-19) + live-версии:

- **Bevy перешёл на Parley/Fontique для текста** — крупный экосистемный сигнал: центр тяжести
  текстового стека смещается от cosmic-text/glyphon к Parley + Fontique + Skrifa.
- **Glifo** (бывш. `parley_draw`, переехал в репозиторий Vello) — рендеринг глифов поверх
  Skrifa: color emoji, ink skipping для подчёркиваний, atlas-based glyph caching в активной
  итерации. Долгосрочная цель — независимость от Vello-рендера.
- **Parley 0.11** (live, 2026-06-26): перечисление системных шрифтов macOS через CoreText,
  загрузка шрифтов из системы и путей, CSS `text-indent`, все AccessKit text properties.
- **cosmic-text 0.19** (live, 2026-04-22) жив и используется в COSMIC/glyphon — стек FLUI не
  мёртв, но темп инноваций сейчас у Parley.
- **glyphon 0.12** (live, 2026-07-09) требует **wgpu 30** — апгрейд FLUI заблокирован
  wgpu-пином; содержимое релиза проверить перед миграцией (changelog в репозитории отсутствует).
- **fearless_simd 0.6** (2026-07-11): AVX-512 + опции отключения — ускорение CPU-путей
  Linebender (Vello CPU, tessellation-подобные задачи).
- **subduction** (новый крейт, Bruce Mitchener): интеграция с системными композиторами
  (AppKit-виджеты, wgpu-поверхность рядом с нативным UI) — направление, важное для
  platform-view-будущего FLUI.

### 4.3. Тренд на renderer-agnostic seams

Сразу три независимых абстракции «виджет-фреймворк ↔ рендерер»: `imaging` (Masonry/forest-rs),
`AnyRender` (Blitz), Vello CPU/Hybrid/Classic под одним API. Экосистема голосует за то же, что
FLUI заложил в `RasterBackend` — но чужие seams уже живут в multi-backend реальности (CPU/GPU/
hybrid), а seam FLUI пока синхронный `render_scene(&Scene)` под одну модель. Это стоит учесть
при проектировании подключения `RasterOwner` (#559): форма seam'а может потребовать
present-ориентированного, а не scene-ориентированного интерфейса.

### 4.4. Xilem/Masonry

Xilem остаётся экспериментальным (5.5k звёзд, MSRV 1.92, web-бэкенд + Masonry-бэкенд), но
Masonry получил новую систему layout (`xilem#1560`), `ui-events` для IME (меньше зависимости
от winit, вплоть до встраивания в VST-плагины через baseview) и набор виджетов
(Svg/Divider/CollapsePanel/StepInput/RadioButtons/Switch/Clip/Split). Для FLUI интересен не как
конкурент виджетного слоя, а как полигон layout/IME-решений.

### 4.5. GPUI (Zed)

Репозиторий держит `.gpui/` как read-only референс. Публичных релизных новостей по рендеру
GPUI за период в блоге Zed не найдено (проверено 2026-08-06); значимых внешних изменений для
учёта нет.

---

## 5. Импликации для FLUI

1. **wgpu 29 → 30 — запланировать как отдельную миграцию** (оценка стоимости в §1):
   обязательный `color_space` + `Some(&layout)` в vertex states + ревизия integer I/O в WGSL.
   Разблокирует glyphon 0.12 и, главное, даёт реальные HDR-возможности вместо эвристики.
2. **HDR-стратегию решать при миграции, а не после**: `SurfaceColorSpace::Auto` сохраняет
   текущее поведение (можно мигрировать без визуальных изменений), а `display_hdr_info` —
   точка входа для правильного tone mapping. Это продуктовое решение (Flutter/Impeller тут не
   оракул — у Impeller HDR-путь свой), попадает в leapfrog-зону ADR-0027.
3. **Vello — держать как кандидата второго бэкенда и CPU-оракла**; конкретный эксперимент:
   visitor `DisplayList → vello::Scene` на подмножестве (shapes/gradients/clips) как второй
   наблюдатель в golden-контуре. Это же лечит DoD-боль «golden-инструмент лжёт»
   (§3.2.1 аудита) независимым источником пикселей.
4. **tiny-skia — самый дешёвый CPU-оракл** для CI без WARP на shape/gradient-подмножестве.
5. **Текстовый стек — развилка по шву «текст vs Command IR»** (в исходниках помечена `T11`, расшифрована в `crates/flui-engine/ARCHITECTURE.md:276`): glyphon+cosmic-text (текущий, живой)
   против Parley+Fontique+Skrifa (+Glifo) (растущий центр экосистемы, Bevy уже мигрировал).
   Решение влияет на IR-швов текста (текст вне Command IR, §3.2.2 аудита) и на future
   Vello-бэкенд (Vello рендерит глифы через Parley/Skrifa-стек).
6. **Ничего из увиденного не отменяет выбор wgpu** — но подтверждает, что ценность FLUI
   смещается к тому, что *над* абстракцией: record/replay IR, seam'ы и верность поведения —
   именно их переносит любой будущий бэкенд.
