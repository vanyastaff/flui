# 📝 Input Widgets (Виджеты ввода)

## TextField
```
📦 TextField
  └─ EditableText -> RenderEditable
      └─ RenderEditable
          ├─ InputDecoration (border, label, hint, etc.)
          └─ Text input + cursor
```

**RenderObject:** `RenderEditable`

**Параметры (основные):**
- `controller` - TextEditingController
- `focusNode` - FocusNode
- `decoration` - InputDecoration
- `keyboardType` - TextInputType
- `textInputAction` - TextInputAction
- `textCapitalization` - TextCapitalization
- `style` - TextStyle
- `textAlign` - TextAlign
- `textDirection` - TextDirection
- `readOnly` - bool
- `obscureText` - bool (для паролей)
- `autocorrect` - bool
- `maxLines` - int (null = unlimited)
- `minLines` - int
- `expands` - bool
- `maxLength` - int
- `onChanged` - void Function(String)
- `onSubmitted` - void Function(String)
- `onEditingComplete` - VoidCallback
- `enabled` - bool
- `cursorColor` - Color
- `keyboardAppearance` - Brightness
- `scrollPadding` - EdgeInsets
- `enableInteractiveSelection` - bool
- `buildCounter` - Widget? Function(...)

---

## TextFormField
```
📦 TextFormField (Form-integrated TextField)
  └─ FormField<String>
      └─ TextField -> RenderEditable
          └─ Validation + save/restore
```

**RenderObject:** `RenderEditable`

**Параметры:** Те же что у TextField + дополнительные:
- `initialValue` - String
- `validator` - String? Function(String?)
- `onSaved` - void Function(String?)
- `autovalidateMode` - AutovalidateMode
- `restorationId` - String

---

## Checkbox
```
📦 Checkbox
  └─ Material (checkbox shape + ripple) -> RenderInkFeatures
      └─ CustomPaint -> RenderCustomPaint
          └─ Checkmark animation
```

**RenderObject:** `RenderCustomPaint` (для checkmark) + `RenderInkFeatures` (для ripple)

**Параметры:**
- `value` - bool? (null = indeterminate)
- `onChanged` - void Function(bool?)
- `tristate` - bool (разрешить null)
- `activeColor` - Color (checked color)
- `checkColor` - Color (checkmark color)
- `fillColor` - MaterialStateProperty<Color?>
- `focusColor`, `hoverColor` - Color
- `overlayColor` - MaterialStateProperty<Color?>
- `splashRadius` - double
- `materialTapTargetSize` - MaterialTapTargetSize
- `visualDensity` - VisualDensity
- `focusNode` - FocusNode
- `autofocus` - bool
- `shape` - OutlinedBorder
- `side` - BorderSide
- `isError` - bool

---

## CheckboxListTile
```
📦 CheckboxListTile (ListTile + Checkbox)
  └─ MergeSemantics
      └─ ListTile
          └─ Checkbox
```

**RenderObject:** Комбинация RenderObject из ListTile и Checkbox

**Параметры:**
- `value`, `onChanged`, `tristate` - как у Checkbox
- `title` - Widget (главный текст)
- `subtitle` - Widget (подзаголовок)
- `secondary` - Widget (leading/trailing icon)
- `isThreeLine` - bool
- `dense` - bool
- `selected` - bool
- `controlAffinity` - ListTileControlAffinity
- `activeColor`, `checkColor`, `tileColor`, `selectedTileColor`
- `contentPadding` - EdgeInsets
- `enabled` - bool

---

## Radio
```
📦 Radio<T>
  └─ Material (radio button shape + ripple) -> RenderInkFeatures
      └─ CustomPaint -> RenderCustomPaint
          └─ Filled circle animation
```

**RenderObject:** `RenderCustomPaint` (для circle) + `RenderInkFeatures` (для ripple)

