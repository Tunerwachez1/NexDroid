<p align="center">
  <a href="README.md"><strong><kbd>🇬🇧 Read in English</kbd></strong></a>
  &nbsp;·&nbsp;
  <strong>Русский</strong>
</p>

<p align="center">
  <img src="assets/banner-ru.png" alt="NexDroid beta — Android-игры на Linux" width="100%">
</p>

<p align="center">
  <a href="#статус"><img alt="статус" src="https://img.shields.io/badge/статус-публичная%20beta-f08a3c?style=flat-square"></a>
  <img alt="тесты" src="https://img.shields.io/badge/тесты-458%20проходят-4fd1c5?style=flat-square">
  <img alt="rust" src="https://img.shields.io/badge/rust-edition%202024-c8cfe4?style=flat-square">
  <img alt="лицензия" src="https://img.shields.io/badge/лицензия-GPL--3.0--or--later-9aa4c0?style=flat-square">
</p>

---

> [!WARNING]
> **NexDroid находится на стадии beta-тестирования.** Программа меняет
> пользовательские и системные службы Waydroid, проверена на
> ограниченном количестве компьютеров и пока не выпускается в виде
> готового пакета для дистрибутива. Перед установкой сделайте резервные
> копии важных данных.

# Что такое NexDroid

**NexDroid** — это написанная на Rust оболочка и игровой слой для
запуска Android-приложений через Waydroid на Linux. Android отображается
в отдельном окне с боковой панелью, полноэкранным режимом,
клавиатурно-мышиным управлением и диагностикой.

