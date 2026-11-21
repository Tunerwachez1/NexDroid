# Root-права (Magisk)

Шестерёнка → «Дополнительно» → **Root-права (Magisk)** → сохраните изменения → переоткройте Android через NexDroid. Переключатель показывает выбранный режим, строка «Root сейчас активен» — состояние работающего Magisk. До перезапуска может отображаться ожидание применения.

Root предназначен для Android-приложений. Magisk выдаёт разрешения по отдельности; NexDroid не добавляет политики автоматического разрешения всем приложениям. При активном root кнопка «Открыть разрешения Magisk» открывает установленный менеджер. Некоторые игры могут отказываться работать при включённом root. Переключатель не скрывает root и не меняет проверки приложений.

## Дополнительный runtime для новой установки

Поддерживается официальный Magisk 31.0. Само приложение Magisk APK без runtime не даёт root. Подготовьте payload один раз (это ещё не включает root):

```bash
curl -fL https://github.com/topjohnwu/Magisk/releases/download/v31.0/Magisk-v31.0.apk -o Magisk-v31.0.apk
sudo python3 scripts/prepare-root-runtime.py --apk ./Magisk-v31.0.apk
```

Установщик сверяет фиксированный SHA256 официального APK, извлекает только allowlist native binaries/scripts и stub APK, поддерживает x86_64/aarch64. Данные игр, настройки графики, host kernel и телефонные boot-разделы не меняются. Не используйте перепрошивку boot из стандартного Magisk Install/Reinstall: Waydroid использует другой контейнерный способ запуска. Для него Magisk может сообщать о дополнительной настройке, несмотря на работающие базовые root-запросы.

Если на этом компьютере уже есть root-only `magisk-disabled-*` backup прежней установки, NexDroid импортирует из него только публичный runtime и соответствующий stub при включении. Исходный backup, ключи, policies, DB и приватные app data не экспортируются/не копируются. Это сохраняет identity установленного менеджера. Portable installer отказывается заменять такую установку.

## Применение и отключение

Намерение хранится в `/var/lib/nexdroid/root/settings.json`, payload — в `runtime/` того же root-owned каталога. На следующем старте supervisor после NVIDIA preflight вызывает helper, который требует STOPPED-контейнер и устанавливает только собственные `nexdroid-root.rc` и `nexdroid-root/bootstrap.sh` в system overlay. Bootstrap подготавливает контейнерный Magisk и apps-only режим; подписанные APK и trust checks не подменяются. Существующие пользовательские root policies и настройки менеджера не удаляются.

Отключение убирает только два сверенных NexDroid hook на следующем остановленном старте. Runtime/data сохраняются для повторного включения; Linux driver, ARM translation и другие overlay не удаляются. Если hook изменён вне NexDroid или обнаружены старые чужие hooks, операция останавливается с ошибкой вместо перезаписи. Поддерживается управление этим Magisk runtime; произвольные сторонние root-установки не удаляются автоматически.

## English

Gear → Advanced → **Root access (Magisk)**. Save and reopen Android to apply. The checkbox is the requested mode; the status line reports the running daemon. Apps use individual Magisk approvals, never automatic grants to all apps. The optional runtime is prepared from the digest-verified official Magisk 31.0 APK; existing disabled installations preserve their manager identity. Disabling removes only NexDroid's verified boot hooks and keeps user data. This does not patch the Linux kernel, host driver or an Android boot image.