**Параметры:**
- `value` - T (значение этой радиокнопки)
- `groupValue` - T? (текущее выбранное значение)
- `onChanged` - void Function(T?)
- `toggleable` - bool (можно ли снять выбор)
- `activeColor` - Color
- `fillColor` - MaterialStateProperty<Color?>
- `focusColor`, `hoverColor` - Color
- `overlayColor` - MaterialStateProperty<Color?>
- `splashRadius` - double
- `materialTapTargetSize` - MaterialTapTargetSize
- `visualDensity` - VisualDensity
- `focusNode` - FocusNode
- `autofocus` - bool

---

## RadioListTile
```
📦 RadioListTile<T> (ListTile + Radio)
  └─ MergeSemantics
      └─ ListTile
          └─ Radio<T>
```

**RenderObject:** Комбинация RenderObject из ListTile и Radio

**Параметры:**
- `value`, `groupValue`, `onChanged`, `toggleable` - как у Radio
- `title`, `subtitle`, `secondary`, `isThreeLine`, `dense`, `selected` - как у CheckboxListTile
- `controlAffinity`, `activeColor`, `tileColor`, `selectedTileColor`, `contentPadding`, `enabled`

---

## Switch
```
📦 Switch
  └─ Material (track + thumb) -> RenderInkFeatures
      └─ CustomPaint -> RenderCustomPaint
          └─ Slide animation
```

**RenderObject:** `RenderCustomPaint` (для track/thumb) + `RenderInkFeatures`

**Параметры:**
- `value` - bool
- `onChanged` - void Function(bool)
- `activeColor` - Color (thumb color when on)
- `activeTrackColor` - Color (track color when on)
- `inactiveThumbColor` - Color
- `inactiveTrackColor` - Color
- `activeThumbImage` - ImageProvider
- `inactiveThumbImage` - ImageProvider
- `thumbColor` - MaterialStateProperty<Color?>
- `trackColor` - MaterialStateProperty<Color?>
- `trackOutlineColor` - MaterialStateProperty<Color?>
- `thumbIcon` - MaterialStateProperty<Icon?>
- `materialTapTargetSize` - MaterialTapTargetSize
- `dragStartBehavior` - DragStartBehavior
- `focusColor`, `hoverColor` - Color
- `overlayColor` - MaterialStateProperty<Color?>
- `splashRadius` - double
- `focusNode` - FocusNode
- `autofocus` - bool

---

## SwitchListTile
```
📦 SwitchListTile (ListTile + Switch)
  └─ MergeSemantics
      └─ ListTile
          └─ Switch
```

**RenderObject:** Комбинация RenderObject из ListTile и Switch

**Параметры:**
- `value`, `onChanged` - как у Switch
- `title`, `subtitle`, `secondary`, `isThreeLine`, `dense`, `selected` - как у CheckboxListTile
- `controlAffinity`, `activeColor`, `activeTrackColor`, `inactiveThumbColor`, `inactiveTrackColor`
- `tileColor`, `selectedTileColor`, `contentPadding`, `enabled`

---

## Slider
```
📦 Slider
  └─ Material (track + thumb + overlay) -> RenderInkFeatures
      └─ CustomPaint -> RenderCustomPaint
          └─ Gesture detection
```

**RenderObject:** `RenderCustomPaint` (для track/thumb) + gesture handling

**Параметры:**
- `value` - double (current value)
- `onChanged` - void Function(double)
- `onChangeStart` - void Function(double)
- `onChangeEnd` - void Function(double)
- `min` - double (default 0.0)
- `max` - double (default 1.0)
- `divisions` - int? (discrete steps)
- `label` - String (показывается над thumb)
- `activeColor` - Color
- `inactiveColor` - Color
- `thumbColor` - Color
- `overlayColor` - MaterialStateProperty<Color?>
- `mouseCursor` - MouseCursor
- `semanticFormatterCallback` - String Function(double)
- `focusNode` - FocusNode
- `autofocus` - bool

**Варианты:**
- `Slider()` - обычный
- `Slider.adaptive()` - платформо-специфичный

---

## RangeSlider
```
📦 RangeSlider (Two-thumb slider)
  └─ Material (track + 2 thumbs + overlays) -> RenderInkFeatures
      └─ CustomPaint -> RenderCustomPaint
          └─ Gesture detection для обоих thumbs
```

