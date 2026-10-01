# Цветовая основа FLUI: внедрение современных библиотек

Дата проверки: 1 октября 2026. Это предложение внедрения, не отчёт о выполненной миграции. Production-код и Cargo в рамках этой записки не изменялись, сборки не запускались.

## Решение, которое стоит принять до 1.0

Внедрить Linebender `color` как внутренний вычислительный фундамент `flui-painting`; сохранить собственный публичный словарь FLUI. Одновременно изменить контракт цвета: хранить пространство источника и конечные float-компоненты со straight alpha, разрешить расширенные RGB вне [0,1], а преобразование в линейные premultiplied рабочие значения сделать явной границей engine. Это позволяет уже сейчас сохранять Display P3 и линейную яркость выше SDR white без собственной реализации матриц и transfer functions. Реальный HDR вывод требует отдельного, реально подключённого presentation path.

Недостаточно заменить четыре u8 на четыре f32, оставив обязательную нормализацию в [0,1]. Недостаточно добавить dependency, если запись display list, кисти текста или vertex conversion снова превращают цвет в u8.

Исследование разделено на вопросы: какие точные библиотеки и API доступны; какие данные сохраняет нынешний FLUI; где проходят source/working/output границы; какие решения нужны для alpha/serde/hash; какие consumer и pixel tests доказывают миграцию. API проверены по опубликованным исходным manifests и rustdoc через Keenable; DynamicColor и спецификация CSS Color независимо прочитаны Firecrawl. Полный HDR на платформе не проверялся.

## Проверенные библиотеки

| Компонент | Проверенная версия и лицензия | Что можно использовать сейчас |
|---|---|---|
| Linebender color | 0.3.3; Apache-2.0 OR MIT; MSRV 1.82 | `AlphaColor<S>`, `PremulColor<S>`, `DynamicColor`, `ColorSpaceTag`, `convert`, `interpolate`, отдельное `clip` |
| Peniko | 0.6.1; Apache-2.0 OR MIT; MSRV 1.85 | Внутренний адаптер градиента; пространство интерполяции, направление hue и alpha policy явно представлены |
| ICU4X icu_segmenter | 2.3.0; Unicode-3.0; MSRV 1.88 | Обновление существующей production segmentation, согласованное с Parley и ICU data |

