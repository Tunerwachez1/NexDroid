# Готовый NexDroid / Standalone beta.2

Полный standalone-комплект отличается от старого архива beta.1 и от checkout
исходников. После распаковки откройте исполняемый **NexDroid**. Запускать Cargo,
устанавливать Waydroid, скачивать Android или вызывать install-nexdroid.sh не нужно.

В комплекте находятся:

- готовые Rust UI, менеджер сессии и привилегированный helper;
- Android 13 GAPPS с Desktop/HardwareIme и Houdini ARMv7/ARM64;
- private Linux runtime: Python, LXC, Waydroid, сеть и их библиотеки;
- NVIDIA glibc-shim, графические тесты двух ABI и legacy native payload;
- исходники NexDroid в Sources, package inventory и лицензии компонентов.

## Первый запуск

PolicyKit запросит системное разрешение для контейнера, сети и служб. NexDroid
сам размещает проверенный комплект в root-owned `/opt/nexdroid/releases/<id>`,
настраивает D-Bus/systemd и использует готовые локальные Android-образы.
Для свежей NVIDIA установки выполняются hardware gates, а не переносится
готовый результат проверки с компьютера разработчика. HOME/клавиатура Android
и пользовательская служба раскладки настраиваются автоматически.

Обычное окно и менеджер сессии работают от вашего пользователя. Повторный запуск
готовой версии не повторяет начальную системную подготовку. Сборка, package
manager и OTA-загрузка Android при этом не используются. Для первого запуска
нужен запас места на распакованный комплект и root-owned копию (около 13 ГБ
без учёта игр; на Btrfs reflink уменьшает фактическое дублирование).

На существующем ПК приложения, аккаунты и Android images не сбрасываются.
Открытую игру bootstrap не завершает: перед первой миграцией нужно закрыть её
сессию. Незавершённый fresh boot оставляет pending marker и журнал, а не
объявляется готовым. В комплект не копируются личный `/data`, root policies,
аккаунты и ключ подписи APK. Настройки root по умолчанию не включаются.

## Реальные требования beta

Это x86_64 Linux с Wayland, systemd, PolicyKit, доступными user namespaces,
Binder и поддержкой Android-контейнера в ядре. Сам GPU-драйвер и ядро остаются
компонентами Linux. Bubblewrap подставляет private `/usr`, сохраняя доступ к
host devices/modules/firmware. Видеодрайвер Linux не заменяется.

Новая NVIDIA установка использует **glibc-shim с библиотеками драйвера самого
пользователя**, без привязки к заранее собранному RM-профилю 620.73.04. Нужны
64- и 32-битные библиотеки NVIDIA, соответствующие загруженному модулю ядра.
Проверяются реальные импорты glibc/libdrm и 12 аппаратных Vulkan/ANB/ASTC/ETC2
тестов. После обновления драйвера подготовка повторяется автоматически перед
запуском контейнера. Обновление требует завершённой Android-сессии. Несовместимый
новый ABI, отсутствие библиотек или ошибка GPU останавливают запуск с причиной;
сравнение host userspace/kernel сохраняется, отдельная сборка профиля не нужна.

Linux UI/compositor тоже используют host NVIDIA через отдельный lookup-каталог;
библиотеки драйвера машины сборки исключены из private runtime. Системная libc
не подставляется вместо комплектной. Существующие native-установки не мигрируются
неявно: их данные и backend сохраняются. Миграция вызывается отдельно
разработчиком, только при остановленном Android. Подробнее: [host-driver](nvidia-host-driver.md).

Это не обещание поддержки всех будущих драйверов, GPU или дистрибутивов. Реальная
проверка выполнена на RTX 3050 Laptop / 620.73.04: standalone fresh init, 12 GPU
cases, генерация конфигурации, verify-active и cached restart. При живом запуске
подтверждены sys.boot_completed=1, EGL=angle, Vulkan=nvidia и получение кадров
Rust-окном на NVIDIA. Пользователь подтвердил проверку Android и Standoff 2.
Другие версии драйверов и ПК требуют отдельных проверок.
Native NVDEC media gate не является частью glibc-shim проверки. Google Play может
требовать входа и регистрации.

Политики GPL NexDroid/Liwinux и лицензии зависимостей сохранены. GAPPS и Houdini
имеют отдельные условия распространения; GPL оболочки их не заменяет.
Standalone распространяется в [beta.2](https://github.com/Tunerwachez1/NexDroid/releases/tag/v0.1.0-beta.2).
Прежний beta.1 этого runtime не содержит.

## Диагностика и сборка разработчиком

Журнал: `~/.local/state/nexdroid/standalone.log`.
`./NexDroid --check` проверяет hashes, symlinks, executable modes и комплектные
Waydroid/LXC/Python без изменения служб и данных Android.

Только для разработчика: `scripts/build-runtime.py`, затем
`scripts/build-standalone.py`. Аргументы `--help` у второго показывают входные
каталоги. Исходный Android экспортирует `scripts/build-android-bundle.sh`.
Пользователь готового комплекта эти команды не выполняет.

Проверки: `scripts/test-standalone-payload.py` проверяет повреждённые/добавленные
файлы, symlink подмену, executable modes и parent traversal.
`scripts/check-standalone-fresh.sh` проверяет local init и GPU gates в отдельных
mount/work деревьях, сохраняя host службы и Android data.

## English

Extract the full standalone bundle and open **NexDroid**. No separate Waydroid,
Android download, package manager command or source build is required. One-time
PolicyKit authorization prepares root-owned runtime/services; subsequent launches
use the prepared version. Existing Android data is preserved. See requirements
above: x86_64 Wayland/systemd Linux with Binder/user namespaces.
Fresh NVIDIA installations use hardware-gated glibc-shim with the host driver,
not a precompiled exact-version RM profile. Both NVIDIA library ABIs must match
the kernel module; driver updates trigger preparation before container startup.
Existing native installs are retained. This is not universal driver support or
a claim of validation on every distribution. The older published beta.1 archive
does not include this standalone runtime.
