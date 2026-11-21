# Preferences / Предпочтения

The gear in the Android window opens the embedded settings panel. Preferences is a native Rust/GPUI page, available in Russian and English.

- Settings language: preview immediately; save to persist. Import a JSON language using the file picker. Missing strings fall back to English.
- Fullscreen on start: affects the next newly opened Android window; F11 remains available.
- Esc: independently enable releasing the captured mouse and exiting fullscreen. If both apply, the first Esc releases the mouse and the second exits fullscreen. Disabled host actions pass Esc to Android. The settings panel itself always closes with Esc.
- Screenshots: choose a directory, or use the default `$HOME/Pictures/NexDroid`. Ctrl+Shift+S saves the current Android picture as PNG, excluding the host toolbar/settings. The directory is created on first capture. Capture errors are displayed in the Android window.

Preferences are stored in the existing XDG config directory (`$XDG_CONFIG_HOME/liwinux/performance.json`, normally `~/.config/liwinux/performance.json`). Saving this page changes only `language`, `fullscreen_start`, `escape_fullscreen`, `escape_mouse`, and `screenshots`. It does not alter Android resolution, refresh rate, CPU or memory, and does not request root authorization. Existing profiles default to windowed startup and both Esc actions enabled.

Custom language example:

```json
{
  "id": "custom",
  "name": "My language",
  "strings": {
    "settings": "My settings",
    "preferences": "My preferences",
    "close": "Close"
  }
}
```

Complete translation templates are in `crates/liw-ui/locales/en.json` and `ru.json`. Imported files are copied to the config directory under `languages/<id>.json`. IDs may contain ASCII letters, digits, `_` and `-`, up to 32 characters; files are limited to 1 MiB. Loading a language changes the settings form; it does not change Android's locale.

## По-русски

Откройте шестерёнку в окне Android → «Предпочтения». Здесь доступны язык настроек и импорт JSON, запуск на весь экран, два независимых действия Esc и папка снимков. После выбора нажмите «Сохранить изменения». Снимок Android сохраняется по Ctrl+Shift+S. F11 и назначенная в игровом профиле клавиша захвата мыши остаются доступны при отключении Esc. Эта вкладка не меняет ресурсы контейнера и не требует root.
