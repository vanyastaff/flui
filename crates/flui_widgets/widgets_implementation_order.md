# 📋 Список виджетов Flutter по порядку реализации

> Упорядоченный список для поэтапной реализации виджетов

---

## Phase 1: Leaf Widgets (Примитивы без детей)

### Приоритет: CRITICAL
**Цель:** Базовые строительные блоки

1. **ColoredBox** - простейший виджет (цветной прямоугольник)
2. **SizedBox** - фиксированный размер или spacer
3. **Placeholder** - временная заглушка
4. **Text** - текстовый виджет ⚠️ СЛОЖНЫЙ (text shaping)
5. **Icon** - иконка из IconFont
6. **Image** - изображение ⚠️ (image decoding)
7. **RawImage** - уже декодированное изображение

---

## Phase 2: Single-Child Layout (Контейнеры с 1 ребенком)

### Приоритет: CRITICAL
**Цель:** Базовые layout примитивы

8. **Padding** - отступы вокруг ребенка
9. **Center** - центрирование ребенка
10. **Align** - позиционирование с alignment
11. **SizedBox** (с child) - ограничение размера
12. **ConstrainedBox** - ограничения min/max
13. **UnconstrainedBox** - снятие ограничений
14. **LimitedBox** - ограничения для unbounded
15. **AspectRatio** - поддержание соотношения сторон
16. **FittedBox** - масштабирование под размер
17. **FractionallySizedBox** - процент от родителя
18. **Baseline** - выравнивание по baseline
19. **OverflowBox** - выход за границы
20. **SizedOverflowBox** - фиксированный размер + overflow
21. **Transform** - трансформации (rotate, scale, translate)
22. **RotatedBox** - поворот на 90° increments
23. **Offstage** - рендерить но не показывать
24. **Visibility** - условная видимость

---

## Phase 3: Visual Effects (Визуальные эффекты)

### Приоритет: HIGH
**Цель:** Декорирование и эффекты

25. **DecoratedBox** - фон, границы, тени
26. **Opacity** - прозрачность
27. **ClipRect** - прямоугольная обрезка
28. **ClipRRect** - обрезка со скругленными углами
29. **ClipOval** - овальная обрезка
30. **ClipPath** - обрезка по произвольному пути
31. **BackdropFilter** - blur эффект
32. **ShaderMask** - gradient маска
33. **ColorFiltered** - цветовой фильтр
34. **RepaintBoundary** - изоляция repaint

---

## Phase 4: Multi-Child Layout (Flex & Stack)

### Приоритет: CRITICAL
**Цель:** Самые используемые layouts

35. **Row** - горизонтальная раскладка ⚠️ СЛОЖНЫЙ
36. **Column** - вертикальная раскладка ⚠️ СЛОЖНЫЙ
37. **Flex** - базовый flex контейнер
38. **Flexible** - гибкий ребенок в Flex
39. **Expanded** - расширяющийся ребенок
40. **Spacer** - пустое пространство с flex
41. **Stack** - наложение слоями
42. **Positioned** - абсолютное позиционирование в Stack
43. **PositionedDirectional** - positioned с учетом direction
44. **IndexedStack** - Stack с одним видимым ребенком

---

## Phase 5: Multi-Child Layout (Advanced)

### Приоритет: MEDIUM
**Цель:** Продвинутые layouts

45. **Wrap** - flow-like layout с переносом
46. **Flow** - custom positioned children
47. **ListBody** - простой vertical/horizontal list
48. **Table** - табличная раскладка
49. **TableRow** - строка таблицы
50. **TableCell** - ячейка с настройками
51. **CustomMultiChildLayout** - custom layout logic
52. **LayoutId** - метка для CustomMultiChildLayout

---

## Phase 6: Composite Widgets (Stateless)

### Приоритет: HIGH
**Цель:** Высокоуровневые виджеты из примитивов

53. **Container** - универсальный контейнер ⚠️ ВАЖНЫЙ
54. **Card** - Material карточка

---

## Phase 7: Interaction Widgets

### Приоритет: HIGH
**Цель:** Интерактивность

55. **GestureDetector** - распознавание жестов ⚠️ СЛОЖНЫЙ
56. **InkWell** - Material ripple эффект
57. **InkResponse** - кастомизируемый InkWell
58. **Listener** - raw pointer events
59. **MouseRegion** - mouse события
60. **AbsorbPointer** - блокировка событий
61. **IgnorePointer** - игнорирование событий
62. **Draggable** - перетаскиваемый виджет
63. **LongPressDraggable** - drag после long press
64. **DragTarget** - зона для drop
65. **Dismissible** - swipe to dismiss
66. **InteractiveViewer** - pan & zoom
67. **Scrollbar** - визуальный scrollbar

---

## Phase 8: Scrolling Widgets

### Приоритет: HIGH
**Цель:** Scrollable контент