**RenderObject:** `RenderCustomPaint` (для track/thumbs) + gesture handling

**Параметры:**
- `values` - RangeValues (start, end)
- `onChanged` - void Function(RangeValues)
- `onChangeStart`, `onChangeEnd` - void Function(RangeValues)
- `min`, `max` - double
- `divisions` - int
- `labels` - RangeLabels (start label, end label)
- `activeColor`, `inactiveColor` - Color
- Остальные как у Slider

---

## DropdownButton
```
📦 DropdownButton<T>
  └─ InkWell (trigger) -> RenderInkFeatures
      └─ Row -> RenderFlex
          ├─ Selected item
          └─ Down arrow icon
      └─ Overlay (popup menu)
          └─ DropdownMenuItem items
```

**RenderObject:** `RenderFlex` + overlay для меню

**Параметры:**
- `items` - List<DropdownMenuItem<T>>
- `value` - T? (selected value)
- `onChanged` - void Function(T?)
- `onTap` - VoidCallback
- `selectedItemBuilder` - List<Widget> Function(BuildContext)
- `hint` - Widget (показывается если value == null)
- `disabledHint` - Widget
- `elevation` - int
- `style` - TextStyle
- `icon` - Widget (down arrow)
- `iconDisabledColor`, `iconEnabledColor` - Color
- `iconSize` - double
- `isDense` - bool
- `isExpanded` - bool (заполнить ширину)
- `itemHeight` - double
- `focusColor` - Color
- `focusNode` - FocusNode
- `autofocus` - bool
- `dropdownColor` - Color
- `menuMaxHeight` - double
- `enableFeedback` - bool
- `alignment` - AlignmentGeometry
- `borderRadius` - BorderRadius
- `padding` - EdgeInsets

---

## DropdownMenuItem
```
📦 DropdownMenuItem<T>
  └─ Container -> RenderPadding + RenderDecoratedBox
      └─ InkWell -> RenderInkFeatures
          └─ Child Widget
```

**RenderObject:** `RenderPadding` + `RenderInkFeatures`

**Параметры:**
- `value` - T
- `onTap` - VoidCallback
- `enabled` - bool
- `alignment` - AlignmentGeometry
- `child` - Widget

---

## DropdownButtonFormField
```
📦 DropdownButtonFormField<T>
  └─ FormField<T>
      └─ InputDecorator
          └─ DropdownButton<T>
```

**RenderObject:** Комбинация из FormField и DropdownButton

**Параметры:** Те же что у DropdownButton + дополнительные:
- `decoration` - InputDecoration
- `validator` - String? Function(T?)
- `onSaved` - void Function(T?)
- `autovalidateMode` - AutovalidateMode

---

# 🔘 Button Widgets (Кнопки)

## TextButton
```
📦 TextButton (Material Design text button)
  └─ Material -> RenderPhysicalModel
      └─ InkWell (ripple) -> RenderInkFeatures
          └─ Padding -> RenderPadding
              └─ Row -> RenderFlex
                  ├─ Icon (optional)
                  └─ Text
```

**RenderObject:** `RenderPhysicalModel` + `RenderInkFeatures` + `RenderFlex`

**Параметры:**
- `onPressed` - VoidCallback? (null = disabled)
- `onLongPress` - VoidCallback?
- `onHover` - void Function(bool)
- `onFocusChange` - void Function(bool)
- `style` - ButtonStyle
- `focusNode` - FocusNode
- `autofocus` - bool
- `clipBehavior` - Clip
- `child` - Widget

**Варианты:**
- `TextButton()` - стандартный
- `TextButton.icon()` - с иконкой

---

## ElevatedButton
```
📦 ElevatedButton (Material Design elevated button)
  └─ Material (elevation, shadow) -> RenderPhysicalModel
      └─ InkWell (ripple) -> RenderInkFeatures
          └─ Padding -> RenderPadding
              └─ Row -> RenderFlex
                  ├─ Icon (optional)
                  └─ Text
```

