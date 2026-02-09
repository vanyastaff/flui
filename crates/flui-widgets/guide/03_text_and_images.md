# ✏️ Text Widgets (Текстовые виджеты)

## Text
```
📦 Text
  └─ RichText -> RenderParagraph
      └─ TextSpan (single style)
          └─ Rendered text
```

**RenderObject:** `RenderParagraph`

**Параметры:**
- `data` - String (текст)
- `style` - TextStyle
- `textAlign` - TextAlign
- `textDirection` - TextDirection
- `softWrap` - перенос строк
- `overflow` - TextOverflow (clip, fade, ellipsis, visible)
- `textScaler` - масштабирование текста
- `maxLines` - максимум строк
- `semanticsLabel` - метка для accessibility
- `textWidthBasis` - TextWidthBasis

**Варианты:**
- `Text()` - обычный текст
- `Text.rich()` - с TextSpan

---

## RichText
```
📦 RichText (Multi-style text)
  └─ RenderParagraph
      └─ TextSpan (tree of styled spans)
          ├─ TextSpan 1 (style 1)
          ├─ TextSpan 2 (style 2)
          └─ WidgetSpan (встроенный виджет)
```

**RenderObject:** `RenderParagraph`

**Параметры:**
- `text` - InlineSpan (TextSpan tree)
- `textAlign`, `textDirection`, `softWrap`, `overflow`, `maxLines`, etc.
- `textScaler` - масштабирование
- `strutStyle` - минимальная высота строки

---

## TextSpan
```
📦 TextSpan (Styled text fragment)
  └─ InlineSpan
      ├─ text: String (опционально)
      ├─ style: TextStyle (опционально)
      ├─ children: List<InlineSpan> (опционально)
      └─ recognizer: GestureRecognizer (опционально)
```

**RenderObject:** Не создает свой RenderObject (рендерится в RenderParagraph)

**Параметры:**
- `text` - текст этого span
- `style` - TextStyle для этого span
- `children` - вложенные InlineSpan
- `recognizer` - TapGestureRecognizer, etc.
- `semanticsLabel` - для accessibility
- `locale` - Locale
- `spellOut` - произносить побуквенно

---

## WidgetSpan
```
📦 WidgetSpan (Widget внутри RichText)
  └─ InlineSpan
      └─ Embedded Widget (baseline-aligned)
```

**RenderObject:** Создает RenderObject для встроенного виджета

**Параметры:**
- `child` - Widget для встраивания
- `alignment` - PlaceholderAlignment
- `baseline` - TextBaseline
- `style` - TextStyle (для контекста)

---

## SelectableText
```
📦 SelectableText (Selectable text)
  └─ EditableText (readOnly: true) -> RenderEditable
      └─ Selectable RenderParagraph
```

**RenderObject:** `RenderEditable`

**Параметры:**
- `data` - String
- `style` - TextStyle
- `textAlign`, `textDirection`, `maxLines`, etc.
- `cursorColor` - цвет курсора при выделении
- `showCursor` - показывать курсор
- `selectionControls` - кастомные controls
- `onSelectionChanged` - callback при выделении

**Варианты:**
- `SelectableText()`
- `SelectableText.rich()` - с TextSpan

---

## DefaultTextStyle
```
📦 DefaultTextStyle (Inherited text style)
  └─ InheritedTheme
      └─ Children (наследуют style)
```

**RenderObject:** `RenderParagraph` (для детей)

**Параметры:**
- `style` - TextStyle по умолчанию
- `textAlign` - выравнивание по умолчанию
- `softWrap` - перенос по умолчанию
- `overflow` - overflow по умолчанию
- `maxLines` - maxLines по умолчанию
- `textWidthBasis` - basis по умолчанию
- `textHeightBehavior` - behavior по умолчанию
- `child` - дочерний виджет

---

## TextStyle
```
📦 TextStyle (Text styling data)
  └─ Immutable configuration
      ├─ Color (color, backgroundColor)
      ├─ Font (fontFamily, fontSize, fontWeight, fontStyle)
      ├─ Decoration (decoration, decorationColor, decorationStyle)
      ├─ Spacing (letterSpacing, wordSpacing, height)
      ├─ Shadows (shadows)
      └─ Features (fontFeatures, fontVariations)
```

