# Phone profiles / Профили телефонов

Open the gear inside the Android window → **Phone / Телефон**. The dropdown offers the original Waydroid identity and three real device presets:

| Preset | Manufacturer | Android model | Device/product name |
| --- | --- | --- | --- |
| Google Pixel 6 | Google | Pixel 6 | oriole |
| Google Pixel 7 | Google | Pixel 7 | panther |
| Google Pixel 8 | Google | Pixel 8 | shiba |

Models and codenames: [Android Open Source Project device list](https://source.android.com/docs/setup/reference/build-numbers), [Pixel kernels](https://source.android.com/docs/setup/build/building-pixel-kernels).

Save changes, then reopen Android to apply. The current Android model is shown separately from the selected preset, since `ro.*` properties take effect only at container boot. Saving does not interrupt the running game. Original Waydroid profile restores the values present before NexDroid's first identity change. No apps, app data or Google Play data are cleared.

Only these five properties are changed:

- `ro.product.brand`
- `ro.product.manufacturer`
- `ro.product.model`
- `ro.product.name`
- `ro.product.device`

This changes reported device identity. It does not install a phone firmware, change Android version, emulate the phone's hardware, change supported ABIs/GPU/display, or provide device certification or game compatibility guarantees. Build fingerprints, verified-boot, security and root settings remain untouched.

The privileged helper validates the preset ID against a fixed allowlist and uses the existing one-time Android settings permission. It edits only the five allowed keys in `/var/lib/waydroid/waydroid.cfg` and `/var/lib/waydroid/waydroid_base.prop`. Both files are needed because Waydroid regenerates its base properties on init/upgrade, not every session start. Existing unrelated settings and sections are retained.

Original values, last written values and the selected profile are stored as root-owned JSON in `/var/lib/nexdroid/phone/state.json`. Atomic file replacement, rollback after a write failure, and detection of external changes protect the previous configuration. If a managed property is edited externally, saving fails with an explanation rather than overwriting it. Resolve that change against the saved original/last values before retrying. The helper does not accept arbitrary property names or property values from the UI.

## По-русски

Шестерёнка → «Телефон» → выберите Google Pixel 6, Pixel 7 или Pixel 8 → «Сохранить изменения». Для применения переоткройте Android. Перезапуск игры по нажатию «Сохранить» не выполняется. «Исходный профиль Waydroid» возвращает значения, сохранённые перед первым изменением. Во вкладке доступен отдельный показ текущей модели Android и повторное чтение профиля.
