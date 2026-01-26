# Git Worktree Workflow для Speckit

## Что такое Git Worktree?

Git worktree позволяет иметь несколько рабочих копий одного репозитория одновременно. Каждая фича получает свою изолированную директорию, что даёт:

- ✅ **Изоляция**: Каждая фича в отдельной папке
- ✅ **Безопасность**: Невозможно потерять uncommitted работу при переключении веток
- ✅ **Параллельная работа**: Работайте над несколькими фичами одновременно
- ✅ **Чистота**: Не нужно переключаться между ветками в main worktree

## Структура Директорий

```
flui/                          # Основной репозиторий (main worktree)
│
├── .git/                      # Git metadata
├── specs/                     # Specs для main branch
├── src/                       # Source code на main branch
└── ...

../.worktrees/                 # Worktrees для фич (sibling к repo)
│
├── 001-flui-scheduler/        # Worktree для фичи #001
│   ├── .git                   # Git link (не копия!)
│   ├── specs/
│   │   └── 001-flui-scheduler/
│   │       ├── spec.md
│   │       ├── plan.md
│   │       └── tasks.md
│   ├── src/
│   └── ...
│
├── 002-user-auth/             # Worktree для фичи #002
│   └── ...
│
└── 003-api-refactor/          # Worktree для фичи #003
    └── ...
```

## Использование

### 1. Создание Новой Фичи с Worktree

**Автоматически (рекомендуется):**

```powershell
# Создаёт worktree в ../.worktrees/<branch-name>
pwsh .specify/scripts/powershell/create-new-feature.ps1 "Add user authentication"
```

**С кастомным расположением:**

```powershell
pwsh .specify/scripts/powershell/create-new-feature.ps1 "Add user authentication" -WorktreeDir "C:/dev/my-features/user-auth"
```

**Legacy mode (без worktree):**

```powershell
# Использует старое поведение git checkout -b
pwsh .specify/scripts/powershell/create-new-feature.ps1 "Add user authentication" -NoWorktree
```

### 2. Переключение на Worktree

После создания фичи просто перейдите в директорию worktree:

```powershell
# Output from create-new-feature.ps1 показывает путь:
cd C:/Users/vanya/RustroverProjects/.worktrees/001-user-auth

# Теперь вы работаете в изолированной копии на ветке 001-user-auth
git status  # показывает ветку 001-user-auth
```

### 3. Работа с Worktree

```powershell
# Обычная работа с git
git add .
git commit -m "feat: implement login flow"
git push origin 001-user-auth

# Main worktree остаётся нетронутым!
# Можете одновременно работать в обеих директориях
```

### 4. Список Всех Worktrees

```powershell
pwsh .specify/scripts/powershell/manage-worktrees.ps1 list

# Output:
# Git Worktrees:
# ================================================================================
# [MAIN] main (a7b9b51d)
#        C:/Users/vanya/RustroverProjects/flui
#
#        001-user-auth (e8c34a2f)
#        C:/Users/vanya/RustroverProjects/.worktrees/001-user-auth
#
#        002-api-refactor (9f5d7b1a)
#        C:/Users/vanya/RustroverProjects/.worktrees/002-api-refactor
```

### 5. Удаление Worktree

**Удалить конкретный worktree:**

```powershell
pwsh .specify/scripts/powershell/manage-worktrees.ps1 remove 001-user-auth

# С force (если есть uncommitted changes):
pwsh .specify/scripts/powershell/manage-worktrees.ps1 remove 001-user-auth -Force
```

**Почистить устаревшие references:**

```powershell
# Если вы удалили worktree вручную (rm -rf), нужно прочистить references
pwsh .specify/scripts/powershell/manage-worktrees.ps1 prune
```

**Удалить ВСЕ feature worktrees:**

```powershell
# Требует -Force для безопасности
pwsh .specify/scripts/powershell/manage-worktrees.ps1 clean -Force
```

## Частые Сценарии

### Сценарий 1: Начать новую фичу

```powershell
# 1. Создать worktree
pwsh .specify/scripts/powershell/create-new-feature.ps1 "Implement OAuth2"

# 2. Перейти в worktree (путь показан в output)
cd C:/Users/vanya/RustroverProjects/.worktrees/002-implement-oauth2

# 3. Работать как обычно
# Edit files, commit, push, etc.
```

### Сценарий 2: Переключиться между фичами

```powershell
# Worktree 1: Работа над user-auth
cd C:/Users/vanya/RustroverProjects/.worktrees/001-user-auth
# Edit, commit...

# Worktree 2: Переключиться на api-refactor (не теряя работу в user-auth)
cd C:/Users/vanya/RustroverProjects/.worktrees/002-api-refactor
# Edit, commit...

# Main worktree: Вернуться на main
cd C:/Users/vanya/RustroverProjects/flui
# Main branch всё ещё на месте, ничего не изменилось
```

### Сценарий 3: Закончить фичу

```powershell
# 1. Завершить работу, запушить PR
cd C:/Users/vanya/RustroverProjects/.worktrees/001-user-auth
git push origin 001-user-auth

# 2. После merge в main, удалить worktree
pwsh .specify/scripts/powershell/manage-worktrees.ps1 remove 001-user-auth

# Скрипт предложит удалить и ветку тоже
```

### Сценарий 4: Hotfix на main при работе над фичей