NexDroid — изменённый форк
**[Liwinux](https://github.com/Liwinux-Project/liwinux)**. История Git,
лицензия GPL и авторство upstream сохранены. NexDroid не является
официальным релизом Liwinux.

## Изменения NexDroid beta

- Android встроен в нативное Rust-окно с боковой панелью;
- F11 включает настоящий полноэкранный режим;
- русский редактор экранного управления;
- профили управления для каждой игры в `cont_<android.package>.json`;
- WASD-джойстик, нажатия, прицел, захват курсора, ЛКМ, ПКМ и
  расширенные клавиши;
- передача физической клавиатуры, Caps Lock и состояния раскладки RU/EN;
- автоматическое переподключение к каналу касаний Waydroid;
- ярлык рабочего стола Android и сценарий Google Play;
- сохраняемый профиль экрана и окно с фиксированными пропорциями;
- определение активной Android-игры для автовыбора профиля;
- исправления запуска с NVIDIA без привязки к одной desktop-версии
  драйвера;
- 458 автоматизированных Rust-тестов.

## Что исправлено относительно Liwinux

Большинство этих исправлений разрабатывалось до первой публичной фиксации
NexDroid и было опубликовано одним начальным beta-коммитом. Поэтому ссылка в
таблице повторяется: отдельных исторических коммитов не существовало, а
создавать искусственную историю задним числом мы не стали.

| Баг / проявление | Причина | Исправление в NexDroid | Коммит |
|---|---|---|---|
| Повторный запуск завершался сообщением `The compositor did not start` / `wayland-liw ... already in use` | Каждый процесс UI пытался занять один Wayland-сокет; передачи запроса уже запущенному экземпляру не было. | Добавлены блокировка владельца для пользователя и локальный командный сокет: следующие запуски передают запрос существующему процессу. | [`0c5984c`](https://github.com/Tunerwachez1/NexDroid/commit/0c5984cde7a6bd16b6cb55dbcfe340b77c0cbfed) |
| После переподключения Android пропадали касания и появлялся `Broken pipe (os error 32)` | Waydroid пересоздаёт `wl_touch_events`, а старый файловый дескриптор остаётся подключён к мёртвому FIFO. | Запись касаний определяет обрыв, заново открывает FIFO и переподключается без полного перезапуска UI. | [`0c5984c`](https://github.com/Tunerwachez1/NexDroid/commit/0c5984cde7a6bd16b6cb55dbcfe340b77c0cbfed) |
| Физическая клавиатура, Caps Lock и переключение RU/EN работали нестабильно | Встроенный Wayland-seat не сохранял и не передавал полное состояние keymap, группы раскладки и модификаторов. | Добавлена передача keymap/group/modifiers и тесты состояния клавиатуры. | [`0c5984c`](https://github.com/Tunerwachez1/NexDroid/commit/0c5984cde7a6bd16b6cb55dbcfe340b77c0cbfed) |
| Уже созданную кнопку управления нельзя было выбрать, передвинуть или переназначить | Нажатие в редакторе только создавало новый элемент; проверки попадания по маркеру и постоянного выбора не было. | Добавлены hit testing, выбор, перенос кликом/перетаскиванием, сдвиг стрелками, удаление и переназначение. | [`0c5984c`](https://github.com/Tunerwachez1/NexDroid/commit/0c5984cde7a6bd16b6cb55dbcfe340b77c0cbfed) |
| WASD-джойстик сохранялся, но персонаж не двигался | В профиль могла попасть незавершённая привязка четырёх направлений или повторяющийся триггер. | Сохранение продолжает назначение недостающих W/A/S/D и отклоняет неполные либо дублирующиеся привязки. | [`0c5984c`](https://github.com/Tunerwachez1/NexDroid/commit/0c5984cde7a6bd16b6cb55dbcfe340b77c0cbfed) |
| После переключения Android-приложения оставалось управление от другой игры | Профиль выбирался по запросу запуска, а не по реально активному Android-пакету. | Добавлены `ForegroundPackage`, отпускание зажатого ввода и автоматическая смена профиля с исключением launcher/system overlay. | [`0c5984c`](https://github.com/Tunerwachez1/NexDroid/commit/0c5984cde7a6bd16b6cb55dbcfe340b77c0cbfed) |
| Раскладки управления разных игр могли пересекаться | Старые профили были общими и не всегда привязывались к имени Android-пакета. | Добавлены атомарные JSON-профили `cont_<package>.json` для каждой игры, приоритет JSON и совместимость со старыми TOML. | [`0c5984c`](https://github.com/Tunerwachez1/NexDroid/commit/0c5984cde7a6bd16b6cb55dbcfe340b77c0cbfed) |
| Выбранный размер окна менялся после первого клика или изменения размера гостем | Preferred-size от Android перезаписывал профиль хоста, а закреплённый GPUI Wayland backend не соблюдал `is_resizable`. | Добавлены расчёт размера с сохранением пропорций и восстановление обычных границ окна без вмешательства в fullscreen/maximized. | [`0c5984c`](https://github.com/Tunerwachez1/NexDroid/commit/0c5984cde7a6bd16b6cb55dbcfe340b77c0cbfed) |
| F11 скрывал панель, но не включал настоящий полноэкранный режим | Полноэкранность хранилась как локальный флаг UI, состояние Linux-окна не менялось. | F11 вызывает нативное переключение fullscreen; панель следует реальному состоянию окна, Esc и редактор корректно выходят из режима. | [`0c5984c`](https://github.com/Tunerwachez1/NexDroid/commit/0c5984cde7a6bd16b6cb55dbcfe340b77c0cbfed) |
| При долгой игре с высоким FPS в GPU-памяти накапливались отрисованные кадры | Заменённые изображения Android не удалялись из атласа изображений GPUI. | Рендерер хранит только текущий и предыдущий кадр и явно освобождает более старые GPU-изображения. | [`0c5984c`](https://github.com/Tunerwachez1/NexDroid/commit/0c5984cde7a6bd16b6cb55dbcfe340b77c0cbfed) |

Внутренние имена `liw`, `liwd`, `liwinux` и `id.liwinux.*` пока сохранены
для совместимости с профилями, службами и D-Bus API Liwinux.

## Скриншоты

| Библиотека NexDroid | Google Play внутри NexDroid |
|---|---|
| ![Библиотека игр NexDroid](assets/screenshots/library.png) | ![Google Play во встроенном Android](assets/screenshots/play-store.png) |

| Игровое окно с боковой панелью | Полноэкранная игра |
|---|---|
| ![Standoff 2 в окне NexDroid](assets/screenshots/game-window.png) | ![Standoff 2 в полноэкранном режиме](assets/screenshots/fullscreen-game.png) |

## Сборка и установка

Требуется Linux с Wayland и уже работающая установка Waydroid. Основной
проверенный композитор — KWin. Пакеты для Arch/CachyOS и GPU-примечания
перечислены в [`DEPENDENCIES.txt`](DEPENDENCIES.txt).

```bash
git clone https://github.com/Tunerwachez1/NexDroid.git
cd NexDroid
cargo build --release --workspace

install -Dm755 target/release/liw ~/.local/bin/liw
install -Dm755 target/release/liwd ~/.local/bin/liwd
install -Dm755 target/release/liw-ui ~/.local/bin/liw-ui
install -Dm644 dist/systemd/liwd.service ~/.config/systemd/user/liwd.service
install -Dm644 -t ~/.local/share/liwinux/kwin scripts/kwin/*.js
./dist/install-desktop.sh
systemctl --user daemon-reload
systemctl --user enable --now liwd.service

sudo bash dist/install-helper.sh
```

Нужно собирать всё рабочее пространство через `--workspace`. Обычная команда
`cargo build --release` не собирает тяжёлые UI-крейты, так как это было
намеренно задано в upstream.

## Запуск

```bash
liw-ui
liw-ui --game dev.waydroid.manager.home
```

Первая команда открывает библиотеку NexDroid. Вторая сразу открывает
рабочий стол Android.

## Основные горячие клавиши

- `F11` — полный экран и возврат в оконный режим;
- `Esc` — освободить курсор или выйти из полного экрана;
- `F1` — включить или выключить захват курсора для камеры;
- `F8` — режим ввода текста в Android;
- `Ctrl+Shift+A` — открыть редактор управления.

## Архитектура

| Компонент | Назначение |
|---|---|
| `liw-ui` | Библиотека, игровое окно, боковая панель и редактор управления |
| `liw-compositor` | Встроенный Wayland-композитор для Android-поверхности |
| `liwd` | Пользовательская служба сессии, окон и управления |
| `liwd-helper` | Ограниченные привилегированные операции через polkit |
| `liw-input` | Преобразование клавиатуры и мыши в Android-касания |
| `liw` | Инструмент командной строки, тесты и диагностика |

## Статус

NexDroid — **публичная beta**. Текущая ветка собирается, а 458 тестов
проходят, но это не означает, что поддерживается каждый Android-образ,
GPU-драйвер и Wayland-композитор. Лучше всего проверены CachyOS/Arch,
KWin, **NVIDIA GeForce RTX 3050 Laptop GPU**, Android 13, Google Apps и
ARM-трансляция. AMD и Intel нуждаются
в более широком независимом тестировании.

## Авторство и лицензия

NexDroid основан на [Liwinux](https://github.com/Liwinux-Project/liwinux) и
использует Waydroid, наработки `waydroid-nvidia`, `waydroid_script`, XtMapper и другие
компоненты сообщества. Подробности сохранены в английском
[`README.md`](README.md) и [`FORK_NOTICE.md`](FORK_NOTICE.md).

Проект распространяется по **GNU General Public License v3.0 или более новой
версии (`GPL-3.0-or-later`)**. См. [`LICENSE`](LICENSE). Программа поставляется
без гарантий.
