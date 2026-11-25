# Единая установка NexDroid / Unified installation

Установщик предназначен для **x86_64 Arch Linux, CachyOS и производных с pacman, systemd и Wayland**. Полный комплект содержит наш готовый Android 13 GAPPS: `dist/android/system.img` и `vendor.img`, Desktop/Manager, HardwareIme и Houdini ARMv7/ARM64. Отдельно скачивать Android не нужно. Интернет остаётся нужен для Linux-пакетов. Образы не находятся в Git: нужен полный архив с ними. Старый опубликованный v0.1.0-beta.1 ещё не содержит этот комплект.

```bash
# После распаковки полного NexDroid-архива:
cd NexDroid
bash scripts/install-nexdroid.sh
```

В готовом release-архиве уже находятся Rust-бинарники и APK, поэтому Cargo-сборка пропускается. После распаковки запустите тот же скрипт. При установке из Git он самостоятельно установит инструменты сборки и выполнит Cargo build. Пакеты pacman запрашивают обычное подтверждение; дистрибутивные GPU-драйверы не заменяются.

Что устанавливается:

- Waydroid и его системные зависимости;
- комплектный Android GAPPS с системными приложениями и ARMv7/ARM64;
- нативные NexDroid UI/daemon/helper и ярлыки;
- собственные Android Desktop/Manager, обои и HardwareIme;
- физическая клавиатура и канал автоматического переключения ввода.

Существующие Android-образы, аккаунты, приложения и конфигурация не сбрасываются. `init --force` не вызывается; существующий VANILLA не заменяется автоматически на GAPPS. Подпись APK проверяется Android при установке, SHA256 — установщиком. Root не включается автоматически. В настройках Android выбирается наш HOME/IME. Google Play требует обычного входа и может потребовать регистрации устройства.

## Реальные границы beta

Это **не универсальный готовый Android-образ для любого GPU**. Для Intel/AMD используются возможности штатного Waydroid. Для проприетарной NVIDIA необходим совместимый native backend и его аппаратный gate; текущий установщик проверяет его через `verify-active` и останавливается при отсутствии/несовпадении. Он не скачивает случайный драйвер, не подменяет номер версии и не включает программный fallback. Наш локально проверенный профиль620.73.04 не означает поддержку всех NVIDIA установок. Исходный NVIDIA-проект: [waydroid-nvidia](https://github.com/Shiro836/waydroid-nvidia).

ARMv7/ARM64 включены в новый комплектный образ. Существующая установка не заменяется, поэтому ей этот образ автоматически не добавляет ARM. Наличие Manager/Google Play не гарантирует запуск каждой игры или сертификацию устройства. Новые образы прошли offline-проверку файловой системы; загрузка на свежем другом компьютере пока не подтверждена.

## Сборка комплекта разработчиком

```bash
sudo bash scripts/build-android-bundle.sh "$PWD/dist/android"
```

Каталог назначения должен отсутствовать. Сборщик работает с копиями текущих системных образов в отдельном mount namespace, переносит только явно перечисленные ARM-компоненты и проверенный APK. Он не экспортирует `/data`, `overlay_rw`, аккаунты, приложения пользователя, Magisk payload или конфигурацию конкретного GPU. Ничего не меняет в работающем Android. Manifest и SHA256SUMS проверяются установщиком до установки пакетов. На новой системе локальные образы помещаются в штатный `/usr/share/waydroid-extra/images`; Waydroid init использует их без OTA. При отсутствии комплекта установка останавливается, а не подменяет его стандартным Android.

Это сборщик локального комплекта, а не AOSP build: для распространения производного GAPPS/Houdini-комплекта необходимы соответствующие условия распространения этих компонентов; GPL проекта не меняет их лицензии. NVIDIA host backend намеренно не переносится с машины сборщика.

На других дистрибутивах скрипт останавливается до установки пакетов. Сборка из исходников и [официальные инструкции Waydroid](https://docs.waydro.id/usage/install-on-desktops) доступны отдельно. Ни личный Android `/data`, ни аккаунты, ни ключ APK не входят в GitHub-архив.

Предварительный просмотр без установки:

```bash
bash scripts/install-nexdroid.sh --dry-run
```

## English

The beta installer supports x86_64 Arch/CachyOS-family Wayland/systemd desktops. The new full bundle includes custom Android 13 GAPPS system/vendor images, Desktop/HardwareIme and ARMv7/ARM64 translation. Run `bash scripts/install-nexdroid.sh` from the extracted full bundle: fresh installs use local preinstalled images, never stock OTA. Existing Android data/images are retained. A source checkout alone does not contain the images. Published v0.1.0-beta.1 predates this bundle.

Proprietary NVIDIA still requires a compatible, hardware-gated native backend. Google Play sign-in/registration can still be required. The images are filesystem-checked offline; fresh-machine boot has not yet been validated. Third-party GAPPS/Houdini redistribution terms remain separate from the project's GPL license.
