# История изменений / Changelog

## v0.1.0-beta.2 — 2026-11-25

**Beta testing / Бета-тестирование.** Based on [Liwinux](https://github.com/Liwinux-Project/liwinux); upstream history and credits are retained. NexDroid code is GPL-3.0-or-later. Bundled dependencies, Google apps and ARM translation retain their respective licenses.

## Русский

Теперь можно скачать готовый комплект, распаковать и открыть **NexDroid**. Отдельно устанавливать Waydroid, скачивать Android или собирать его пользователю не требуется.

### Добавлено

- Встроенный Android 13 GAPPS с системными приложениями, Desktop/Manager, HardwareIme и Houdini ARMv7/ARM64. Без личных аккаунтов, пользовательского `/data` и ключей подписи.
- Готовые Rust UI/daemon/helper, нативный запускатель и private Linux runtime: Waydroid, Python, LXC и зависимости. Первичная подготовка выполняется через PolicyKit; пакетный менеджер и Cargo при обычном запуске не вызываются.
- NVIDIA через **glibc-shim и системные библиотеки компьютера пользователя** вместо обязательного RM-профиля 620.73.04. Поддерживаются оба ABI, выполняются проверки импортов и 12 аппаратных тестов; после изменения драйвера подготовка повторяется автоматически перед стартом контейнера.
- Описание установки и ограничений на русском и английском, исходники NexDroid и лицензии компонентов внутри комплекта.

### Исправлено

- Управление мышью: привязка обзора кешируется вместо повторного поиска и копирования на каждом движении; движение камеры отправляется до последующих событий кнопок, при захвате палец камеры создаётся первым. Это оптимизация пути ввода, без обещания нулевой задержки и без реализации TCE.

- Несовпадение private `/usr` и системного драйвера: host-библиотеки обнаруживаются до подстановки `/usr`, монтируются read-only и используются Android, Linux UI и композитором. Библиотеки NVIDIA машины сборки исключены из runtime.
- Чёрный экран из-за выбора Intel/Mesa после перехода на glibc-shim: Rust учитывает все NVIDIA-бэкенды при выборе GPU. Exact-RM guard старого native-режима сохранён.
- Разные Binder-контексты между службами и private runtime: устройства подготавливаются в host namespace и используются совместно.
- Расхождение путей `current` и `releases/<id>`: runtime использует canonical paths, совместимые с проверкой LXC mounts.
- Ложный отказ проверки payload из-за подключённого host mirror: проверка выполняется вне mount namespace, контроль файлов и SHA256 сохранён.
- Ошибка миграции существующего `images_path`: при регенерации свойств сохраняются OTA-параметры конфигурации; ничего не скачивается и `/data` не очищается.
- Повторный re-entry UI/daemon не подставляет private `/usr` вместо исходного host mirror.

### Проверено

- 194 теста `liw-core`, 11 host-driver regression tests и 6 payload tests.
- Vulkan compute/graphics, dma-buf interop, ANB, ASTC и ETC2 — 12 аппаратных случаев для Android x86_64/x86.
- Linux EGL/GLES: настоящий NVIDIA renderer, контрольный пиксель и повторный вход в runtime.
- Live migration на **RTX 3050 Laptop, NVIDIA 620.73.04**: Android `boot_completed=1`, ANGLE + NVIDIA Vulkan, поток кадров в Rust-окно. **Пользователь подтвердил проверку Android и Standoff 2.** Это не обещание такого же FPS или совместимости на любом ПК.

### Установка и требования

Скачайте архив `NexDroid-v0.1.0-beta.2-linux-x86_64.tar.zst`, распакуйте и запустите файл `NexDroid` обычным пользователем в Wayland. При первом запуске подтвердите системный запрос PolicyKit. Контрольная сумма находится в `SHA256SUMS`.

Нужны x86_64 Linux, Wayland, systemd, PolicyKit, user namespaces и Binder в ядре. На NVIDIA нужны системные 64- и 32-битные библиотеки, соответствующие модулю ядра. Неизвестный несовместимый ABI не маскируется; поддержка всех драйверов/GPU/дистрибутивов не заявляется. Существующий native backend не переключается неявно; миграция выполняется на остановленном Android. Google Play может требовать входа/регистрации. Планируйте около 13 ГБ для распакованного и установленного комплектов, дополнительно место под игры.

[Подробности](https://github.com/Tunerwachez1/NexDroid/blob/master/docs/standalone.md) · [NVIDIA](https://github.com/Tunerwachez1/NexDroid/blob/master/docs/nvidia-host-driver.md) · [README на русском](https://github.com/Tunerwachez1/NexDroid/blob/master/README_RU.md)

## English

Download the standalone archive, extract it and open **NexDroid** as your desktop user. It includes custom Android 13 GAPPS, Desktop/Manager, HardwareIme, ARMv7/ARM64 translation, Rust binaries and a private Waydroid/Python/LXC runtime. No separate Android download, Waydroid install or source build is required. First-run system preparation uses PolicyKit. Personal Android data/accounts and APK signing keys are not included.

Fresh NVIDIA installs use the host driver through glibc-shim rather than a precompiled exact-version RM profile. Both library ABIs are checked and hardware-gated; driver changes trigger preparation before container startup. The host driver is also used by the Rust UI/compositor. Fixed shared Binder setup, canonical release paths, payload verification outside the host mirror, configuration regeneration for existing image paths and graphics selection for all NVIDIA modes.

Captured mouse input also caches the look binding and dispatches camera motion before subsequent button frames. This is an input-path optimization, not a zero-latency guarantee or TCE implementation.

Validated: 194 core tests, 17 host-driver/payload regression tests, 12 Android GPU cases and Linux EGL/GLES pixel smoke. Live Android boot and rendering were measured on RTX 3050 Laptop / NVIDIA 620.73.04; the owner confirmed Android and Standoff 2. Other driver versions and machines remain unvalidated. No universal compatibility or game FPS claim is made.

Requires x86_64 Wayland/systemd Linux, PolicyKit, user namespaces and Binder; NVIDIA needs matching 64/32-bit host libraries. Existing native installations are retained unless explicitly migrated with Android stopped. See the linked installation documentation and included component licenses. This is a **prerelease**, not a stable distribution package.