68. **SingleChildScrollView** - простая прокрутка
69. **ListView** - список элементов
70. **ListView.builder** - ленивый список
71. **ListView.separated** - список с разделителями
72. **ListView.custom** - custom delegate
73. **GridView** - сетка элементов
74. **GridView.count** - фиксированное число колонок
75. **GridView.extent** - фиксированный размер ячейки
76. **GridView.builder** - ленивая сетка
77. **CustomScrollView** - sliver-based scroll
78. **PageView** - paginated scroll
79. **PageView.builder** - ленивый PageView
80. **ListWheelScrollView** - 3D wheel эффект
81. **NestedScrollView** - вложенная прокрутка

---

## Phase 9: Text Widgets

### Приоритет: MEDIUM
**Цель:** Продвинутая работа с текстом

82. **RichText** - multi-style text
83. **TextSpan** - styled text fragment
84. **WidgetSpan** - widget внутри text
85. **SelectableText** - выделяемый текст
86. **DefaultTextStyle** - inherited text style

---

## Phase 10: Animation Widgets (Implicit)

### Приоритет: MEDIUM
**Цель:** Неявные анимации

87. **AnimatedContainer** - анимированный Container
88. **AnimatedPadding** - анимированный Padding
89. **AnimatedAlign** - анимированный Align
90. **AnimatedPositioned** - анимированный Positioned
91. **AnimatedOpacity** - анимированная прозрачность
92. **AnimatedRotation** - анимированный поворот
93. **AnimatedScale** - анимированный масштаб
94. **AnimatedSlide** - анимированное смещение
95. **AnimatedDefaultTextStyle** - анимированный стиль текста
96. **AnimatedPhysicalModel** - анимированная физическая модель

---

## Phase 11: Animation Widgets (Explicit)

### Приоритет: MEDIUM
**Цель:** Явные анимации

97. **AnimatedSwitcher** - cross-fade между детьми
98. **AnimatedCrossFade** - fade между двумя детьми
99. **Hero** - shared element transition
100. **AnimatedBuilder** - explicit animation builder
101. **TweenAnimationBuilder** - tween-based animation

---

## Phase 12: Material Design - Structure

### Приоритет: HIGH
**Цель:** Структура Material приложения

102. **MaterialApp** - Material app root
103. **Scaffold** - Material page structure ⚠️ ВАЖНЫЙ
104. **AppBar** - Material app bar
105. **BottomNavigationBar** - нижняя навигация
106. **Drawer** - боковая панель
107. **FloatingActionButton** - FAB кнопка

---

## Phase 13: Material Design - Buttons

### Приоритет: HIGH
**Цель:** Material кнопки

108. **TextButton** - текстовая кнопка
109. **ElevatedButton** - приподнятая кнопка
110. **OutlinedButton** - кнопка с обводкой
111. **IconButton** - кнопка с иконкой
112. **FloatingActionButton** (variants) - FAB варианты

---

## Phase 14: Material Design - Dialogs & Overlays

### Приоритет: MEDIUM
**Цель:** Диалоги и оверлеи

113. **Dialog** - базовый диалог
114. **AlertDialog** - Material alert dialog
115. **SimpleDialog** - простой диалог
116. **SnackBar** - временное сообщение
117. **MaterialBanner** - persistent banner
118. **BottomSheet** - нижняя панель
119. **showModalBottomSheet** - модальный bottom sheet

---

## Phase 15: Material Design - Lists & Cards

### Приоритет: MEDIUM
**Цель:** Списки и карточки

120. **ListTile** - Material list item ⚠️ ВАЖНЫЙ
121. **CheckboxListTile** - ListTile + Checkbox
122. **RadioListTile** - ListTile + Radio
123. **SwitchListTile** - ListTile + Switch
124. **ExpansionTile** - раскрывающийся ListTile
125. **Card** - Material карточка (если еще не реализован)

---

## Phase 16: Input Widgets - Basic

### Приоритет: HIGH
**Цель:** Базовый ввод

126. **TextField** - текстовое поле ⚠️ ОЧЕНЬ СЛОЖНЫЙ
127. **TextFormField** - TextField с валидацией
128. **Checkbox** - галочка
129. **Radio** - радиокнопка
130. **Switch** - переключатель
131. **Slider** - ползунок
132. **RangeSlider** - двойной ползунок

---

## Phase 17: Input Widgets - Advanced

### Приоритет: MEDIUM
**Цель:** Продвинутый ввод

133. **DropdownButton** - выпадающий список
134. **DropdownMenuItem** - элемент dropdown
135. **DropdownButtonFormField** - dropdown с валидацией
136. **Autocomplete** - автодополнение
137. **SearchBar** - поисковая строка
138. **DatePicker** - выбор даты
139. **TimePicker** - выбор времени

---

## Phase 18: Material Design - Advanced

