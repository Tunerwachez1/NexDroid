# NexDroid v0.1.0-beta.1

## Установка одним архивом

Скачайте `NexDroid-v0.1.0-beta.1-linux-x86_64.tar.gz`, распакуйте и запустите:

```bash
cd NexDroid
bash scripts/install-nexdroid.sh
```

Архив содержит Rust-бинарники, исходники, Android Desktop/HardwareIme0.8 APK и установщик. На Arch/CachyOS x86_64 установщик сам устанавливает Waydroid; на новой установке загружает Android GAPPS с системными приложениями и добавляет наш Desktop/Manager. Предварительная ручная установка Waydroid не требуется. Нужны интернет, Wayland, systemd и разрешение администратора на установку пакетов. Данные существующего Android не очищаются.

## Что добавлено

- Автоматическое переключение между игровыми привязками и текстовым вводом по событиям Android EditorInfo/InputConnection.
- Возврат управления после закрытия поля; отпускание игровых клавиш/касаний при входе в чат.
- Сохранение F8 как временного ручного override и отдельный выключатель автоматики для каждой игры (`auto_text` в `cont_<package>.json`).
- Android Desktop/Manager, системный список приложений, закреплённый установленный Google Play, серо-зелёные обои, физическая клавиатура RU/EN/Caps.
- Исходники Android-компонента, готовый APK с SHA256 и единый beta-установщик.

## Что исправлено

- Для ввода в игровой чат больше не требуется каждый раз вручную переключать F8, если игра предоставляет Android InputConnection.
- Промежуточный finish/start при переходе между текстовыми полями не должен кратковременно включать игровые привязки.
- Состояние редактора учитывает TYPE_NULL и InputConnection: общий `mInputStarted` в Standoff остаётся true даже после закрытия чата и не подходит для переключения.
- Переключение режима отпускает нажатия и не превращает удерживаемую физическую клавишу в новое игровое нажатие.
- Новый установщик проверяет явный Success от Android PackageManager вместо тихого `waydroid app install`.

## Ограничения beta

Свежая установка NVIDIA native backend и ARMv7/ARM64 translation **пока не автоматизирована**. На NVIDIA установщик требует совместимый аппаратно проверенный backend и сохраняет штатный exact-version guard. Архив не является полным универсальным Android-образом для всех видеокарт. Google Play может требовать входа/регистрации устройства. Свежая установка на другом компьютере пока не проверена целиком.

Клавиатурная автоматика подтверждена пользователем на Standoff2; Rust-тесты, Android сборка/подпись, Python bridge tests и shell dry-run проверены. Проверка на всех играх, дистрибутивах и GPU не заявляется.

## English

This prerelease adds automatic game/text keyboard switching, per-game opt-out, the Android Desktop/HardwareIme0.8 APK with source, and a unified Arch/CachyOS x86_64 installer. Download the archive, extract it and run `bash scripts/install-nexdroid.sh`. It installs Waydroid automatically, initializes a new GAPPS Android and installs NexDroid/Android Desktop; existing app data is retained.

Fresh proprietary NVIDIA backend and ARM translation provisioning are not yet automatic. A compatible hardware-gated NVIDIA backend is still required; no driver checks are bypassed. Fresh-machine end-to-end installation is not yet validated. This is a beta release, not a universal preconfigured Android image.

NexDroid is based on [Liwinux](https://github.com/Liwinux-Project/liwinux), with upstream history and credits retained. GPL-3.0-or-later. Android/Waydroid and Google components keep their respective upstream licenses and are downloaded from official OTA during initialization; personal Android data and signing keys are not included.