**RenderObject:** `RenderPhysicalModel` (с elevation) + `RenderInkFeatures` + `RenderFlex`

**Параметры:** Те же что у TextButton

**Варианты:**
- `ElevatedButton()`
- `ElevatedButton.icon()`

---

## OutlinedButton
```
📦 OutlinedButton (Material Design outlined button)
  └─ Material (border) -> RenderPhysicalModel
      └─ InkWell (ripple) -> RenderInkFeatures
          └─ Padding -> RenderPadding
              └─ Row -> RenderFlex
                  ├─ Icon (optional)
                  └─ Text
```

**RenderObject:** `RenderPhysicalModel` (с border) + `RenderInkFeatures` + `RenderFlex`

**Параметры:** Те же что у TextButton

**Варианты:**
- `OutlinedButton()`
- `OutlinedButton.icon()`

---

## IconButton
```
📦 IconButton (Icon button)
  └─ Material -> RenderPhysicalModel
      └─ InkWell (ripple) -> RenderInkFeatures
          └─ Padding -> RenderPadding
              └─ Icon
```

**RenderObject:** `RenderPhysicalModel` + `RenderInkFeatures` + `RenderPadding`

**Параметры:**
- `onPressed` - VoidCallback?
- `icon` - Widget
- `iconSize` - double
- `visualDensity` - VisualDensity
- `padding` - EdgeInsets
- `alignment` - AlignmentGeometry
- `splashRadius` - double
- `color` - Color
- `focusColor`, `hoverColor`, `highlightColor`, `splashColor`, `disabledColor` - Color
- `mouseCursor` - MouseCursor
- `focusNode` - FocusNode
- `autofocus` - bool
- `tooltip` - String
- `enableFeedback` - bool
- `constraints` - BoxConstraints
- `style` - ButtonStyle
- `isSelected` - bool
- `selectedIcon` - Widget

---

## FloatingActionButton
```
📦 FloatingActionButton (FAB)
  └─ Material (circular elevation) -> RenderPhysicalShape
      └─ InkWell (ripple) -> RenderInkFeatures
          └─ Padding -> RenderPadding
              └─ Icon или Text
```

**RenderObject:** `RenderPhysicalShape` (circular) + `RenderInkFeatures`

**Параметры:**
- `onPressed` - VoidCallback?
- `tooltip` - String
- `foregroundColor` - Color (icon/text color)
- `backgroundColor` - Color
- `focusColor`, `hoverColor`, `splashColor` - Color
- `elevation` - double
- `focusElevation`, `hoverElevation`, `highlightElevation`, `disabledElevation` - double
- `shape` - ShapeBorder
- `clipBehavior` - Clip
- `focusNode` - FocusNode
- `autofocus` - bool
- `materialTapTargetSize` - MaterialTapTargetSize
- `mini` - bool (small FAB)
- `mouseCursor` - MouseCursor
- `child` - Widget
- `heroTag` - Object (для Hero transition)

**Варианты:**
- `FloatingActionButton()` - обычный
- `FloatingActionButton.extended()` - с текстом
- `FloatingActionButton.small()` - маленький
- `FloatingActionButton.large()` - большой

---

## CupertinoButton
```
📦 CupertinoButton (iOS-style button)
  └─ GestureDetector -> RenderPointerListener
      └─ Opacity (при нажатии) -> RenderOpacity
          └─ DecoratedBox (опционально) -> RenderDecoratedBox
              └─ Padding -> RenderPadding
                  └─ Child Widget
```

**RenderObject:** `RenderPointerListener` + `RenderOpacity` + `RenderPadding`

**Параметры:**
- `onPressed` - VoidCallback?
- `child` - Widget
- `padding` - EdgeInsets
- `color` - Color (background)
- `disabledColor` - Color
- `minSize` - double
- `pressedOpacity` - double
- `borderRadius` - BorderRadius
- `alignment` - AlignmentGeometry

**Варианты:**
- `CupertinoButton()`
- `CupertinoButton.filled()` - с фоном
