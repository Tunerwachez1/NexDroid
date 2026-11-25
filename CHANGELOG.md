# История изменений / Changelog

## v0.1.0-beta.4 — 2026-11-26

### Интерфейс и установка

- В верхней панели, заголовке окна, разделе «О программе» и меню приложений появилось название **WAYTRIVO** с новым значком. Комплект устанавливает ярлык Linux и отдельные записи для значков окон в Wayland. Исполняемый файл сохранил имя `NexDroid`.
- Параллельный запуск standalone-установщика теперь блокируется. Фоновые процессы не наследуют блокировку. Ожидание первой загрузки Android ограничено по времени.
- Обновление существующей установки использует текущие Android-образы и данные. Переход на комплектный образ выполняется отдельно. На тестовой установке при таком переходе сохранились все 172 прежних пакета.

### Буфер обмена и клавиатура

- Встроенный композитор Smithay реализует текстовый Wayland selection для Composer HAL Android 13. В активном окне Android `Ctrl+V` передаёт текст из Linux, а копирование из Android публикует текст в Linux. В «Предпочтениях» общий буфер можно отключить.
- Мост ограничивает текст 256 КиБ и ограничивает время обмена внутри композитора. В standalone-runtime есть `wl-clipboard` и `xclip` для прежнего Binder-адаптера. Полноценный запуск standalone в X11 пока не проверен.
- `Alt+Shift` в окне Android передаёт команду переключения физической раскладки Android. Добавляйте нужные раскладки в «Физическая клавиатура → wayland_keyboard». Языки интерфейса Android и физические раскладки настраиваются отдельно.
- После загрузки Android фоновая задача восстанавливает каталог клавиатурного моста с UID пользователя Linux, GID Desktop APK и правами `0710`. Установщик больше не ждёт восстановления каталога при обычном обновлении. Для стандартной обработки физической раскладки комплект не назначает HardwareIme методом ввода по умолчанию.
- Для Google Assistant комплект однократно отключает компонент голосового окна, который запускался самим приложением Google. Google Play, Play Services и аккаунты сохраняются.

### Запуск и сеть

- Отсутствие IP у запущенного Android переводит сеанс в состояние Degraded без автоматического перезапуска. Восстановление критических сбоев учитывает время загрузки и лимит попыток.
- При настройке экрана пропускаются лишние команды сброса размеров и повторная установка уже заданного DPI. На тестовой машине лишний сброс вызывал сбой графического стека Android.
- В «Диагностике» появились проверка и исправление сети. NexDroid показывает мост, DHCP-аренду, исходящий интерфейс по маршруту трафика Android и наличие сети по умолчанию в Android. Для UFW исправление добавляет правила DHCP, DNS и пересылки для обнаруженных адресов и интерфейсов. Для nftables, iptables и firewalld доступны рекомендации по настройке.

### Проверено

- На живом Android 13 прошёл двусторонний обмен тестовым текстом с кириллицей, эмодзи и переносом строки между Linux и Composer HAL. Настольная сторона проверена на вложенном Weston в сеансе DriftWM.
- На комплектном образе выполнились отдельные ELF для ARMv7 и ARM64. В «О программе» видны `armeabi-v7a` и `arm64-v8a`.
- После сетевого восстановления Android получил ответ HTTPS 204 и открыл тестовую страницу в браузере. Проверены целостность standalone-комплекта и оба Android-образа.
- Тестовая система — CachyOS, DriftWM и NVIDIA. Переключение раскладки наблюдалось на живом Android, но стабильное прохождение всех повторов и сочетаний с глобальными клавишами хоста не подтверждено.

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
