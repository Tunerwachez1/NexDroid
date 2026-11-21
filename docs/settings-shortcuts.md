# Сочетания клавиш / Shortcuts

Шестерёнка внутри Android-окна → **Сочетания клавиш**. Нажмите кнопку с текущим назначением, затем нужную комбинацию. Переключатель справа отключает сочетание и сохраняет его для повторного включения. Нажмите «Сохранить изменения»; Android перезапускать не нужно.

| Действие / Action | По умолчанию / Default |
| --- | --- |
| Полноэкранный режим / Fullscreen | F11 |
| Настройки / Settings | Ctrl + Shift + I |
| Редактор управления / Controls editor | Ctrl + Shift + A |
| Ввод текста / Text input | F8 |
| Снимок экрана / Screenshot | Ctrl + Shift + S |
| Захват мыши / Mouse capture | Из профиля текущей игры / From current game profile |

Ctrl, Shift, Alt и Super можно сочетать с основной клавишей. Запись внутри Android-окна использует физические evdev-коды: раскладка не меняет назначение, NumPad различается с основным цифровым рядом. При отсутствии native keyboard connection используется GPUI fallback. Esc отменяет запись; обычное поведение Esc отдельно задаётся в «Предпочтениях».

Совпадения включённых назначений блокируют сохранение. Встроенная форма учитывает также наследуемую клавишу захвата текущего игрового профиля. Если после настройки переключиться на другую игру с иным capture key, сочетания окна имеют приоритет; назначьте другую комбинацию или отключите конфликтующее действие. Сам захват требует элемента обзора/прицела в игровом профиле.

Комбинации работают только в активном Android-окне. Linux и оконный менеджер могут перехватывать системные сочетания до доставки приложению. Отключение сочетания не отключает экранную кнопку: настройки и выход из fullscreen остаются доступны мышью. Отпущенное сочетание передаётся обычной обработке Android/игрового профиля. Переназначенные или отключённые F8/F11 больше не зарезервированы статически редактором.

Файл: `$XDG_CONFIG_HOME/liwinux/shortcuts.json`, обычно `~/.config/liwinux/shortcuts.json`. Он сохраняется атомарно отдельно от performance/display JSON и не требует root. Изменения перечитываются работающим окном раз в секунду и сразу при закрытии формы. Кнопка «Перечитать JSON» обновляет черновик формы с диска; «Восстановить по умолчанию» требует последующего сохранения.

Пример одного переопределённого действия (недостающие действия загружаются со стандартными настройками):

```json
{
  "version": 1,
  "bindings": {
    "fullscreen": {
      "enabled": false,
      "chord": { "key": 87, "modifiers": 0 }
    }
  }
}
```

`key` — физический evdev-код. `modifiers` — сумма Ctrl=1, Shift=2, Alt=4, Super=8. Имена действий: `fullscreen`, `settings`, `editor`, `text_input`, `screenshot`, `capture_mouse`. Для `capture_mouse` значение `chord: null` означает наследование из игрового профиля. Для остальных включённых действий комбинация обязательна. Неизвестная версия, неверные клавиши, конфликты и повреждённый JSON не заменяют последний рабочий runtime config. Форма показывает ошибку и требует исправления файла/перечитывания либо явного восстановления стандартных назначений перед сохранением.

## English

Open the gear → **Shortcuts**. Click a binding and press the replacement combination; use its switch to disable or re-enable it. Save to apply without restarting Android. Enabled duplicate combinations are rejected. Mouse capture inherits the game profile until overridden. Esc cancels recording; Esc window behavior lives in Preferences. Shortcuts apply only while the Android window has focus; Linux may reserve system combinations. The toolbar remains usable when its keyboard shortcut is disabled.
