Проверь крейт flui-animation на соответствие docs/guide/rust-api-guidelines.md и rust_advanced_types.md.

Проходи по категориям ПОСЛЕДОВАТЕЛЬНО, одна за другой:

1. **Naming** (C-CASE, C-CONV, C-GETTER, C-ITER, C-ITER-TY, C-FEATURE, C-WORD-ORDER)

2. **Interoperability** (C-COMMON-TRAITS, C-CONV-TRAITS, C-COLLECT, C-SERDE, C-SEND-SYNC, C-GOOD-ERR)

3. **Documentation** (C-CRATE-DOC, C-EXAMPLE, C-QUESTION-MARK, C-FAILURE, C-LINK, C-METADATA)

4. **Predictability** (C-SMART-PTR, C-CONV-SPECIFIC, C-METHOD, C-NO-OUT, C-OVERLOAD, C-DEREF, C-CTOR)

5. **Flexibility** (C-INTERMEDIATE, C-CALLER-CONTROL, C-GENERIC, C-OBJECT)

6. **Type Safety** (C-NEWTYPE, C-CUSTOM-TYPE, C-BITFLAG, C-BUILDER)

7. **Dependability** (C-VALIDATE, C-DTOR-FAIL, C-DTOR-BLOCK)

8. **Debuggability** (C-DEBUG, C-DEBUG-NONEMPTY)

9. **Future Proofing** (C-SEALED, C-STRUCT-PRIVATE, C-NEWTYPE-HIDE, C-STRUCT-BOUNDS, C-NON-EXHAUSTIVE)

Для каждой категории выведи:

- ✅ Соблюдено: список правил

- ❌ Нарушено: правило → файл:строка → пример исправления

После всех категорий:

10. **PATTERNS.md рефакторинг** - найди возможности применить паттерны из PATTERNS.md:

    - Extension Traits

    - Builder Pattern

    - Generic Pattern

    - Newtype Pattern

    - Type State Pattern

Для каждой возможности: текущий код → предлагаемый код.