**RenderObject:** Не создает RenderObject (используется в RenderParagraph)

**Параметры:**
- **Цвет:** `color`, `backgroundColor`
- **Шрифт:** `fontFamily`, `fontSize`, `fontWeight`, `fontStyle`
- **Декорация:** `decoration`, `decorationColor`, `decorationStyle`, `decorationThickness`
- **Межстрочный:** `height`, `leadingDistribution`
- **Межбуквенный:** `letterSpacing`, `wordSpacing`
- **Тени:** `shadows`
- **Продвинутое:** `fontFeatures`, `fontVariations`, `locale`, `overflow`

---

# 🖼️ Image Widgets (Изображения)

## Image
```
📦 Image
  └─ RawImage -> RenderImage
      └─ ImageProvider (loads image)
          └─ Painted image
```

**RenderObject:** `RenderImage`

**Параметры:**
- `image` - ImageProvider
- `width`, `height` - размеры
- `fit` - BoxFit
- `alignment` - Alignment
- `repeat` - ImageRepeat
- `color` - tint color
- `colorBlendMode` - BlendMode
- `filterQuality` - FilterQuality
- `semanticLabel` - для accessibility
- `excludeFromSemantics` - исключить из semantics

**Варианты:**
- `Image.asset()` - из assets
- `Image.network()` - из URL
- `Image.file()` - из File
- `Image.memory()` - из Uint8List

---

### Image.asset
```
📦 Image.asset (Asset image)
  └─ AssetImage (provider)
      └─ Load from bundle
```

**RenderObject:** `RenderImage`

**Параметры:**
- `name` - String (путь в assets)
- `bundle` - AssetBundle (optional)
- `package` - для package assets
- `width`, `height`, `fit`, `alignment`, etc.

---

### Image.network
```
📦 Image.network (Network image)
  └─ NetworkImage (provider)
      └─ HTTP request + cache
```

**RenderObject:** `RenderImage`

**Параметры:**
- `src` - String (URL)
- `scale` - масштаб изображения
- `headers` - HTTP headers
- `width`, `height`, `fit`, `alignment`, etc.
- `loadingBuilder` - Widget при загрузке
- `errorBuilder` - Widget при ошибке

---

### Image.file
```
📦 Image.file (File image)
  └─ FileImage (provider)
      └─ Load from filesystem
```

**RenderObject:** `RenderImage`

**Параметры:**
- `file` - File
- `scale` - масштаб
- `width`, `height`, `fit`, `alignment`, etc.

---

### Image.memory
```
📦 Image.memory (Memory image)
  └─ MemoryImage (provider)
      └─ Decode from bytes
```

**RenderObject:** `RenderImage`

**Параметры:**
- `bytes` - Uint8List
- `scale` - масштаб
- `width`, `height`, `fit`, `alignment`, etc.

---

## RawImage
```
📦 RawImage (Low-level image)
  └─ RenderImage
      └─ dart:ui Image (already decoded)
```

**RenderObject:** `RenderImage`

**Параметры:**
- `image` - ui.Image (decoded)
- `width`, `height`, `fit`, `alignment`, `repeat`, `color`, `colorBlendMode`, `filterQuality`

---

## Icon
```
📦 Icon
  └─ RichText (uses icon font) -> RenderParagraph
      └─ TextSpan (icon glyph)
```

**RenderObject:** `RenderParagraph`

**Параметры:**
- `icon` - IconData
- `size` - размер иконки
- `color` - цвет
- `semanticLabel` - для accessibility
- `textDirection` - для directional icons

---

## IconTheme
```
📦 IconTheme (Inherited icon theme)
  └─ InheritedTheme
      └─ Children (наследуют IconThemeData)
```

**RenderObject:** Не создает свой RenderObject (InheritedWidget)

**Параметры:**
- `data` - IconThemeData (color, size, opacity)
- `child` - дочерний виджет

---

## ImageIcon
```
📦 ImageIcon (Image as icon)
  └─ Image с ShaderMask -> RenderImage + RenderShaderMask
      └─ ImageProvider (used as icon)
```

**RenderObject:** `RenderImage` + `RenderShaderMask`

**Параметры:**
- `image` - ImageProvider
- `size` - размер
- `color` - цвет (tint)
- `semanticLabel` - для accessibility
