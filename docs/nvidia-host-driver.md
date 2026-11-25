# NVIDIA: системный драйвер через glibc-shim

Новый standalone выбирает glibc-shim при свежей установке на NVIDIA. Android
загружает неизменённые Linux-библиотеки пользователя через адаптер glibc→bionic.
RM-профиль под одну точную версию больше не является обязательным. Linux-драйвер
и его пакеты NexDroid не заменяет. Сам Android, ARMv7/ARM64 и Linux runtime входят
в комплект, Rust UI/keymapper остаются прежними.

## Подготовка

1. `host-nvidia.py` запускается через `host-python` в настоящем host namespace.
   Находит библиотеки обоих ABI через host loader cache, проверяет kernel/userspace,
   сохраняет canonical paths, hashes, PCI GPU, host glibc и fingerprint комплекта.
2. `runtime-exec` открывает read-only зеркало настоящих `/usr`, `/lib`, `/lib64`
   до подстановки private `/usr`. Адаптер обеих копий `nvidia_native.py` использует
   root-owned manifest. Пользовательский или изменяемый другими manifest отвергается.
3. Linux NVIDIA libs подключаются через отдельный NVIDIA-only lookup; host libc
   в lookup не попадает. GLX/Vulkan kernel match также проверяется. UI и compositor
   используют этот путь. Proprietary библиотеки NVIDIA сборщика не входят в runtime.
4. Проверяются glibc/libdrm imports комплектного shim для обоих ABI. Реальные GPU
   тесты выполняются до смены активного config/link: compute, graphics, interop,
   ANB, ASTC и ETC2, Android x86_64 + x86. Гостевой linker/runtime берутся из system.img.
5. Только после PASS создаются mounts/properties, выбирается glibc-shim и вызывается
   штатный `verify-active`. Ошибка возвращает прежние config/link и регенерирует
   конфигурацию. `/data`, аккаунты, APK и overlays не сбрасываются.
6. Перед запуском контейнера root service снова определяет библиотеки. Если hashes,
   GPU, glibc или bundle изменились, gate повторяется. Иначе проверяется готовая
   конфигурация. Дополнительного polling в игровом вводе нет.

Отсутствующие 32-битные NVIDIA библиотеки показываются с подсказкой upstream для
дистрибутива. Некорректный ABI не объявляется совместимым по одному номеру версии.
Обновления Linux-драйвера во время игры не применяются к живой сессии.

## Существующие установки

Прежний native backend и его exact-profile guard сохранены. Обновление комплекта
не переключает существующий Android неожиданно. Для разработчика, после закрытия
сессии, доступна подготовка миграции:

```sh
sudo /opt/nexdroid/current/host-python /opt/nexdroid/current/host-nvidia.py migrate
```

Это команда новой сборки, а не старого GitHub beta.1. Она сохраняет данные, требует
успешных GPU gates и откажет при работающем контейнере. Поддержка всех будущих
NVIDIA выпусков заранее не гарантируется; неподдержанный импорт требует обновить
адаптер, а не выключить проверку.

## Проверки разработчика

```sh
python3 scripts/test-host-nvidia.py
python3 scripts/test-standalone-payload.py
sudo bash scripts/check-standalone-shim.sh /path/to/bundle /new/test/directory
```

Последняя команда использует отдельные mount/network namespaces, config/state,
выполняет fresh init без OTA, GPU gates, генерацию LXC, verify-active и cached
повтор. Службы и работающий Android host не трогает. Linux smoke создаёт EGL/GLES
pbuffer на NVIDIA, очищает и читает пиксель, проверяет renderer и mapping библиотек.
Этот скрипт не проверяет видимое окно или FPS игры. Отдельно после live migration
измерены boot_completed=1, EGL=angle, Vulkan=nvidia и поток кадров в Rust-окно.
Пользователь подтвердил проверку Android и Standoff 2 на этой сборке.

Проверено: RTX 3050 Laptop, NVIDIA 620.73.04, оба ABI. Другой драйвер не блокируется
из-за отсутствия его RM-профиля, но требует собственных успешных проверок на машине.
GPL NexDroid не заменяет лицензии glibc-shim, Mesa и NVIDIA. Upstream notices и
исходники компонентов сохраняются в комплекте.