```powershell
# Вы в worktree фичи с uncommitted changes
cd C:/Users/vanya/RustroverProjects/.worktrees/001-user-auth
# (uncommitted work here)

# Нужно срочно сделать hotfix на main
cd C:/Users/vanya/RustroverProjects/flui  # main worktree
git checkout main  # Легко! Никаких stash, никаких conflicts
# Fix bug, commit, push

# Вернуться к фиче
cd C:/Users/vanya/RustroverProjects/.worktrees/001-user-auth
# Ваша uncommitted работа всё ещё здесь!
```

## Преимущества vs. Традиционный Checkout

### Традиционный подход (git checkout -b)

```powershell
# Main branch с uncommitted работой
cd C:/Users/vanya/RustroverProjects/flui
git status  # Uncommitted changes

# Нужно переключиться на другую ветку
git checkout feature-branch
# ❌ Error: You have uncommitted changes

# Приходится делать:
git stash
git checkout feature-branch
# Work...
git checkout main
git stash pop
# ⚠️ Возможны конфликты при pop!
```

### Worktree подход

```powershell
# Main worktree с uncommitted работой
cd C:/Users/vanya/RustroverProjects/flui
git status  # Uncommitted changes

# Нужно поработать над другой фичей
cd C:/Users/vanya/RustroverProjects/.worktrees/001-user-auth
# ✅ Просто переходим в другую директорию!
# ✅ Main worktree остаётся нетронутым
# ✅ Никаких stash, никаких конфликтов
```

## Git Commands в Worktree

Все обычные git команды работают в worktree:

```powershell
cd C:/Users/vanya/RustroverProjects/.worktrees/001-user-auth

git status              # Status текущего worktree
git add .
git commit -m "feat: ..."
git push origin 001-user-auth
git pull
git rebase main
git merge main
# etc.
```

**Важно:** Все worktrees используют один `.git` metadata, поэтому:
- Commits, branches, remotes общие для всех worktrees
- Можно делать rebase между worktrees
- Экономия места на диске (не дублируется .git/)

## Ограничения

1. **Нельзя checkout одну ветку в нескольких worktrees одновременно**
   - Git не позволит: `git checkout main` в worktree если main уже checked out в другом worktree
   - Решение: создавайте новые ветки для каждого worktree

2. **Worktrees занимают место на диске**
   - Каждый worktree это полная копия рабочих файлов (но не .git/)
   - Для большого репозитория может быть накладно иметь 10+ worktrees

3. **Нужно помнить о cleanup**
   - Удаляйте ненужные worktrees: `manage-worktrees.ps1 remove <branch>`
   - Периодически prune: `manage-worktrees.ps1 prune`

## Интеграция с IDE

### Visual Studio Code

Каждый worktree можно открыть как отдельный workspace:

```powershell
# Открыть worktree в VS Code
code C:/Users/vanya/RustroverProjects/.worktrees/001-user-auth

# Или добавить в multi-root workspace
# File -> Add Folder to Workspace...
```

### JetBrains IDEs (RustRover, IntelliJ)

```powershell
# Открыть worktree как отдельный проект
# File -> Open -> выбрать worktree директорию

# Или добавить как module в существующий проект
```

## Troubleshooting

### Проблема: "worktree already exists"

```powershell
# Если worktree directory существует, но git не знает о нём:
pwsh .specify/scripts/powershell/manage-worktrees.ps1 prune

# Или удалить директорию вручную:
rm -rf C:/Users/vanya/RustroverProjects/.worktrees/001-user-auth
pwsh .specify/scripts/powershell/manage-worktrees.ps1 prune
```

### Проблема: "branch is already checked out"

```powershell
# Нельзя checkout одну ветку в двух worktrees
# Решение: создайте новую ветку или удалите старый worktree

# Посмотреть, где ветка checked out:
pwsh .specify/scripts/powershell/manage-worktrees.ps1 list

# Удалить worktree:
pwsh .specify/scripts/powershell/manage-worktrees.ps1 remove <branch>
```

### Проблема: "uncommitted changes" при удалении

```powershell
# Если есть uncommitted changes, git не даст удалить worktree
# Опции:

# 1. Commit changes
cd C:/Users/vanya/RustroverProjects/.worktrees/001-user-auth
git add .
git commit -m "WIP: save work"

# 2. Force удаление (потеряете changes!)
pwsh .specify/scripts/powershell/manage-worktrees.ps1 remove 001-user-auth -Force
```

## Best Practices

1. **Используйте worktrees для feature branches**
   - Main worktree держите на stable branch (main/master)
   - Все фичи делайте в отдельных worktrees

2. **Cleanup после merge**
   - После merge PR удаляйте worktree: `manage-worktrees.ps1 remove <branch>`
   - Периодически: `manage-worktrees.ps1 prune`

3. **Именование веток**
   - Используйте numbered branches: `001-feature`, `002-feature`
   - Helps track feature lifecycle

4. **Backup uncommitted work**
   - Worktrees защищают от потери work при checkout
   - Но всё равно делайте commits часто!

5. **IDE per worktree**
   - Открывайте каждый worktree как отдельный project/workspace
   - Helps with context switching

## См. также

- [Git Worktree Documentation](https://git-scm.com/docs/git-worktree)
- [Speckit Documentation](./README.md)
- [create-new-feature.ps1](../scripts/powershell/create-new-feature.ps1)
- [manage-worktrees.ps1](../scripts/powershell/manage-worktrees.ps1)