Источники: [color manifest](https://docs.rs/crate/color/0.3.3/source/Cargo.toml), [peniko manifest](https://docs.rs/crate/peniko/0.6.1/source/Cargo.toml), [ICU manifest](https://docs.rs/crate/icu_segmenter/2.3.0/source/Cargo.toml). Peniko 0.6.1 зависит от color 0.3.3 и kurbo 0.13.1: обновлять совместимую группу, а не объявлять произвольную совместимость major/minor версий.

`DynamicColor` содержит runtime tag, flags и `[f32;4]`, четвёртая компонента — separate alpha. Missing components нужны CSS-интерполяции; обычный FLUI Color должен быть полностью определённым значением. CSS parser можно предоставить отдельно, с ограничением входа и явным разрешением missing values перед записью сцены. [DynamicColor API](https://docs.rs/color/0.3.3/color/struct.DynamicColor.html).

`ColorSpaceTag` включает Srgb, LinearSrgb, DisplayP3, Rec2020, Oklab/Oklch и другие пространства. `clip` является отдельной операцией. Значения integer discriminants могут изменяться в breaking releases: сериализовать собственные FLUI имена, не чужой `repr(u8)`. `convert_absolute` означает преобразование без chromatic adaptation; это **не** шкала абсолютной яркости в нитах. Наличие Rec2020 не означает поддержку PQ/HLG или управление монитором. [ColorSpaceTag](https://docs.rs/color/0.3.3/color/enum.ColorSpaceTag.html).

Peniko Gradient явно хранит `interpolation_cs`, `hue_direction`, `interpolation_alpha_space`; его default — sRGB, тогда как ADR-0098 предлагает Oklab. Поэтому адаптер обязан задавать политику FLUI явно. [Gradient API](https://docs.rs/peniko/0.6.1/peniko/struct.Gradient.html). Не требуется заменять всю paint vocabulary на Peniko ради его цветовой библиотеки.

## Нынешний код и конкретная потеря данных

`crates/flui-painting/src/styling/color.rs` содержит публичные r/g/b/a u8, Eq/Hash и условный derived serde. Float constructor квантует до байтов; Oklab и sRGB transfer реализованы вручную. `to_f32_array` возвращает encoded sRGB, а не linear RGB. Эти имена нельзя молча переопределить, иначе существующий encoded pipeline получит неверные значения.

`crates/flui-painting/src/parley_text/shape.rs` хранит `SpanBrush(Option<Color>)`: owned Color уже может перенести расширение в текст, если весь downstream путь сохранит его. `crates/flui-engine/src/tessellator.rs` превращает Color в vertex array; replay, фильтры, gradient preparation и WGSL требуют согласованной рабочей семантики. Нынешний renderer выбирает SDR UNORM; нельзя включить float/HDR surface только по наличию texture format.

ADR-0098 §7 уже предусматривает private f32 straight sRGB и внутреннее использование color, но status откладывает миграцию. Для tagged source color следует явно supersede соответствующее решение новым ADR, сохранив API closure: публичные signatures не раскрывают color/peniko/kurbo.

Пример потери: Display P3 red, приведённый к byte sRGB при создании paint, уже нельзя восстановить при переносе окна на wide gamut display. Линейный `[2,1,0]` теряет headroom при clamp до записи. Публичная сцена должна сохранять оба значения независимо от текущего дисплея.

## Минимальная производственная миграция

1. В `flui-painting` дать owned `ColorSpace` и private `Color` с проверенными конечными компонентами, alpha [0,1] и нормализацией -0. Начальный поддерживаемый набор: sRGB, Display P3, extended linear sRGB. Добавлять остальные пространства по реально поддержанным constructor/conversion contracts. Сохранить const `rgb/rgba/from_argb` и именованные цвета как sRGB convenience. Дать fallible float/source constructors, явно названный lossy `to_srgba8`, getters вместо mutable полей. Внутри преобразовывать через color, удалить дублированную space math после consumer tests.
2. В собственной Gradient добавить interpolation space и alpha policy. Oklab premultiplied — осознанный default FLUI; `color::DynamicColor::interpolate` делает precomputation вне pixel loops. При использовании Peniko конвертировать только на внутренней границе. LUT имеет ограниченный размер и задаваемую погрешность; не строить его заново для каждого пикселя.
3. Провести сохранение Color через Paint, Shader, DisplayList, SpanBrush, animation, layer и cache keys. Это изменения существующих путей, а не новый необслуживаемый pub helper. Запретить промежуточные byte conversions кроме явно SDR texture/output операций.
4. Отдельно мигрировать working pipeline: source conversion → `PremulColor<LinearSrgb>` → эффекты и compositing в согласованном float target → SDR output encoding. Изменить tessellator/vertex, replay gradient preparation, text tint, image sampling metadata, advanced blend, color matrix и shaders вместе. Сначала работающий линейный SDR readback; HDR presentation включать только после platform negotiation. Пользователь должен получить typed unsupported outcome, если native HDR режим отсутствует, либо явно выбранный SDR mapping.
5. В host/platform contract зафиксировать display encoding, reference white и headroom, поколение display configuration, фактически выбранный режим и mapping policy. Float format сам по себе не сообщает эти данные. При перемещении окна пересчитывается presentation, источник сцены сохраняется. ICC images требуют отдельного profile conversion provider: Linebender color не заявлен здесь как ICC CMS.

Это одна последовательная migration, которую можно делить на reviewable изменения. Первое изменение может уже хранить P3/extended values и корректно выводить явно mapped SDR; заявление «HDR поддержан» допустимо только после end-to-end platform проверки. [Apple EDR](https://developer.apple.com/videos/play/wwdc2022/10114/) и [Android wide gamut](https://developer.android.com/training/wide-color-gamut?hl=en) независимо подтверждают необходимость согласовать content encoding и native presentation. [CSS Color 4](https://www.w3.org/TR/css-color-4/#color-conversion) различает conversion и gamut mapping; clipping нельзя скрывать в каждом constructor.

## Совместимость и обязательные доказательства

Eq/Hash — equality представления, не perceptual equivalence между пространствами. Запрет NaN/Infinity и canonical -0 позволяют согласованные bit keys. Не использовать approximate equality в Hash. Преобразование конечных входов также может overflow: валидировать результат, сообщать ошибку либо документированный bounded output mapping. Alpha zero не должен разрушать straight source components, нужные будущей интерполяции; premul рабочий zero нормализуется отдельно.

Serde: зафиксировать versioned FLUI wire vocabulary `space/components/alpha`, принимать прежний byte формат только отдельным совместимым decoder при наличии нужды. Derived serde внешнего типа не обеспечивает стабильную wire схему FLUI. Constructor validation должна действовать и при deserialize. Constant colors и compile-time consumers проверять из tests/ публичным API; u8 field literals — намеренное pre-1.0 breaking изменение с changelog.

Необходимые behavioral tables: byte convenience roundtrip и прежний SDR результат; source P3 и extended linear survive DisplayList/text brush; -0/Eq/Hash/serde и malformed inputs; прозрачный красный→непрозрачный синий без утечки hidden red; разные interpolation spaces дают разные контрольные значения. Конверсии проверять независимыми опубликованными reference vectors, не вызовом того же helper в oracle.

GPU readbacks: solid/gradient/text имеют одинаковую source interpretation; 50% linear white over black даёт около 0.735 encoded sRGB, поэтому проверка отличает encoded blending; float target сохраняет >1 до output; P3 input преобразуется до правильного SDR output без двойного transfer; blur, opacity layer, advanced blend, color matrix и image alpha проверяются также в конкуренции. CPU oracle текущего Color::blend, повторяющий encoded семантику, не доказывает линейную миграцию. Native EDR/wide gamut smoke отдельно: SDR white стабилен, headroom выше 1 заметен, изменение display поколения не меняет source Color. Golden SDR картинка не доказывает HDR.

## ICU: уже используемая основа, которую нужно развивать

Root Cargo уже содержит icu_properties 2.3 и icu_segmenter 2.1.2. `text_boundaries.rs` использует ICU compiled data для grapheme/word, `text_layout/host.rs` — Script properties. Следовательно задача — согласованное обновление существующего контракта, не добавление ICU как будущей возможности. Текущий `new_for_non_complex_scripts` документированно сегментирует Thai/CJK по символам; полноценный word selection нуждается в deliberate policy и соответствующих dictionary/model data, с измеренным размером и временем.

ICU4X 2.3 вводит Unicode 17 default для grapheme/word/sentence и отдельные обновлённые line constructors; preview API не следует превращать в стабильное публичное обещание. [Официальный релиз Unicode](https://blog.unicode.org/2026/08/icu4x-23-released.html). Зафиксировать Unicode/data revision, byte-offset contract, связь caret и shaped clusters, selectable locale word policy. Tests: ZWJ emoji, combining marks, flags, Thai word selection, bidi caret/hit-test и wrapping на узкой ширине; они должны проверять пользовательское движение/selection, а не только наличие нового dependency. ICU segmentation не заменяет shaping, bidi и IME transaction protocol.

Уверенность высокая в проверенных версиях, API и потерях byte storage; средняя в размере полного pipeline изменения без компиляции. Предложенный минимальный source contract пересматривается, если реальный image/video ingress требует ICC или абсолютной PQ/HLG шкалы раньше: расширять source description и provider, не выдавать Rec2020 tag за полную HDR систему.
