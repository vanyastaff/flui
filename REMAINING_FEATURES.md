# Remaining Features in flui_core_old

**Total**: ~14,168 LOC осталось в старом коде
**Мигрировано**: ~5,135 LOC (Phase 1.1-1.8, 2.1)

---

## ✅ УЖЕ МИГРИРОВАНО (9 фаз)

| Phase | Feature | Status |
|-------|---------|--------|
| 1.1 | LayoutCache + Statistics | ✅ |
| 1.2 | DebugFlags | ✅ |
| 1.3 | Diagnostics | ✅ |
| 1.4 | DependencyTracker | ✅ |
| 1.5 | ChangeNotifier/ValueNotifier | ✅ |
| 1.6 | String Cache | ✅ Skipped |
| 1.7 | Slot System | ✅ |
| 1.8 | BuildOwner | ✅ |
| 2.1 | Notification System | ✅ |

---

## 🔍 ЧТО ОСТАЛОСЬ

### 📁 **Модули уже частично в новом коде:**

✅ **foundation/** - Большая часть мигрирована
- ✅ key.rs (уже есть в новом коде)
- ✅ diagnostics.rs (мигрировано Phase 1.3)
- ✅ change_notifier.rs (мигрировано Phase 1.5)
- ✅ slot.rs (мигрировано Phase 1.7)

✅ **debug/** - Мигрировано (Phase 1.2)
- ✅ mod.rs (387 LOC)

✅ **cache/** - Мигрировано (Phase 1.1)
- ✅ layout_cache.rs

✅ **notification/** - Мигрировано (Phase 2.1)
- ✅ mod.rs, listener.rs

✅ **element/** - Базовая структура есть в новом коде
- Новая архитектура использует enum Element вместо старой

✅ **widget/** - Базовая структура есть в новом коде
- Stateless, Stateful, Inherited, RenderObject, ParentData все есть

✅ **render/** - Базовая структура есть в новом коде
- RenderObject trait, arity system, paint/layout уже есть

✅ **tree/** - Частично мигрировано
- ✅ build_owner.rs (мигрировано Phase 1.8)
- ✅ element_tree.rs (уже в новом коде, другая архитектура)
- ⏳ **pipeline.rs** (408 LOC) - **НУЖНО МИГРИРОВАТЬ**

---

## ⚠️ ТРЕБУЮТ МИГРАЦИИ

### 1️⃣ **tree/pipeline.rs** (408 LOC) ⭐ **ВАЖНО**
**Описание**: PipelineOwner - координатор build → layout → paint pipeline
**Функции**:
- `flush_build()` - rebuild dirty widgets
- `flush_layout()` - layout dirty RenderObjects
- `flush_paint()` - paint dirty RenderObjects
- Dirty tracking для incremental rendering
- Hit testing coordination

**Статус**: 🔴 Критически важно для рендеринга
**Приоритет**: HIGH

---

### 2️⃣ **context/** (1,756 LOC total) ⭐ **ВАЖНО**
**Файлы**:
- `impl_.rs` (573 LOC) - BuildContext implementation
- `dependency.rs` (512 LOC) - Dependency tracking (частично мигрировано в Phase 1.4)
- `inherited.rs` (399 LOC) - InheritedWidget context methods
- `iterators.rs` (230 LOC) - Tree traversal iterators
- `mod.rs` (42 LOC)

**Описание**: BuildContext API - главный интерфейс для виджетов
**Функции**:
- `dependOnInheritedWidgetOfExactType<T>()`
- `findAncestorWidgetOfExactType<T>()`
- `findRenderObject()`
- `visitAncestorElements()`
- `visitChildElements()`

**Статус**: 🟡 BuildContext частично есть, но методы нужно дополнить
**Приоритет**: HIGH

---

### 3️⃣ **typed/** (1,168 LOC total) 🤔 **EXPERIMENTAL**
**Файлы**:
- `context.rs` (804 LOC) - Typed context with arity
- `render_object.rs` (218 LOC) - Typed RenderObject trait
- `arity.rs` (112 LOC) - Arity types (Leaf, Single, Multi)
- `mod.rs` (34 LOC)

**Описание**: Экспериментальная типизированная версия RenderObject
**Особенности**:
- Compile-time arity checks (Leaf/Single/Multi children)
- Type-safe context (LayoutCx<A>, PaintCx<A>)
- Zero-cost abstractions

**Статус**: 🟣 Экспериментально, новый код уже использует похожий подход
**Приоритет**: MEDIUM (может быть скипнуто, если новый код лучше)

---

### 4️⃣ **testing/** (698 LOC total) 🧪
**Файлы**:
- `mod.rs` (481 LOC) - Testing utilities
- `render_testing.rs` (217 LOC) - RenderObject testing helpers

**Описание**: Testing infrastructure для unit tests
**Функции**:
- `MockRenderObject`
- Layout testing helpers
- Paint verification
- Tree validation

**Статус**: 🟢 Nice-to-have для тестов
**Приоритет**: MEDIUM-LOW

---

### 5️⃣ **Standalone files** (747 LOC total)

#### **error.rs** (352 LOC) ⚠️
**Описание**: Error types и Result aliases
**Типы**:
- `FluiError` enum
- `FluiResult<T>`
- Widget/Element/Render error variants

**Статус**: 🟡 Нужно для error handling
**Приоритет**: MEDIUM

#### **hot_reload.rs** (244 LOC) 🔥
**Описание**: Hot reload support для development
**Функции**:
- Widget state preservation
- Element tree diffing
- Incremental updates

**Статус**: 🟢 Nice-to-have для dev experience
**Приоритет**: LOW

#### **profiling.rs** (151 LOC) 📊
**Описание**: Performance profiling utilities
**Функции**:
- Frame timing
- Layout/paint metrics
- Memory usage tracking

**Статус**: 🟢 Nice-to-have для optimization
**Приоритет**: LOW

---

## 📊 ИТОГО: Приоритеты миграции

### 🔴 **CRITICAL** (нужно для базовой работы):
1. **tree/pipeline.rs** (408 LOC) - PipelineOwner для rendering loop
2. **context/impl_.rs** (573 LOC) - BuildContext API methods

### 🟡 **HIGH** (важно для полной функциональности):
3. **context/inherited.rs** (399 LOC) - InheritedWidget support
4. **context/iterators.rs** (230 LOC) - Tree traversal
5. **error.rs** (352 LOC) - Error handling

### 🟢 **MEDIUM** (nice-to-have):
6. **testing/** (698 LOC) - Testing infrastructure
7. **typed/** (1,168 LOC) - Typed RenderObject (если новый код не покрывает)

### 🔵 **LOW** (можно отложить):
8. **hot_reload.rs** (244 LOC) - Development convenience
9. **profiling.rs** (151 LOC) - Performance analysis

---

## 💡 РЕКОМЕНДАЦИИ

### Минимальный набор для функциональности:
1. ✅ Мигрировать **PipelineOwner** (tree/pipeline.rs)
2. ✅ Дополнить **BuildContext** методами из context/impl_.rs
3. ✅ Добавить **Error types** (error.rs)

После этого можно **удалить flui_core_old** ✅

### Дополнительно (по желанию):
- Testing utilities для unit tests
- Hot reload для dev experience
- Profiling для optimization

---

## 🎯 СЛЕДУЮЩИЙ ШАГ

**Рекомендация**: Мигрировать **Phase 3.1: PipelineOwner** (408 LOC)
- Критически важен для rendering loop
- Координирует build → layout → paint
- Управляет dirty tracking

**Альтернатива**: Мигрировать **Phase 3.2: BuildContext API** (573 LOC)
- Главный интерфейс для виджетов
- Методы dependOnInheritedWidget, findAncestor, etc.

Что выберешь?