### Приоритет: LOW
**Цель:** Продвинутые Material компоненты

140. **Chip** - Material chip
141. **InputChip** - chip для ввода
142. **ChoiceChip** - chip для выбора
143. **FilterChip** - chip для фильтра
144. **ActionChip** - chip для действия
145. **Badge** - значок уведомления
146. **Tooltip** - всплывающая подсказка
147. **TabBar** - вкладки
148. **TabBarView** - содержимое вкладок
149. **Stepper** - пошаговый виджет
150. **DataTable** - таблица данных
151. **CircularProgressIndicator** - круговой индикатор
152. **LinearProgressIndicator** - линейный индикатор
153. **RefreshIndicator** - pull-to-refresh

---

## Phase 19: Navigation & Routing

### Приоритет: HIGH
**Цель:** Multi-page приложения

154. **Navigator** - навигационный стек ⚠️ СЛОЖНЫЙ
155. **MaterialPageRoute** - Material transition
156. **CupertinoPageRoute** - iOS transition
157. **PageRouteBuilder** - custom transition
158. **Hero** (если еще не реализован) - shared element transition

---

## Phase 20: Form & Validation

### Приоритет: MEDIUM
**Цель:** Работа с формами

159. **Form** - контейнер формы
160. **FormField** - базовое поле формы
161. **TextFormField** (если еще не реализован)
162. **DropdownButtonFormField** (если еще не реализован)

---

## Phase 21: Utility Widgets

### Приоритет: MEDIUM
**Цель:** Утилитные виджеты

163. **Builder** - новый BuildContext
164. **StatefulBuilder** - локальный state
165. **LayoutBuilder** - адаптивная верстка
166. **OrientationBuilder** - orientation-aware
167. **MediaQuery** - информация об экране
168. **SafeArea** - избежание system UI
169. **Theme** - inherited theme
170. **InheritedWidget** - data propagation
171. **ValueListenableBuilder** - reactive на ValueNotifier
172. **StreamBuilder** - reactive на Stream
173. **FutureBuilder** - loading states для Future

---

## Phase 22: Advanced Rendering

### Приоритет: LOW
**Цель:** Custom рендеринг

174. **CustomPaint** - custom painting
175. **CustomSingleChildLayout** - custom single-child layout
176. **CustomMultiChildLayout** (если еще не реализован)

---

## Phase 23: Platform-Specific

### Приоритет: LOW
**Цель:** Платформо-специфичные виджеты

177. **CupertinoApp** - iOS app root
178. **CupertinoButton** - iOS кнопка
179. **CupertinoNavigationBar** - iOS nav bar
180. **CupertinoTabBar** - iOS tab bar
181. **CupertinoSwitch** - iOS переключатель
182. **CupertinoSlider** - iOS ползунок

---

## Phase 24: Accessibility & Semantics

### Приоритет: LOW
**Цель:** Доступность

183. **Semantics** - accessibility info
184. **ExcludeSemantics** - скрыть от accessibility
185. **MergeSemantics** - объединить semantics
186. **BlockSemantics** - блокировать semantics

---

## Phase 25: Focus & Keyboard

### Приоритет: LOW
**Цель:** Управление фокусом

187. **Focus** - focus management
188. **FocusScope** - focus subtree
189. **FocusTraversalGroup** - tab order
190. **Actions** - keyboard shortcuts
191. **Shortcuts** - shortcut bindings

---

## 📊 Сводка по приоритетам

### CRITICAL (Блокируют всё) - 26 виджетов
Phases 1, 2, 4, 6: ColoredBox → Text → Padding → Row/Column → Container

### HIGH (Нужны для MVP) - 45 виджетов
Phases 3, 7, 8, 12, 13, 16, 19: Visual effects, Interaction, Scrolling, Material basics, Buttons, Input, Navigation

### MEDIUM (Нужны для полноценного приложения) - 60 виджетов
Phases 5, 9, 10, 11, 14, 15, 17, 20, 21: Advanced layouts, Text, Animations, Dialogs, Forms

### LOW (Nice to have) - 44 виджета
Phases 22, 23, 24, 25: Custom rendering, Platform-specific, Accessibility, Focus

---

## 🎯 Рекомендуемый минимальный набор для MVP (30 виджетов):

1. ColoredBox
2. SizedBox
3. Text
4. Padding
5. Center
6. Align
7. Container
8. Row
9. Column
10. Expanded
11. Stack
12. Positioned
13. DecoratedBox
14. Opacity
15. ClipRRect
16. GestureDetector
17. InkWell
18. SingleChildScrollView
19. ListView
20. Scaffold
21. AppBar
22. TextButton
23. ElevatedButton
24. IconButton
25. FloatingActionButton
26. TextField
27. Checkbox
28. ListTile
29. Card
30. Navigator

**С этими 30 виджетами можно создать полноценное приложение!** 🎉
