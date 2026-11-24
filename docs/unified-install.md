# Единая установка NexDroid / Unified installation

Начальный автоматический установщик предназначен для **x86_64 Arch Linux, CachyOS и производных с pacman, systemd и Wayland**. Предварительно скачивать Waydroid отдельно не нужно: установщик сам устанавливает пакет, на новой установке вызывает `waydroid init -s GAPPS` и загружает Android через официальный OTA Waydroid. Нужны интернет, пароль администратора и достаточно свободного места для образов/сборки.

```bash
git clone https://github.com/Tunerwachez1/NexDroid.git
cd NexDroid
bash scripts/install-nexdroid.sh
```

В готовом release-архиве уже находятся Rust-бинарники и APK, поэтому Cargo-сборка пропускается. После распаковки запустите тот же скрипт. При установке из Git он самостоятельно установит инструменты сборки и выполнит Cargo build. Пакеты pacman запрашивают обычное подтверждение; дистрибутивные GPU-драйверы не заменяются.

Что устанавливается:

- Waydroid и его системные зависимости;
- образ Android GAPPS с системными приложениями на новой установке;
- нативные NexDroid UI/daemon/helper и ярлыки;
- собственные Android Desktop/Manager, обои и HardwareIme;
- физическая клавиатура и канал автоматического переключения ввода.

Существующие Android-образы, аккаунты, приложения и конфигурация не сбрасываются. `init --force` не вызывается; существующий VANILLA не заменяется автоматически на GAPPS. Подпись APK проверяется Android при установке, SHA256 — установщиком. Root не включается автоматически. В настройках Android выбирается наш HOME/IME. Google Play требует обычного входа и может потребовать регистрации устройства.

## Реальные границы beta

Это **не универсальный готовый Android-образ для любого GPU**. Для Intel/AMD используются возможности штатного Waydroid. Для проприетарной NVIDIA необходим совместимый native backend и его аппаратный gate; текущий установщик проверяет его через `verify-active` и останавливается при отсутствии/несовпадении. Он не скачивает случайный драйвер, не подменяет номер версии и не включает программный fallback. Наш локально проверенный профиль620.73.04 не означает поддержку всех NVIDIA установок. Исходный NVIDIA-проект: [waydroid-nvidia](https://github.com/Shiro836/waydroid-nvidia).

ARMv7/ARM64 translation не входит в штатный образ и не включается этим установщиком. Наличие Manager/Google Play не гарантирует запуск ARM-игр, сертификацию устройства или прохождение игровых проверок. Полная установка на свежем другом компьютере пока не подтверждена: установщик опубликован как beta, shell/dry-run проверены, работа APK и автоматики подтверждена на существующей системе.

На других дистрибутивах скрипт останавливается до установки пакетов. Сборка из исходников и [официальные инструкции Waydroid](https://docs.waydro.id/usage/install-on-desktops) доступны отдельно. Ни личный Android `/data`, ни аккаунты, ни ключ APK не входят в GitHub-архив.

Предварительный просмотр без установки:

```bash
bash scripts/install-nexdroid.sh --dry-run
```

## English

The beta installer supports x86_64 Arch/CachyOS-family Wayland/systemd desktops. Run `bash scripts/install-nexdroid.sh`: it installs Waydroid automatically, initializes a new GAPPS Android from official OTA, installs NexDroid binaries, Android Desktop/HardwareIme and shortcuts. Existing images and app data are retained. Release archives include binaries; a Git checkout builds them automatically. No separate manual Waydroid download is needed.

Proprietary NVIDIA still requires a compatible, hardware-gated native backend; this installer verifies an existing backend and does not provision one universally. ARM translation is not bundled. Google Play sign-in/registration can still be required. Fresh-machine installation has not yet been validated; this is a beta installer, not an assurance of compatibility on every GPU/distribution.
